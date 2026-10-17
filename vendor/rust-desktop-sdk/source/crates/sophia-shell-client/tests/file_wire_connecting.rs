#[allow(dead_code)]
#[path = "support/file_wire_peer.rs"]
mod peer;
use peer::{Peer, Profile, WAIT};
use sophia_9p_client::pipeline::{Pipeline, PipelineError, PipelineLimits, Reply};
use sophia_shell_client::{Connecting, ShellClientError, ShellClientOptions};
use std::{
    io::Read,
    os::unix::net::{UnixListener, UnixStream},
    sync::mpsc,
    time::Instant,
};

fn options(profile: Profile) -> ShellClientOptions {
    ShellClientOptions {
        minimum_revision: profile.revision,
        maximum_revision: profile.revision,
        required_capabilities: profile.capabilities,
        handshake_timeout: WAIT,
    }
}
#[test]
fn caller_driven_startup_matches_all_profiles_on_both_paths() {
    for attached in [false, true] {
        for (role, revision, capabilities) in [
            ("bar", 6, 0x83),
            ("launcher", 7, 0x9a0),
            ("dock", 8, 0x11a2),
            ("descriptor", 8, 3),
        ] {
            let path = std::env::temp_dir().join(format!(
                "shell-connecting-{role}-{attached}-{}",
                std::process::id()
            ));
            let listener = UnixListener::bind(&path).unwrap();
            let profile = Profile {
                role,
                revision,
                capabilities,
                limits_published: role != "descriptor",
            };
            let (stop, done) = mpsc::channel();
            let thread = std::thread::spawn(move || {
                let mut peer = Peer::fragmented_retry_handshake(listener, profile);
                let end = Instant::now() + WAIT;
                while done.try_recv().is_err() {
                    assert!(Instant::now() < end);
                    peer.pump();
                    std::thread::yield_now();
                }
                peer
            });
            let mut connecting = if attached {
                let mut wire = Pipeline::connect(&path, PipelineLimits::default(), WAIT).unwrap();
                let (tag, root) = wire.attach(b"supplied", role.as_bytes()).unwrap();
                assert!(matches!(
                    wire.wait(tag, Instant::now() + WAIT).unwrap(),
                    Reply::Attach(_)
                ));
                Connecting::attached(wire, root, options(profile), 0, 2000).unwrap()
            } else {
                Connecting::direct(
                    UnixStream::connect(&path).unwrap(),
                    PipelineLimits::default(),
                    options(profile),
                    0,
                    2000,
                )
                .unwrap()
            };
            let started = Instant::now();
            let end = started + WAIT;
            while !connecting.ready() {
                assert!(
                    Instant::now() < end,
                    "startup did not finish: {:?}",
                    connecting.error()
                );
                connecting
                    .dispatch(started.elapsed().as_millis() as u64, 1)
                    .unwrap();
                assert!(connecting.fd().is_some());
                std::thread::yield_now();
            }
            assert!(connecting.io_interest().unwrap().immediate);
            assert_eq!(connecting.wake_deadline(), None);
            let connection = connecting.take_connection().unwrap();
            assert!(connecting.take_connection().is_none());
            assert_eq!(connection.connection_epoch(), peer::EPOCH);
            assert_eq!(connection.welcome().selected_revision, revision);
            stop.send(()).unwrap();
            let peer = thread.join().unwrap();
            assert_eq!(peer.attaches, 1);
            assert_eq!(
                peer.acks.iter().map(|ack| ack.sequence).collect::<Vec<_>>(),
                [1, 2]
            );
            std::fs::remove_file(path).unwrap();
        }
    }
}
#[test]
fn constructor_zero_budget_regression_and_expiry_never_write() {
    let (stream, mut peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let profile = Profile {
        role: "bar",
        revision: 6,
        capabilities: 0x83,
        limits_published: true,
    };
    let mut c =
        Connecting::direct(stream, PipelineLimits::default(), options(profile), 5, 10).unwrap();
    let mut byte = [0];
    assert_eq!(
        peer.read(&mut byte).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    c.dispatch(6, 0).unwrap();
    assert_eq!(
        peer.read(&mut byte).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert_eq!(
        c.dispatch(5, 4096),
        Err(ShellClientError::InvalidArgument("clock regressed"))
    );
    assert!(c.error().is_none());
    assert_eq!(c.wake_deadline(), Some(10));
    assert_eq!(
        c.dispatch(10, 4096),
        Err(ShellClientError::Pipeline(PipelineError::Timeout))
    );
    assert_eq!(peer.read(&mut byte).unwrap(), 0);
    assert!(c.fd().is_none());
    c.close();
    assert_eq!(
        c.error(),
        Some(&ShellClientError::Pipeline(PipelineError::Timeout))
    );
}
