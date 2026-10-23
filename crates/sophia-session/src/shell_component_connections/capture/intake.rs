//! DMA-BUF offer intake on the native maintenance turn. A destination is
//! prepared and its physical bound reserved before ImageAdmitted; the logical
//! capture reservation and that ticket then wait together for the Source.
use super::endpoint::Backend;
use super::*;
use crate::shell_capture_owner::MAX_JOBS;
use sophia_protocol::{ContentReason, OutputId};
use sophia_renderer_live::LiveRendererSnapshotReservation;
use std::{io, time::Instant};

/// The one outstanding destination preparation the native owner allows.
struct Preparing {
    key: ComponentConnectionKey,
    offer_id: u64,
    deadline: Instant,
    requested: bool,
    /// Proven bound and its reservation, kept across admission retries.
    ticket: Option<(u64, LiveRendererSnapshotReservation)>,
}

#[derive(Default)]
pub(crate) struct Intake {
    preparing: Option<Preparing>,
}

/// Renderer image ids are caller-assigned. Shell captures own the bit-63
/// half of the id space; X presentation uses small Present transaction ids.
const SHELL_IMAGE_IDS: u64 = 1 << 63;

/// Process-global, so neither endpoint recreation nor a new connection
/// table can reuse an id. Exhaustion refuses submission rather than wrap.
fn next_image_id() -> Option<sophia_renderer_live::LiveRendererImageId> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
            (n < SHELL_IMAGE_IDS - 1).then_some(n + 1)
        })
        .ok()?;
    Some(sophia_renderer_live::LiveRendererImageId::from_raw(
        SHELL_IMAGE_IDS | n,
    ))
}

impl Intake {
    pub(super) fn preparing(&self) -> bool {
        self.preparing.is_some()
    }
}

enum Step {
    Wait,
    Refuse(ContentReason),
    Admit(u64),
}

impl ShellComponentConnections {
    /// The first phase demands the single output the granted device drives.
    fn demand_capture_output(&mut self, backend: &impl Backend) {
        let mut devices = self.connections.iter().filter_map(|c| c.capture_device);
        let Some(device) = devices.next() else {
            return;
        };
        if devices.any(|other| other != device) {
            return;
        }
        if let Some(output) = backend.capture_output(device) {
            self.capture_link.demand_output(output, &self.capture);
        }
    }

    pub(super) fn service_capture_intake(
        &mut self,
        backend: &mut impl Backend,
        now: u64,
        instant: Instant,
        wake: &sophia_wake::Notifier,
    ) -> Vec<(ComponentConnectionKey, ShellTransportError)> {
        self.demand_capture_output(backend);
        let mut errors = Vec::new();
        for slot in 0..self.connections.len() {
            let connection = &mut self.connections[slot];
            let Some((grant, ComponentConnectionPhase::Connected)) = connection.attempt else {
                continue;
            };
            if connection.capture_device.is_none()
                || !connection.transport.capture_image_channel_ready()
            {
                continue;
            }
            if let Err(error) = connection.transport.enable_capture_image_admission() {
                errors.push((ComponentConnectionKey { slot, grant }, error));
                continue;
            }
            while let Some(source) = self.connections[slot].transport.take_capture_image_source() {
                self.receive_capture_source(slot, grant, source);
            }
        }
        if let Err(error) = self.service_capture_offer(backend, now, instant, wake) {
            errors.push(error);
        }
        self.submit_received_captures(now, instant);
        errors
    }

    /// Moves each received source, with its pre-reserved destination, into
    /// the renderer owner. A missing endpoint keeps it held until its
    /// deadline; a renderer refusal marks it for pre-use settlement.
    pub(super) fn submit_received_captures(&mut self, now: u64, instant: Instant) {
        for binding in self.capture_sources.submittable() {
            // An exhausted namespace leaves the source to expire pre-use.
            let Some(image) = next_image_id() else {
                return;
            };
            // The held ticket supplies the bound; zero never reserves anything.
            let _ = self.submit_retained_capture_source(binding, image, 0, now, instant);
        }
    }

    fn service_capture_offer(
        &mut self,
        backend: &mut impl Backend,
        now: u64,
        instant: Instant,
        wake: &sophia_wake::Notifier,
    ) -> Result<(), (ComponentConnectionKey, ShellTransportError)> {
        if self.capture_intake.preparing.is_none() {
            let Some(next) = self.next_capture_offer() else {
                return Ok(());
            };
            self.capture_intake.preparing = Some(next);
        }
        let preparing = self.capture_intake.preparing.as_ref().expect("preparing");
        let key = preparing.key;
        let current = self.connections.get(key.slot).and_then(|c| {
            (c.attempt == Some((key.grant, ComponentConnectionPhase::Connected)))
                .then(|| c.transport.pending_capture_image_offer())
                .flatten()
        });
        let Some((metadata, _)) = current.filter(|(m, _)| m.offer_id == preparing.offer_id) else {
            // Withdrawn by disconnect or refusal elsewhere; its ticket is unused.
            self.capture_intake.preparing = None;
            return Ok(());
        };
        let step = self.prepare_capture_offer(backend, metadata, instant, wake);
        let result = match step {
            Step::Wait => return Ok(()),
            Step::Refuse(reason) => self.connections[key.slot]
                .transport
                .refuse_pending_image(reason)
                .map(|()| true),
            Step::Admit(bound) => {
                let connection = &mut self.connections[key.slot];
                match connection
                    .transport
                    .admit_capture_image_offer(&mut self.epochs, bound, now)
                {
                    Ok(admission) => {
                        let (_, ticket) = self
                            .capture_intake
                            .preparing
                            .take()
                            .and_then(|p| p.ticket)
                            .expect("admitted with a held ticket");
                        self.hold_capture_admission(key.slot, key.grant, admission, ticket);
                        return Ok(());
                    }
                    // Journal or budget pressure: keep the ticket until the deadline.
                    Err(ShellTransportError::ContentQueueSaturated) => return Ok(()),
                    Err(ShellTransportError::WrongContentGrant) => connection
                        .transport
                        .refuse_pending_image(ContentReason::Stale)
                        .map(|()| true),
                    Err(ShellTransportError::WrongContentRecord) => connection
                        .transport
                        .refuse_pending_image(ContentReason::Malformed)
                        .map(|()| true),
                    Err(error) => Err(error),
                }
            }
        };
        match result {
            Ok(_) => {
                self.capture_intake.preparing = None;
                Ok(())
            }
            // A refusal held by journal pressure retries on a later turn.
            Err(ShellTransportError::ContentQueueSaturated) => Ok(()),
            Err(error) => {
                self.capture_intake.preparing = None;
                Err((key, error))
            }
        }
    }

    /// Oldest pending DMA-BUF offer of any capture slot, once the shared
    /// four-job bound has room for one more admission.
    fn next_capture_offer(&self) -> Option<Preparing> {
        if self.capture_pending() >= MAX_JOBS {
            return None;
        }
        self.connections
            .iter()
            .enumerate()
            .filter(|(_, c)| c.capture_device.is_some())
            .find_map(|(slot, c)| {
                let (grant, phase) = c.attempt?;
                if phase != ComponentConnectionPhase::Connected {
                    return None;
                }
                let (metadata, deadline) = c.transport.pending_capture_image_offer()?;
                Some(Preparing {
                    key: ComponentConnectionKey { slot, grant },
                    offer_id: metadata.offer_id,
                    deadline,
                    requested: false,
                    ticket: None,
                })
            })
    }

    fn prepare_capture_offer(
        &mut self,
        backend: &mut impl Backend,
        metadata: sophia_protocol::shell_files::ShellImageMetadata,
        instant: Instant,
        wake: &sophia_wake::Notifier,
    ) -> Step {
        let destination = self.capture_link.destination().cloned();
        let preparing = self.capture_intake.preparing.as_mut().expect("preparing");
        if instant >= preparing.deadline {
            return Step::Refuse(ContentReason::Timeout);
        }
        let Some(destination) = destination
            .filter(|d| d.epoch.is_valid() && d.output == OutputId::from_raw(metadata.output_id))
        else {
            return Step::Refuse(ContentReason::OutputLost);
        };
        if let Some((bound, _)) = &preparing.ticket {
            return Step::Admit(*bound);
        }
        if !preparing.requested {
            // A refused or withdrawn offer can leave its prepare in flight, and
            // the backend allows one; drain it, since its bound fits another size.
            let _ = backend.poll_prepare();
            match backend.prepare(destination.output, metadata.width, metadata.height, wake) {
                Ok(()) => preparing.requested = true,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Step::Wait,
                Err(_) => return Step::Refuse(ContentReason::RendererFailed),
            }
        }
        let bound = match backend.poll_prepare() {
            Ok(Some(bound)) => bound,
            Ok(None) => return Step::Wait,
            Err(_) => return Step::Refuse(ContentReason::RendererFailed),
        };
        // A full renderer budget is transient until the offer's deadline.
        match destination.budget.reserve_capture(bound) {
            Ok(ticket) => {
                preparing.ticket = Some((bound, ticket));
                Step::Admit(bound)
            }
            Err(_) => {
                preparing.requested = false;
                Step::Wait
            }
        }
    }
}
