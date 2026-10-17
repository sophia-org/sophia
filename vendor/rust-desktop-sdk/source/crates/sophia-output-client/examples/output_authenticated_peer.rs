//! Generic protected-admission peer. The supervisor's test bootstrap moves the
//! original launch channel onto stdin without changing process identity.
use sophia_output_client::{
    Connecting, SessionState,
    protocol::{OutputV1ClientHello, SOPHIA_OUTPUT_CAPABILITY_OBSERVE},
};
use std::{
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

fn main() {
    let deadline = Instant::now() + Duration::from_secs(10);
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay = UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let stream = UnixStream::connect(std::env::var_os("SOPHIA_TEST_LOCK_SOCKET").unwrap()).unwrap();
    let auth_clock = Instant::now();
    let mut auth =
        sophia_launch_client::Client::new(stream, relay, b"output", b"output-authority", 0, 5000)
            .unwrap();
    for round in 0..2 {
        while !auth.ready() {
            assert!(Instant::now() < deadline, "authentication deadline");
            auth.dispatch(auth_clock.elapsed().as_millis() as u64, 65536)
                .unwrap();
            std::thread::sleep(Duration::from_millis(1));
        }
        if round == 0 {
            let now = auth_clock.elapsed().as_millis() as u64;
            auth = auth
                .into_authenticated()
                .unwrap()
                .reauthenticate_at(now, now + 5000)
                .unwrap();
        }
    }
    let admitted = auth.into_authenticated().unwrap();
    let _relay = admitted.relay;
    let start = Instant::now();
    let mut connecting = Connecting::attached(
        admitted.role,
        admitted.root,
        OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE,
        },
        0,
        5000,
    )
    .unwrap();
    while !connecting.ready() {
        assert!(Instant::now() < deadline, "output negotiation deadline");
        connecting
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut session = connecting.take_session().unwrap();
    assert_eq!(session.state(), SessionState::Ready);
    assert_eq!(session.epoch(), 7);
    assert_eq!(
        session.capabilities(),
        Some(SOPHIA_OUTPUT_CAPABILITY_OBSERVE)
    );
    let mut observed = false;
    while !observed || session.acked() < 3 || session.obligations().deadline.is_some() {
        assert!(Instant::now() < deadline, "topology/ACK deadline");
        session
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
        if session.event().is_some() {
            assert_eq!(session.topology().unwrap().snapshot.topology_epoch, 4);
            session.consume().unwrap();
            observed = true;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let report = std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap());
    std::fs::write(&report, b"waiting\n").unwrap();
    while session.error().is_none() {
        assert!(Instant::now() < deadline, "revocation deadline");
        let _ = session.dispatch(start.elapsed().as_millis() as u64, 65536);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(matches!(
        session.state(),
        SessionState::Closed | SessionState::Stale
    ));
    assert_eq!(
        session.custody(1),
        Some(sophia_output_client::submission::Custody::Submitted)
    );
    assert!(session.fd().is_none() && session.event().is_none());
    std::fs::write(&report, b"revoked\n").unwrap();
    session.close();
}
