use super::*;

impl NativeGbmRendererWorkerCore {
    /// Final shutdown after native custody drained, not output replacement.
    /// The atomic request survives a full command queue.
    pub fn request_shutdown(&self) {
        self.control.shutdown();
        let _ = self.command_sender.try_send(WorkerCommand::Shutdown);
    }

    /// Nonblocking inspection of this exact owned thread, not a device-wide
    /// thread count or a replacement worker. The caller bounds retry duration.
    pub fn poll_shutdown(&self) -> io::Result<bool> {
        if !self.control.is_shutdown() {
            return Err(io::Error::other("renderer shutdown was not requested"));
        }
        match self._thread.try_lock() {
            Ok(mut thread) => thread.poll_join(),
            Err(std::sync::TryLockError::WouldBlock) => Ok(false),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                Err(io::Error::other("renderer thread owner lock poisoned"))
            }
        }
    }
}

impl NativeGbmRendererWorker {
    pub(crate) fn request_shutdown(&self) {
        self.core.request_shutdown();
    }

    pub(crate) fn poll_shutdown(&self) -> io::Result<bool> {
        self.core.poll_shutdown()
    }
}
