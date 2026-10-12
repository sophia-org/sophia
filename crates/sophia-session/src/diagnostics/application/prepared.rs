//! A pending diagnostic record owns its pipe until the spawn owner takes it.
use super::*;
use std::os::fd::OwnedFd;

pub struct PreparedLaunch {
    pub(super) shared: Arc<Shared>,
    pub(super) id: u64,
    pub(super) context: LaunchContext,
    pub(super) writer: Option<OwnedFd>,
    pub(super) settled: bool,
}
impl PreparedLaunch {
    /// Move, never duplicate, the writer into the actual process supervisor.
    pub fn take_stderr(&mut self) -> Option<OwnedFd> {
        self.writer.take()
    }

    pub fn spawned(&mut self, pid: u32) -> LaunchTicket {
        if !self.settled {
            record_spawn(&self.shared, self.id, self.context, Ok(pid));
            self.settled = true;
            self.writer.take();
        }
        LaunchTicket(self.id)
    }

    pub fn failed(&mut self, error: &io::Error) {
        if !self.settled {
            record_spawn(&self.shared, self.id, self.context, Err(error));
            self.settled = true;
            self.writer.take();
        }
    }
}
impl Drop for PreparedLaunch {
    fn drop(&mut self) {
        // A cancelled preparation cannot retain a pending record forever.
        self.failed(&io::Error::other("launch preparation abandoned"));
    }
}
