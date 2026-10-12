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
    owner: mpsc::Sender<Box<dyn Send>>,
}
impl SpawnLease {
    pub(crate) fn channel() -> (Self, Receiver<Box<dyn Send>>) {
        let (owner, receiver) = mpsc::channel();
        (Self { owner }, receiver)
    }
    /// Transfer blocking child/resource destruction to the thread that already
    /// owns this spawn's lifetime. There is no additional cleanup worker.
    pub fn retire(self, resources: impl Send + 'static) {
        // The receiver remains alive until this sole sender is dropped. A
        // failure would mean the worker panicked; preserve normal drop cleanup.
        let _ = self.owner.send(Box::new(resources));
    }
}

/// One outstanding spawn per role. Cancellation retains the worker until its
/// result returns: replacement cannot accumulate abandoned launch workers.
pub(crate) struct ProtectedSpawn {
    cancelled: Arc<AtomicBool>,
    result: Receiver<SpawnResult>,
}

impl ProtectedSpawn {
    pub fn start(
        supervisor: ProcessSupervisor,
        channel: LaunchChannel,
        wake: sophia_wake::Notifier,
    ) -> Result<Self, String> {
        Self::start_using(supervisor, channel, wake, false)
    }

    pub(crate) fn start_host_namespace_client(
        supervisor: ProcessSupervisor,
        channel: LaunchChannel,
        wake: sophia_wake::Notifier,
    ) -> Result<Self, String> {
        Self::start_using(supervisor, channel, wake, true)
    }

    fn start_using(
        mut supervisor: ProcessSupervisor,
        channel: LaunchChannel,
        wake: sophia_wake::Notifier,
        host_namespace_client: bool,
    ) -> Result<Self, String> {
        let (sender, result) = mpsc::sync_channel(1);
        let (lease, lifetime) = SpawnLease::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        std::thread::Builder::new()
            .name("sophia-role-spawn".into())
            .spawn(move || {
                let result = if worker_cancelled.load(Ordering::Acquire) {
                    Err("protected launch cancelled before spawn".into())
                } else {
                    (if host_namespace_client {
                        supervisor.start_host_namespace_client(channel)
                    } else {
                        supervisor.start_with_launch_channel(channel)
                    })
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
                    lease,
                });
                wake.notify();
                if let Ok(resources) = lifetime.recv() {
                    drop(resources);
                }
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
