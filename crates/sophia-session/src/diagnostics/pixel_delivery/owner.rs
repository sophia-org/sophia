use super::*;
use crate::diagnostics::storage::Directory;
use crate::launch_origin::{ProcessIdentity, read_process};
use rustix::fs::OFlags;
use sha2::{Digest, Sha256};
use sophia_protocol::NamespaceId;
use sophia_runtime::ProcessSupervisor;
use std::collections::BTreeMap;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Served {
    request: u64,
    info: OutputCaptureFrameInfo,
    sha256: [u8; 32],
}
#[derive(Clone)]
struct Attempt {
    namespace: NamespaceId,
    connector: String,
    active: bool,
    closed: bool,
    served: Option<Served>,
    duplicate: bool,
}

/// A new private Session write path. No directory or file descriptor is granted
/// to the observer. Three attempts are burned, including failed attempts.
pub(crate) struct CaptureStore {
    directory: Directory,
    executable: PathBuf,
    identity: (u64, u64),
    attempts: Mutex<BTreeMap<u64, Attempt>>,
}

impl CaptureStore {
    pub(crate) fn create(path: &Path) -> io::Result<Arc<Self>> {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(invalid(
                "capture directory must be absolute without parent traversal",
            ));
        }
        let executable = std::env::current_exe()?;
        let metadata = std::fs::metadata("/proc/self/exe")?;
        std::fs::DirBuilder::new().mode(0o700).create(path)?;
        Ok(Arc::new(Self {
            directory: Directory::open(path, false)?,
            executable,
            identity: (metadata.dev(), metadata.ino()),
            attempts: Mutex::new(BTreeMap::new()),
        }))
    }

    pub(crate) fn matches_executable(&self, path: &Path) -> bool {
        path == self.executable
            && std::fs::metadata(path).is_ok_and(|m| (m.dev(), m.ino()) == self.identity)
    }

    pub(crate) fn prepare(
        self: &Arc<Self>,
        generation: u64,
        transaction: u64,
        namespace: NamespaceId,
        output: OutputId,
        connector: &str,
        wake: sophia_wake::Notifier,
    ) -> io::Result<(CaptureLaunch, OwnedFd, OwnedFd)> {
        let mut attempts = self
            .attempts
            .lock()
            .map_err(|_| invalid("capture lock poisoned"))?;
        if generation == 0
            || transaction == 0
            || !namespace.is_valid()
            || !output.is_valid()
            || connector.is_empty()
            || connector.len() > 256
            || !connector
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_:./".contains(&b))
            || attempts.len() == 3
            || attempts.contains_key(&generation)
            || attempts
                .values()
                .any(|a| a.namespace == namespace && !a.closed)
        {
            return Err(invalid("capture attempt unavailable"));
        }
        attempts.insert(
            generation,
            Attempt {
                namespace,
                connector: connector.into(),
                active: false,
                closed: false,
                served: None,
                duplicate: false,
            },
        );
        drop(attempts);
        let result = (|| {
            let (reader, writer) = UnixStream::pair()?;
            let (error_reader, error_writer) = UnixStream::pair()?;
            let deadline = Instant::now() + TIMEOUT;
            let (finish, finished) = mpsc::sync_channel(1);
            #[cfg(test)]
            let (done, done_rx) = mpsc::sync_channel(1);
            let failed = Arc::new(AtomicBool::new(false));
            let (stderr_tx, stderr_rx) = mpsc::sync_channel(1);
            let error_failure = failed.clone();
            let error_wake = wake.clone();
            std::thread::Builder::new()
                .name("capture-stderr".into())
                .spawn(move || {
                    let result = stream::read_tail(error_reader, 16 * 1024, deadline);
                    if result.as_ref().map_or(true, |bytes| !bytes.is_empty()) {
                        error_failure.store(true, Ordering::Release);
                        error_wake.notify();
                    }
                    let _ = stderr_tx.send(result);
                })?;
            let store = self.clone();
            let failure = failed.clone();
            std::thread::Builder::new().name("capture-pixels".into()).spawn(move || {
            let frame = stream::read_frame(reader, output, deadline);
            if frame.is_err() { failure.store(true, Ordering::Release); wake.notify(); }
            let errors = stderr_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| invalid("capture stderr deadline")).and_then(|r| r);
            let errors = errors.and_then(|errors| {
                store.save_stderr(generation, &errors)?;
                Ok(errors)
            });
            let result = (|| {
                let frame = frame?;
                let left = deadline.saturating_duration_since(Instant::now());
                let binding: Binding = finished.recv_timeout(left).map_err(|_| invalid("capture not cleanly reaped"))?;
                let errors = errors?;
                if !errors.is_empty() { return Err(invalid("capture child wrote stderr")); }
                store.accept(generation, transaction, &binding, frame)
            })();
            if let Err(error) = result {
                failure.store(true, Ordering::Release);
                wake.notify();
                // A failed publication may leave a PAM; only a complete receipt qualifies it.
                let message = format!("status=refused\nlaunch_generation={generation}\ntransaction={transaction}\nreason={:?}\n", error.to_string());
                let _ = store.write_new(&format!("capture-{generation}.refused"), message.as_bytes());
            }
            #[cfg(test)]
            let _ = done.send(());
        })?;
            Ok((
                CaptureLaunch {
                    store: self.clone(),
                    generation,
                    deadline,
                    failed,
                    finish: Some(finish),
                    binding: None,
                    #[cfg(test)]
                    done: done_rx,
                },
                writer.into(),
                error_writer.into(),
            ))
        })();
        if result.is_err()
            && let Ok(mut attempts) = self.attempts.lock()
            && let Some(attempt) = attempts.get_mut(&generation)
        {
            attempt.closed = true;
        }
        result
    }

    /// Called only at Session's successful immutable capture publication, before
    /// the observer can read it. Child-supplied generations cannot replace this.
    pub(crate) fn served(
        &self,
        namespace: NamespaceId,
        request: u64,
        info: OutputCaptureFrameInfo,
        pixels: &[u8],
    ) {
        if let Ok(mut attempts) = self.attempts.lock() {
            for attempt in attempts
                .values_mut()
                .filter(|a| a.namespace == namespace && a.active)
            {
                if attempt.served.is_some() {
                    attempt.duplicate = true;
                } else {
                    attempt.served = Some(Served {
                        request,
                        info,
                        sha256: Sha256::digest(pixels).into(),
                    });
                }
            }
        }
    }

    fn write_new(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let mut file = self
            .directory
            .file(name, OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL)?;
        file.write_all(bytes)?;
        file.sync_all()
    }
    fn save_stderr(&self, generation: u64, errors: &[u8]) -> io::Result<()> {
        self.write_new(&format!("capture-{generation}.stderr"), errors)
    }
    fn accept(
        &self,
        generation: u64,
        transaction: u64,
        binding: &Binding,
        frame: Frame,
    ) -> io::Result<()> {
        let attempt = self
            .attempts
            .lock()
            .map_err(|_| invalid("capture lock poisoned"))?
            .get(&generation)
            .cloned()
            .ok_or_else(|| invalid("capture attempt lost"))?;
        let actual = Served {
            request: frame.request,
            info: frame.info,
            sha256: Sha256::digest(&frame.pixels).into(),
        };
        if attempt.duplicate || attempt.served.as_ref() != Some(&actual) {
            return Err(invalid("capture differs from Session publication"));
        }
        let mut pam = format!(
            "P7\nWIDTH {}\nHEIGHT {}\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n",
            frame.info.width, frame.info.height
        )
        .into_bytes();
        pam.extend_from_slice(&frame.pixels);
        self.directory
            .publish_new(&format!("capture-{generation}.pam"), &pam)?;
        let digest = format!("{:x}", Sha256::digest(&pam));
        let info = frame.info;
        let receipt = format!(
            "{{\"schema\":1,\"status\":\"delivered\",\"binding\":\"launch_custody_not_per_message\",\"launch_generation\":{generation},\"transaction\":{transaction},\"connector\":\"{}\",\"namespace\":{},\"peer_pid\":{},\"peer_starttime\":{},\"exe_dev\":{},\"exe_ino\":{},\"request\":{},\"output\":{},\"output_generation\":{},\"presentation_generation\":{},\"width\":{},\"height\":{},\"pam_sha256\":\"{digest}\"}}\n",
            attempt.connector,
            attempt.namespace.raw(),
            binding.identity.pid,
            binding.identity.start_time,
            self.identity.0,
            self.identity.1,
            frame.request,
            info.output.raw(),
            info.output_generation,
            info.presentation_generation,
            info.width,
            info.height
        );
        self.directory
            .publish_new(&format!("capture-{generation}.json"), receipt.as_bytes())
    }
}

#[cfg(test)]
#[path = "../../../tests/support/pixel_delivery_owner.rs"]
mod tests;

struct Binding {
    identity: ProcessIdentity,
    _pidfd: OwnedFd,
}
pub(crate) struct CaptureLaunch {
    #[cfg(test)]
    done: mpsc::Receiver<()>,
    store: Arc<CaptureStore>,
    generation: u64,
    deadline: Instant,
    failed: Arc<AtomicBool>,
    finish: Option<mpsc::SyncSender<Binding>>,
    binding: Option<Binding>,
}
impl CaptureLaunch {
    /// After authenticated startup, while the unreaped wrapper still owns its
    /// sole inner child. The socketpair's peer credentials are intentionally unused.
    pub(crate) fn activate(&mut self, supervisor: &ProcessSupervisor) -> io::Result<()> {
        let pid = supervisor
            .peer_id()
            .ok_or_else(|| invalid("capture peer absent"))?;
        let wrapper = supervisor
            .child_id()
            .ok_or_else(|| invalid("capture wrapper absent"))?;
        let pidfd = rustix::process::pidfd_open(
            rustix::process::Pid::from_raw(pid as i32)
                .ok_or_else(|| invalid("invalid capture pid"))?,
            rustix::process::PidfdFlags::empty(),
        )?;
        let identity = read_process(pid)
            .ok_or_else(|| invalid("capture identity absent"))?
            .identity;
        let children = std::fs::read_to_string(format!("/proc/{wrapper}/task/{wrapper}/children"))?;
        let metadata = std::fs::metadata(format!("/proc/{pid}/exe"))?;
        let mut fds = [rustix::event::PollFd::new(
            &pidfd,
            rustix::event::PollFlags::IN,
        )];
        let timeout = rustix::event::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        if children.split_whitespace().collect::<Vec<_>>() != [pid.to_string()]
            || (metadata.dev(), metadata.ino()) != self.store.identity
            || read_process(pid).map(|p| p.identity) != Some(identity)
            || rustix::event::poll(&mut fds, Some(&timeout))? != 0
        {
            return Err(invalid(
                "capture authenticated executable identity mismatch",
            ));
        }
        self.store
            .attempts
            .lock()
            .map_err(|_| invalid("capture lock poisoned"))?
            .get_mut(&self.generation)
            .ok_or_else(|| invalid("capture attempt lost"))?
            .active = true;
        self.binding = Some(Binding {
            identity,
            _pidfd: pidfd,
        });
        Ok(())
    }
    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire) || Instant::now() >= self.deadline
    }
    pub(crate) fn reaped(&mut self, status: std::process::ExitStatus) {
        if status.success()
            && !self.failed()
            && let Some(binding) = self.binding.take()
            && let Some(finish) = self.finish.take()
        {
            let _ = finish.send(binding);
        }
    }
}
impl Drop for CaptureLaunch {
    fn drop(&mut self) {
        if let Ok(mut attempts) = self.store.attempts.lock()
            && let Some(a) = attempts.get_mut(&self.generation)
        {
            a.active = false;
            a.closed = true;
        }
        self.finish.take();
    }
}
