//! An owner's wait observation may skip only the idle listener's accept, and
//! only while the image channel is Ready. Every other service probes; peer
//! authority is checked on every service either way.
use super::*;
use rustix::net::{RecvFlags, SendFlags, recv, send};
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

const WAIT: Duration = Duration::from_secs(5);

/// The supplied afid authority, whose side-channel verdict a test can revoke
/// without touching the 9P wire the peer is parked on.
struct Factory(Arc<AtomicBool>);
struct Owner {
    connection: sophia_9p::ConnectionId,
    admitted: bool,
    live: Arc<AtomicBool>,
}
impl sophia_runtime::authenticated_files::FileAuthenticationFactory for Factory {
    fn create(
        &mut self,
        connection: sophia_9p::ConnectionId,
        _: Instant,
        _: sophia_9p::unix::Wake,
    ) -> Result<Box<dyn sophia_runtime::authenticated_files::FileAuthentication>, sophia_9p::Errno>
    {
        Ok(Box::new(Owner {
            connection,
            admitted: false,
            live: self.0.clone(),
        }))
    }
}
impl sophia_runtime::authenticated_files::FileAuthentication for Owner {
    fn claims(&self, context: &sophia_9p::AttachContext<'_>) -> Result<(), sophia_9p::Errno> {
        if context.connection == self.connection {
            Ok(())
        } else {
            Err(sophia_9p::Errno::EACCES)
        }
    }
    fn begin(
        &mut self,
        afid: sophia_9p::Fid,
        _: Instant,
    ) -> Result<sophia_9p::authentication::Attempt, sophia_9p::Errno> {
        Ok(sophia_9p::authentication::Attempt {
            server: 1,
            connection: self.connection,
            session: 1,
            audience: 1,
            wire_generation: 1,
            attempt_generation: 1,
            afid: afid.0,
        })
    }
    fn read(
        &mut self,
        _: sophia_9p::authentication::Attempt,
        _: u64,
        _: u32,
        _: Instant,
    ) -> Result<sophia_9p::ReadOutcome, sophia_9p::Errno> {
        Err(sophia_9p::Errno::EACCES)
    }
    fn commit(
        &mut self,
        attempt: sophia_9p::authentication::Attempt,
        _: Instant,
    ) -> Result<(), sophia_9p::Errno> {
        if attempt.connection != self.connection {
            return Err(sophia_9p::Errno::EACCES);
        }
        self.admitted = true;
        Ok(())
    }
    fn check(
        &mut self,
        connection: sophia_9p::ConnectionId,
        _: Instant,
    ) -> Result<(), sophia_9p::Errno> {
        if self.admitted && connection == self.connection && self.live.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(sophia_9p::Errno::EACCES)
        }
    }
    fn release(&mut self, _: sophia_9p::authentication::Attempt, _: Instant) {}
    fn reset(&mut self, _: Instant) -> Result<(), sophia_9p::Errno> {
        self.admitted = false;
        Ok(())
    }
    fn close(&mut self, _: Instant) {
        self.admitted = false;
    }
    fn poll(&mut self, _: Instant) -> Result<(), sophia_9p::Errno> {
        Ok(())
    }
    fn drained(&self) -> bool {
        !self.admitted
    }
    fn admitted(&self) -> bool {
        self.admitted
    }
    fn wait(&self, _: Instant) -> Duration {
        Duration::MAX
    }
    fn admission_context(&self) -> Option<ClientAdmissionContext> {
        self.admitted.then(|| {
            ClientAdmissionContext::new(
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
            .unwrap()
        })
    }
}

/// A connector on the image listener. Until the owner accepts it, it sits in
/// the backlog and its peek would block; an accepted-and-refused duplicate
/// sees end of stream.
pub(super) fn connector(image_path: &std::path::Path) -> OwnedFd {
    let socket = rustix::net::socket_with(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::SEQPACKET,
        rustix::net::SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    rustix::net::connect(
        &socket,
        &rustix::net::SocketAddrUnix::new(image_path).unwrap(),
    )
    .unwrap();
    socket
}

enum Join {
    Pending,
    Refused,
}

fn join(socket: &OwnedFd) -> Join {
    let mut byte = [0u8; 1];
    match recv(socket, &mut byte, RecvFlags::PEEK | RecvFlags::DONTWAIT) {
        Err(rustix::io::Errno::AGAIN) => Join::Pending,
        Ok((0, _)) => Join::Refused,
        other => panic!("unexpected connector state: {other:?}"),
    }
}

/// How a wait result is shaped before it is observed.
enum Slice<'a> {
    /// The exact slice `poll_fds` borrowed, as an owner's wait sees it.
    Exact,
    /// The wire descriptors alone: no listener was registered.
    WireOnly,
    /// The wire descriptors, then some other idle descriptor in the
    /// listener's place.
    WrongListener(&'a OwnedFd),
}

/// Serve until nothing is pending, then take one real (empty) wait result.
/// The subscription is taken before the poll and the poll sees the exact
/// slice `poll_fds` borrowed, as an owner's wait does; `slice` then reshapes
/// what is observed.
fn idle_observation(
    transport: &mut ShellComponentTransport,
    epochs: &mut ContentEpochRegistry,
    slice: Slice<'_>,
) -> ShellReadinessObservation {
    let deadline = Instant::now() + WAIT;
    loop {
        assert!(Instant::now() < deadline, "transport never went idle");
        transport.service_owner_turn(epochs).unwrap();
        if transport.output_pending() {
            continue;
        }
        let subscription = transport.readiness_subscription().unwrap();
        let mut fds = transport.poll_fds();
        let ready =
            rustix::event::poll(&mut fds, Some(&rustix::event::Timespec::default())).unwrap();
        if ready != 0 {
            continue;
        }
        match slice {
            Slice::Exact => {}
            Slice::WireOnly => fds.truncate(2),
            Slice::WrongListener(other) => {
                fds.truncate(2);
                fds.push(rustix::event::PollFd::new(
                    other,
                    rustix::event::PollFlags::IN,
                ));
            }
        }
        return subscription
            .observe(&fds)
            .expect("wire descriptors unchanged");
    }
}

#[test]
fn idle_listener_observation_skips_only_the_ready_accept() {
    let directory =
        std::env::temp_dir().join(format!("shell-image-readiness-{}", std::process::id()));
    let mut transport = ShellComponentTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    let live = Arc::new(AtomicBool::new(true));
    transport
        .authorize_authenticated_peer(
            &ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: std::process::id(),
                peer_pid: std::process::id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            },
            Box::new(Factory(live.clone())),
        )
        .unwrap();
    let (d, l, f, b) = config();
    transport.configure_image_channel(d, l, f, b).unwrap();
    let mut epochs = ContentEpochRegistry::new(64 << 20).unwrap();
    transport
        .reserve_content(
            &mut epochs,
            ContentLimits::prototype(ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            }),
        )
        .unwrap();
    transport
        .begin_file_negotiation(
            &epochs,
            1,
            WAIT,
            ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let path = transport.socket_path().to_owned();
    let image_path = directory.join("image.sock");
    let (signal, arrived) = mpsc::channel();
    let (go, proceed) = mpsc::channel();
    // The 9P peer only negotiates, reads discovery and collects ChannelReady.
    // The image socket itself belongs to the test thread, so every accept
    // and every refusal is observed from the owner's side of the turn.
    let peer = std::thread::spawn(move || {
        let mut peer = raw::Peer::connect(&path);
        setup(&mut peer);
        let negotiate = encode_shell_file_negotiate(
            ShellFileHeader {
                kind: ShellFileKind::Negotiate,
                connection_epoch: 1,
                submission_id: 1,
                sequence: 0,
            },
            ShellV1ClientHello {
                minimum_revision: 10,
                maximum_revision: 10,
                required_capabilities: SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER
                    | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
                    | 1 << 15,
            },
        )
        .unwrap();
        peer.submit_acknowledged(&negotiate, 1);
        let event = peer.next_event();
        assert_eq!(
            decode_shell_file_negotiated(&event)
                .unwrap()
                .welcome
                .selected_revision,
            10
        );
        peer.ack(&event);
        peer.open(6, b"image-channel", 0);
        let discovery =
            ShellImageDiscovery::decode(std::str::from_utf8(&peer.read(6, 0)).unwrap()).unwrap();
        assert_eq!(discovery.channel_generation, 2);
        signal.send(()).unwrap();
        proceed.recv_timeout(WAIT).unwrap();
        let ready = peer.next_event();
        let record =
            decode_shell_file_record_at_revision(&ready, ShellFileClass::Event, 10).unwrap();
        assert_eq!(record.header.kind, ShellFileKind::ChannelReady);
        ack(&mut peer, &ready);
        signal.send(()).unwrap();
        // Parked with no request outstanding until the owner is done.
        proceed.recv_timeout(WAIT).unwrap();
    });
    let started = Instant::now();
    while transport
        .poll_negotiation(&mut epochs, 65536)
        .unwrap()
        .is_none()
    {
        assert!(started.elapsed() < WAIT);
        std::thread::yield_now();
    }
    while arrived.try_recv().is_err() {
        transport.poll_io(&mut epochs).unwrap();
        assert!(started.elapsed() < WAIT);
        std::thread::yield_now();
    }

    // Waiting: an idle observation carries no listener token, so the Hello
    // connector arriving after the wait is accepted on that very visit. The
    // wait set grows from the listener alone to listener plus channel.
    let observation = idle_observation(&mut transport, &mut epochs, Slice::Exact);
    assert_eq!(transport.poll_fds().len(), 3);
    let image = connector(&image_path);
    let hello = ShellImagePacket::Hello {
        connection_epoch: 1,
        channel_generation: 2,
    }
    .encode()
    .unwrap();
    send(&image, &hello, SendFlags::NOSIGNAL).unwrap();
    transport
        .service_owner_observed(&mut epochs, Some(observation))
        .unwrap();
    assert_eq!(
        transport.poll_fds().len(),
        4,
        "a Waiting listener must accept regardless of the observation"
    );
    go.send(()).unwrap();
    while arrived.try_recv().is_err() {
        transport.service_owner_turn(&mut epochs).unwrap();
        assert!(started.elapsed() < WAIT);
        std::thread::yield_now();
    }

    // Ready: a duplicate join that arrives after the wait is left in the
    // backlog by the observed visit, which is exactly one service. The next
    // visit without an observation accepts and refuses it.
    let observation = idle_observation(&mut transport, &mut epochs, Slice::Exact);
    let duplicate = connector(&image_path);
    let before = transport.wire_turn_count();
    transport
        .service_owner_observed(&mut epochs, Some(observation))
        .unwrap();
    assert_eq!(transport.wire_turn_count(), before + 1);
    assert!(matches!(join(&duplicate), Join::Pending));
    assert!(
        matches!(join(&image), Join::Pending),
        "the channel stays open"
    );
    transport.service_owner_observed(&mut epochs, None).unwrap();
    assert!(matches!(join(&duplicate), Join::Refused));
    assert!(matches!(join(&image), Join::Pending));

    // The observation is consumed by the visit that received it: a second
    // duplicate after another observed visit is refused by the following
    // ordinary visit without any new observation.
    let observation = idle_observation(&mut transport, &mut epochs, Slice::Exact);
    let duplicate = connector(&image_path);
    transport
        .service_owner_observed(&mut epochs, Some(observation))
        .unwrap();
    assert!(matches!(join(&duplicate), Join::Pending));
    transport.service_owner_turn(&mut epochs).unwrap();
    assert!(matches!(join(&duplicate), Join::Refused));

    // Stale: a turn after the wait invalidates the observation, so the
    // visit that receives it probes and refuses the duplicate at once.
    let observation = idle_observation(&mut transport, &mut epochs, Slice::Exact);
    transport.service_owner_turn(&mut epochs).unwrap();
    let duplicate = connector(&image_path);
    transport
        .service_owner_observed(&mut epochs, Some(observation))
        .unwrap();
    assert!(matches!(join(&duplicate), Join::Refused));
    assert!(matches!(join(&image), Join::Pending));

    // Missing listener: an observation formed from the wire descriptors
    // alone carries no listener token, so the visit probes and refuses the
    // late duplicate at once.
    let observation = idle_observation(&mut transport, &mut epochs, Slice::WireOnly);
    let duplicate = connector(&image_path);
    transport
        .service_owner_observed(&mut epochs, Some(observation))
        .unwrap();
    assert!(matches!(join(&duplicate), Join::Refused));
    assert!(matches!(join(&image), Join::Pending));

    // Wrong listener: an idle descriptor that is not the registered
    // listener never stands in for it.
    let other = rustix::net::socket_with(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::SEQPACKET,
        rustix::net::SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    let observation = idle_observation(&mut transport, &mut epochs, Slice::WrongListener(&other));
    let duplicate = connector(&image_path);
    transport
        .service_owner_observed(&mut epochs, Some(observation))
        .unwrap();
    assert!(matches!(join(&duplicate), Join::Refused));
    assert!(matches!(join(&image), Join::Pending));

    // Revocation: the idle observation skips nothing but the accept. The
    // peer check on the same visit closes the channel.
    let observation = idle_observation(&mut transport, &mut epochs, Slice::Exact);
    live.store(false, Ordering::SeqCst);
    transport
        .service_owner_observed(&mut epochs, Some(observation))
        .unwrap();
    assert!(
        matches!(join(&image), Join::Refused),
        "a revoked peer must lose its channel on the observed visit"
    );
    assert_eq!(transport.poll_fds().len(), 3);

    go.send(()).unwrap();
    peer.join().unwrap();
    transport.disconnect(&mut epochs).unwrap();
    drop(transport);
    assert!(!directory.exists());
}
