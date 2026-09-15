//! Bounded ownership of renderer threads, including threads still inside a driver call.

use std::io;
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

pub(super) const WORKER_CAPACITY: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct DeviceIdentity {
    pub device: u64,
    pub inode: u64,
    pub special_device: u64,
}

impl DeviceIdentity {
    pub fn from_device(device: impl AsFd) -> io::Result<Self> {
        let stat = rustix::fs::fstat(device)?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode)
            != rustix::fs::FileType::CharacterDevice
            || rustix::fs::major(stat.st_rdev) != 226
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "renderer worker requires a DRM device descriptor",
            ));
        }
        Ok(Self {
            device: stat.st_dev,
            inode: stat.st_ino,
            special_device: stat.st_rdev,
        })
    }
}

struct Slot {
    device: DeviceIdentity,
    retired: Option<JoinHandle<()>>,
}

pub(super) struct WorkerRegistry {
    slots: Mutex<[Option<Slot>; WORKER_CAPACITY]>,
}

impl Default for WorkerRegistry {
    fn default() -> Self {
        Self {
            slots: Mutex::new(std::array::from_fn(|_| None)),
        }
    }
}

impl WorkerRegistry {
    pub fn shared() -> &'static Arc<Self> {
        static REGISTRY: OnceLock<Arc<WorkerRegistry>> = OnceLock::new();
        REGISTRY.get_or_init(|| Arc::new(Self::default()))
    }

    pub fn reserve(self: &Arc<Self>, device: DeviceIdentity) -> io::Result<WorkerReservation> {
        let mut slots = self.slots.lock().unwrap_or_else(|error| error.into_inner());
        for slot in slots.iter_mut() {
            if slot
                .as_ref()
                .and_then(|slot| slot.retired.as_ref())
                .is_some_and(JoinHandle::is_finished)
            {
                let retired = slot.take().and_then(|slot| slot.retired).unwrap();
                let _ = retired.join();
            }
        }
        if slots
            .iter()
            .flatten()
            .any(|slot| slot.device == device && slot.retired.is_some())
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "previous renderer worker has not finished",
            ));
        }
        let Some(index) = slots.iter().position(Option::is_none) else {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "renderer worker capacity exhausted",
            ));
        };
        slots[index] = Some(Slot {
            device,
            retired: None,
        });
        Ok(WorkerReservation {
            registry: Arc::clone(self),
            index,
            occupied: true,
        })
    }
}

/// A reserved slot survives core destruction until its thread has finished.
pub(super) struct WorkerReservation {
    registry: Arc<WorkerRegistry>,
    index: usize,
    occupied: bool,
}

impl WorkerReservation {
    pub fn start(
        self,
        spawn: impl FnOnce() -> io::Result<JoinHandle<()>>,
    ) -> io::Result<WorkerThread> {
        let thread = spawn()?;
        Ok(WorkerThread {
            thread: Some(thread),
            reservation: Some(self),
            joined: None,
        })
    }

    fn retire(mut self, thread: JoinHandle<()>) {
        let mut slots = self
            .registry
            .slots
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if thread.is_finished() {
            let _ = thread.join();
            slots[self.index] = None;
        } else {
            slots[self.index]
                .as_mut()
                .expect("worker owns its reservation")
                .retired = Some(thread);
        }
        self.occupied = false;
    }
}

impl Drop for WorkerReservation {
    fn drop(&mut self) {
        if self.occupied {
            self.registry
                .slots
                .lock()
                .unwrap_or_else(|error| error.into_inner())[self.index] = None;
        }
    }
}

#[derive(Default)]
pub(super) struct WorkerControl {
    shutdown: AtomicBool,
}

impl WorkerControl {
    pub fn shutdown(&self) {
        self.shutdown.store(true, Ordering::Release);
    }

    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }
}

/// One registration owns one claim; a delayed detach cannot name its replacement.
#[derive(Default)]
pub(super) struct OutputClaim {
    detached: AtomicBool,
}

impl OutputClaim {
    pub fn detach(&self) {
        self.detached.store(true, Ordering::Release);
    }

    pub fn is_detached(&self) -> bool {
        self.detached.load(Ordering::Acquire)
    }
}

/// Dropping ownership never waits for a driver call to return.
pub(super) struct WorkerThread {
    thread: Option<JoinHandle<()>>,
    reservation: Option<WorkerReservation>,
    joined: Option<bool>,
}

impl WorkerThread {
    /// Join only a thread already reported finished. Preserve a panic outcome
    /// across repeated polls; releasing the registration is not proof of success.
    pub fn poll_join(&mut self) -> io::Result<bool> {
        if self.joined.is_none() {
            if !self.thread.as_ref().is_some_and(JoinHandle::is_finished) {
                return Ok(false);
            }
            self.joined = Some(self.thread.take().expect("finished thread").join().is_ok());
            drop(self.reservation.take());
        }
        match self.joined {
            Some(true) => Ok(true),
            Some(false) => Err(io::Error::other("renderer worker panicked during shutdown")),
            None => unreachable!("join outcome was recorded"),
        }
    }
}

impl Drop for WorkerThread {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            self.reservation
                .take()
                .expect("worker has a reserved slot")
                .retire(thread);
        }
    }
}
