//! Keep the protected launcher and its peer discovery off the Session owner.
use sophia_runtime::{LaunchChannel, ProcessSupervisor};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, TryRecvError},
};

pub(crate) struct SpawnResult {
    pub supervisor: ProcessSupervisor,
    pub result: Result<(), String>,
    pub lease: SpawnLease,
}

/// Bubblewrap's parent-death signal follows the spawning thread. Keep that
/// thread alive until the returned supervisor's child has been reaped/dropped.
pub(crate) struct SpawnLease {
    _owner: mpsc::Sender<()>,
}

/// One outstanding spawn per role. Cancellation retains the worker until its
/// result returns: replacement cannot accumulate abandoned launch workers.
pub(crate) struct ProtectedSpawn {
    cancelled: Arc<AtomicBool>,
    result: Receiver<SpawnResult>,
}

impl ProtectedSpawn {
    pub fn start(
        mut supervisor: ProcessSupervisor,
        channel: LaunchChannel,
        wake: sophia_wake::Notifier,
    ) -> Result<Self, String> {
        let (sender, result) = mpsc::sync_channel(1);
        let (lease, lifetime) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        std::thread::Builder::new()
            .name("sophia-role-spawn".into())
            .spawn(move || {
                let result = if worker_cancelled.load(Ordering::Acquire) {
                    Err("protected launch cancelled before spawn".into())
                } else {
                    supervisor
                        .start_with_launch_channel(channel)
                        .map(drop)
                        .map_err(|e| e.to_string())
                };
                if worker_cancelled.load(Ordering::Acquire) {
                    let _ = supervisor.request_termination();
                }
                // A dropped receiver drops the returned supervisor and its child.
                let _ = sender.send(SpawnResult {
                    supervisor,
                    result,
                    lease: SpawnLease { _owner: lease },
                });
                wake.notify();
                let _ = lifetime.recv();
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { cancelled, result })
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn poll(&self) -> Result<Option<SpawnResult>, String> {
        match self.result.try_recv() {
            Ok(result) => Ok(Some(result)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err("protected launch worker stopped".into()),
        }
    }
}
impl Drop for ProtectedSpawn {
    fn drop(&mut self) {
        self.cancel();
    }
}
