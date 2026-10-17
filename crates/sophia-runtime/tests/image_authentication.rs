//! Side-channel authority joins current 9P custody, never legacy root access.
use sophia_9p::{
    ConnectionId, Errno, Fid, ReadOutcome, authentication::Attempt, export::AttachContext,
};
use sophia_protocol::*;
use sophia_runtime::authenticated_files::{AuthenticatedFiles, FileAuthentication};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

struct Owner {
    live: Arc<AtomicBool>,
    admitted: bool,
    context: Option<ClientAdmissionContext>,
}
impl FileAuthentication for Owner {
    fn claims(&self, _: &AttachContext<'_>) -> Result<(), Errno> {
        unreachable!()
    }
    fn begin(&mut self, _: Fid, _: Instant) -> Result<Attempt, Errno> {
        unreachable!()
    }
    fn read(&mut self, _: Attempt, _: u64, _: u32, _: Instant) -> Result<ReadOutcome, Errno> {
        unreachable!()
    }
    fn commit(&mut self, _: Attempt, _: Instant) -> Result<(), Errno> {
        unreachable!()
    }
    fn check(&mut self, connection: ConnectionId, _: Instant) -> Result<(), Errno> {
        if connection != ConnectionId(7) || !self.live.load(Ordering::SeqCst) {
            Err(Errno::ESTALE)
        } else {
            Ok(())
        }
    }
    fn release(&mut self, _: Attempt, _: Instant) {
        unreachable!()
    }
    fn reset(&mut self, _: Instant) -> Result<(), Errno> {
        unreachable!()
    }
    fn close(&mut self, _: Instant) {
        unreachable!()
    }
    fn poll(&mut self, _: Instant) -> Result<(), Errno> {
        unreachable!()
    }
    fn drained(&self) -> bool {
        false
    }
    fn admitted(&self) -> bool {
        self.admitted
    }
    fn admission_context(&self) -> Option<ClientAdmissionContext> {
        self.context
    }
    fn wait(&self, _: Instant) -> Duration {
        Duration::MAX
    }
}

#[test]
fn side_channel_requires_live_authenticated_custody_and_context() {
    let now = Instant::now();
    for mut files in [
        AuthenticatedFiles::legacy(()),
        AuthenticatedFiles::pending(()),
    ] {
        assert_eq!(
            files.check_side_channel(ConnectionId(7), now),
            Err(Errno::EACCES)
        );
    }
    let context = ClientAdmissionContext::new(
        ClientAdmissionId::from_raw(1),
        NamespaceContext::new(
            NamespaceId::from_raw(2),
            NamespaceProfile::Confined,
            NamespaceCapabilities::NONE,
        )
        .unwrap(),
        ClientAuthProvenance::new(
            ClientAuthenticationMethod::FactotumLaunch {
                principal: 3,
                launch_generation: 4,
            },
            5,
        )
        .unwrap(),
    )
    .unwrap();
    let live = Arc::new(AtomicBool::new(true));
    for (admitted, identity) in [(false, Some(context)), (true, None), (true, Some(context))] {
        let mut files = AuthenticatedFiles::pending(());
        files
            .install(Box::new(Owner {
                live: live.clone(),
                admitted,
                context: identity,
            }))
            .unwrap();
        let expected = if admitted && identity.is_some() {
            Ok(context)
        } else {
            Err(Errno::EACCES)
        };
        assert_eq!(files.check_side_channel(ConnectionId(7), now), expected);
        assert_eq!(
            files.check_side_channel(ConnectionId(8), now),
            Err(Errno::ESTALE)
        );
        live.store(false, Ordering::SeqCst);
        // Remembered identity remains available but grants no operation.
        assert_eq!(files.admission_context(), identity);
        assert_eq!(
            files.check_side_channel(ConnectionId(7), now),
            Err(Errno::ESTALE)
        );
        live.store(true, Ordering::SeqCst);
    }
    // Real side socket and SO_PEERPIDFD, with only the afid owner supplied.
    use rustix::net::*;
    use sophia_runtime::image_channel::ImageChannelListener;
    use std::os::unix::fs::PermissionsExt;
    let directory = std::env::temp_dir().join(format!("sophia-image-auth-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    {
        let listener = ImageChannelListener::bind(&directory).unwrap();
        let connect_peer = || {
            let fd = socket_with(
                AddressFamily::UNIX,
                SocketType::SEQPACKET,
                SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
                None,
            )
            .unwrap();
            connect(
                &fd,
                &SocketAddrUnix::new(directory.join("image.sock")).unwrap(),
            )
            .unwrap();
            fd
        };
        let identity = sophia_runtime::PolicyPeerIdentity {
            uid: rustix::process::geteuid().as_raw(),
            pid: std::process::id(),
        };
        let mut files = AuthenticatedFiles::pending(());
        files
            .install(Box::new(Owner {
                live: live.clone(),
                admitted: true,
                context: Some(context),
            }))
            .unwrap();
        let _first = connect_peer();
        let wrong = sophia_runtime::PolicyPeerIdentity { pid: 0, ..identity };
        assert!(
            listener
                .accept_authenticated(wrong, ConnectionId(7), &mut files, now)
                .is_err()
        );
        let _second = connect_peer();
        let mut legacy = AuthenticatedFiles::legacy(());
        assert!(
            listener
                .accept_authenticated(identity, ConnectionId(7), &mut legacy, now)
                .is_err()
        );
        let _third = connect_peer();
        let peer = listener
            .accept_authenticated(identity, ConnectionId(7), &mut files, now)
            .unwrap()
            .unwrap();
        assert!(peer.check(&mut files, now).is_ok());
        live.store(false, Ordering::SeqCst);
        assert!(peer.check(&mut files, now).is_err());
        live.store(true, Ordering::SeqCst);
        use sophia_runtime::image_channel::{ImageChannelRole, ImageHelloPending};
        for role in [ImageChannelRole::Shell, ImageChannelRole::Lock] {
            for bad in [false, true] {
                let sender = connect_peer();
                let peer = listener
                    .accept_authenticated(identity, ConnectionId(7), &mut files, now)
                    .unwrap()
                    .unwrap();
                let mut pending =
                    ImageHelloPending::new(peer, role, 9, 11, now + Duration::from_millis(250))
                        .unwrap();
                assert!(!pending.poll(&mut files, now).unwrap());
                let generation = if bad { 12 } else { 11 };
                let hello = match role {
                    ImageChannelRole::Shell => shell_files::ShellImagePacket::Hello {
                        connection_epoch: 9,
                        channel_generation: generation,
                    }
                    .encode()
                    .unwrap(),
                    ImageChannelRole::Lock => lock_files::LockImagePacket::Hello {
                        connection_epoch: 9,
                        channel_generation: generation,
                    }
                    .encode()
                    .unwrap(),
                };
                assert_eq!(
                    send(&sender, &hello, SendFlags::NOSIGNAL).unwrap(),
                    hello.len()
                );
                if bad {
                    assert!(pending.poll(&mut files, now).is_err());
                } else {
                    assert!(pending.poll(&mut files, now).unwrap());
                    // Journal backpressure cannot renew the original deadline.
                    assert!(
                        pending
                            .poll(&mut files, now + Duration::from_millis(249))
                            .unwrap()
                    );
                    assert!(
                        pending
                            .poll(&mut files, now + Duration::from_millis(250))
                            .is_err()
                    );
                }
                assert!(pending.poll(&mut files, now).is_err());
                assert!(pending.into_peer().is_err());
            }
        }
    }
    std::fs::remove_dir(directory).unwrap();
}
