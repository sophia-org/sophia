//! Session's private in-process requester transport. The bounded owner queue
//! creates socketpairs; there is no listener or peer-selected admission record.
use crate::files::*;
use sophia_9p::{Errno, Limits, unix::Server};
use sophia_protocol::{NamespaceId, portal_files::PortalFileLimits};
use std::os::unix::net::UnixStream;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, SyncSender},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub struct PortalRequesterConnection {
    pub stream: UnixStream,
    pub epoch: u64,
}
struct Connect {
    source: NamespaceId,
    target: NamespaceId,
    revision: u64,
    deadline: Instant,
    reply: SyncSender<Result<PortalRequesterConnection, Errno>>,
}
pub struct SessionPortalFileService {
    connect: SyncSender<Connect>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}
impl SessionPortalFileService {
    /// `execute` is the recipient handoff, not policy. It must perform its
    /// final security/source check and must not wait for transport progress.
    pub fn spawn<A, F>(
        authority: A,
        limits: PortalFileLimits,
        mut execute: F,
    ) -> std::io::Result<Self>
    where
        A: PortalFileAuthority + Send + 'static,
        F: FnMut(PortalFileExecution) -> bool + Send + 'static,
    {
        let export = PortalFileExport::new(authority, limits)
            .map_err(|e| std::io::Error::from_raw_os_error(e.0 as i32))?;
        let mut server = Server::new(export, Limits::default())?;
        let (connect, requests) = mpsc::sync_channel::<Connect>(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::Builder::new()
            .name("portal-files".into())
            .spawn(move || {
                let mut active = None;
                let mut epoch = 0_u64;
                while !stopping.load(Ordering::Acquire) {
                    server.export_mut().expire(Instant::now());
                    if server.turn(Some(Duration::from_millis(5))).is_err() {
                        break;
                    }
                    if server.connection_count() == 0
                        && let Some(connection) = active.take()
                    {
                        server.export_mut().disconnect(connection);
                    }
                    // The requester may close and enqueue its successor after
                    // this turn's socket read. Keep that successor in the
                    // bounded queue until a later turn observes EOF; rejecting
                    // it here makes serial transfers depend on scheduling.
                    if active.is_none()
                        && let Ok(request) = requests.try_recv()
                    {
                        let result = (|| {
                            if Instant::now() >= request.deadline {
                                return Err(Errno::EIO);
                            }
                            if !server.export().authority().live()
                                || server.export().authority().revision() != request.revision
                            {
                                return Err(Errno::ESTALE);
                            }
                            epoch = epoch.checked_add(1).ok_or(Errno::ENOSPC)?;
                            let (owner, stream) = UnixStream::pair().map_err(|_| Errno::EIO)?;
                            let connection = server.adopt(owner).map_err(|_| Errno::EIO)?;
                            active = Some(connection);
                            server.export_mut().admit(PortalRequesterAdmission {
                                connection,
                                epoch,
                                source: request.source,
                                target: request.target,
                                custody: PortalRequesterCustody::SessionXAuthority,
                            })?;
                            // The fence may change between the precheck and
                            // admission. Never re-label queued old work as a
                            // request from the newly unlocked interval.
                            if server.export().authority().revision() != request.revision {
                                server.export_mut().revoke();
                                return Err(Errno::ESTALE);
                            }
                            Ok(PortalRequesterConnection { stream, epoch })
                        })();
                        let _ = request.reply.try_send(result);
                    }
                    if let Some(work) = server.export_mut().take_execution() {
                        let transfer = work.grant.transfer;
                        let succeeded = execute(work);
                        if server
                            .export_mut()
                            .settle_execution(transfer, succeeded)
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                server.export_mut().revoke();
            })?;
        Ok(Self {
            connect,
            stop,
            thread: Some(thread),
        })
    }
    /// Called only by the off-owner X clipboard worker. Queue pressure refuses
    /// before creating a connection; the two-second wait never runs on Session.
    pub fn connect(
        &self,
        source: NamespaceId,
        target: NamespaceId,
        revision: u64,
    ) -> Result<PortalRequesterConnection, Errno> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.connect
            .try_send(Connect {
                source,
                target,
                revision,
                deadline: Instant::now() + Duration::from_secs(2),
                reply,
            })
            .map_err(|_| Errno::EAGAIN)?;
        answer
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| Errno::EIO)?
    }
}
impl Drop for SessionPortalFileService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
