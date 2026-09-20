use crate::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibinputDeviceKind {
    Pointer,
    Keyboard,
    Touch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LibinputDeviceDescriptor {
    pub seat: SeatId,
    pub device: DeviceId,
    pub kind: LibinputDeviceKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibinputEventIngest {
    Accepted,
    UnknownDevice,
    SeatMismatch,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LibinputEventSource {
    devices: BTreeMap<DeviceId, LibinputDeviceDescriptor>,
    pending: Vec<InputEventPacket>,
}

impl LibinputEventSource {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_device(&mut self, device: LibinputDeviceDescriptor) {
        self.devices.insert(device.device, device);
    }

    pub fn remove_device(&mut self, device: DeviceId) -> Option<LibinputDeviceDescriptor> {
        self.devices.remove(&device)
    }

    pub fn device(&self, device: DeviceId) -> Option<&LibinputDeviceDescriptor> {
        self.devices.get(&device)
    }

    pub fn devices(&self) -> impl Iterator<Item = &LibinputDeviceDescriptor> {
        self.devices.values()
    }

    pub fn push_event(&mut self, event: InputEventPacket) -> LibinputEventIngest {
        match event.kind {
            // An announcement is the registration. A device offering none of
            // the kinds this source routes is announced and left unregistered,
            // so its later events, if it ever had any, stay unknown.
            sophia_protocol::InputEventKind::DeviceAdded {
                keyboard,
                pointer,
                touch,
                ..
            } => {
                let kind = if keyboard {
                    Some(LibinputDeviceKind::Keyboard)
                } else if pointer {
                    Some(LibinputDeviceKind::Pointer)
                } else if touch {
                    Some(LibinputDeviceKind::Touch)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    self.register_device(LibinputDeviceDescriptor {
                        seat: event.seat,
                        device: event.device,
                        kind,
                    });
                }
                self.pending.push(event);
                return LibinputEventIngest::Accepted;
            }
            sophia_protocol::InputEventKind::DeviceRemoved => {
                if self.remove_device(event.device).is_none() {
                    return LibinputEventIngest::UnknownDevice;
                }
                self.pending.push(event);
                return LibinputEventIngest::Accepted;
            }
            _ => {}
        }
        let Some(device) = self.devices.get(&event.device) else {
            return LibinputEventIngest::UnknownDevice;
        };
        if device.seat != event.seat {
            return LibinputEventIngest::SeatMismatch;
        }

        self.pending.push(event);
        LibinputEventIngest::Accepted
    }

    pub fn drain_events(&mut self) -> Vec<InputEventPacket> {
        self.pending.drain(..).collect()
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibinputPollReport {
    pub polled: usize,
    pub accepted: usize,
    pub rejected: Vec<LibinputEventIngest>,
}

pub trait NonBlockingInputPoller {
    fn poll_ready(&mut self) -> io::Result<Vec<InputEventPacket>>;
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LibinputPhysicalInputAdapter<P> {
    poller: P,
    source: LibinputEventSource,
}

impl<P> LibinputPhysicalInputAdapter<P> {
    pub fn new(poller: P, source: LibinputEventSource) -> Self {
        Self { poller, source }
    }

    pub fn source(&self) -> &LibinputEventSource {
        &self.source
    }

    pub fn source_mut(&mut self) -> &mut LibinputEventSource {
        &mut self.source
    }

    pub fn poller(&self) -> &P {
        &self.poller
    }

    pub fn poller_mut(&mut self) -> &mut P {
        &mut self.poller
    }

    pub fn into_source(self) -> LibinputEventSource {
        self.source
    }
}

impl<P> LibinputPhysicalInputAdapter<P>
where
    P: NonBlockingInputPoller,
{
    pub fn poll_once(&mut self) -> io::Result<LibinputPollReport> {
        let events = self.poller.poll_ready()?;
        let polled = events.len();
        let mut accepted = 0;
        let mut rejected = Vec::new();

        for event in events {
            match self.source.push_event(event) {
                LibinputEventIngest::Accepted => accepted += 1,
                rejected_outcome => rejected.push(rejected_outcome),
            }
        }

        Ok(LibinputPollReport {
            polled,
            accepted,
            rejected,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct QueuedInputPoller {
    queued: Vec<InputEventPacket>,
}

impl QueuedInputPoller {
    pub fn new(queued: Vec<InputEventPacket>) -> Self {
        Self { queued }
    }

    pub fn push(&mut self, event: InputEventPacket) {
        self.queued.push(event);
    }

    pub fn queued_len(&self) -> usize {
        self.queued.len()
    }
}

impl NonBlockingInputPoller for QueuedInputPoller {
    fn poll_ready(&mut self) -> io::Result<Vec<InputEventPacket>> {
        Ok(self.queued.drain(..).collect())
    }
}
