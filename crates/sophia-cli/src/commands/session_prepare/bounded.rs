use super::Result;
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

/// A preparation child never inherits custody of session processes. Reap its
/// whole group on every exit, including a successful parent with stray children.
pub(super) fn check(command: &mut Command, label: &str) -> Result<()> {
    let mut child = ProcessGroup(command.process_group(0).spawn()?);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.0.try_wait()? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("{label} refused preparation: {status}").into())
            };
        }
        if Instant::now() >= deadline {
            return Err(format!("{label} exceeded ten seconds").into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
