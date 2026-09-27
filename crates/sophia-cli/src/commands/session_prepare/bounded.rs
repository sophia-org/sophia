use super::Result;
use rustix::process::{Pid, WaitId, WaitIdOptions};
use std::{
    os::unix::process::CommandExt as _,
    process::{Child, Command},
    time::{Duration, Instant},
};

struct ProcessGroup(Child);

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        if let Some(group) = rustix::process::Pid::from_raw(self.0.id() as i32) {
            let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A preparation child never inherits custody of session processes. Signal its
/// group before reaping on every exit, including a successful leader. Trusted
/// checkers must not escape cleanup by changing their session or process group.
pub(super) fn check(command: &mut Command, label: &str) -> Result<()> {
    let child = ProcessGroup(command.process_group(0).spawn()?);
    let pid = Pid::from_raw(child.0.id() as i32).expect("spawned child has a process ID");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        // Keep the exited leader waitable until Drop has sent its final signal;
        // reaping here could let an unrelated process reuse the group number.
        if let Some(status) = rustix::process::waitid(
            WaitId::Pid(pid),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
        )? {
            return if status.exit_status() == Some(0) {
                Ok(())
            } else {
                Err(format!("{label} refused preparation: {status:?}").into())
            };
        }
        if Instant::now() >= deadline {
            return Err(format!("{label} exceeded ten seconds").into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
