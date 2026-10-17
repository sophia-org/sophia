//! Generic protected-admission peer. The supervisor's test bootstrap moves the
//! original launch channel onto stdin without changing process identity.
use sophia_wm_client::{
    Connecting, SessionState,
    protocol::{SOPHIA_WM_CAPABILITY_CONFIGURATION, wm_files::WmFileNegotiationOffer},
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
    let mut auth =
        sophia_launch_client::Client::over(stream, relay, b"wm", b"spatial-policy").unwrap();
    while !auth.ready() {
        assert!(Instant::now() < deadline, "authentication deadline");
        auth.poll().unwrap();
        auth.wait_for_io(Duration::from_millis(10)).unwrap();
    }
    let admitted = auth.into_authenticated().unwrap();
    let _relay = admitted.relay;
    let start = Instant::now();
    let mut connecting = Connecting::attached(
        admitted.role,
        admitted.root,
        WmFileNegotiationOffer {
            required_capabilities: SOPHIA_WM_CAPABILITY_CONFIGURATION,
            optional_capabilities: 0,
        },
        0,
        5000,
    )
    .unwrap();
    while !connecting.ready() {
        assert!(Instant::now() < deadline, "WM negotiation deadline");
        connecting
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut session = connecting.take_session().unwrap();
    assert_eq!(session.state(), SessionState::Ready);
    assert_eq!(session.epoch(), 9);
    assert_eq!(
        session.capabilities(),
        Some(SOPHIA_WM_CAPABILITY_CONFIGURATION)
    );
    while session.obligations().deadline.is_some() {
        assert!(Instant::now() < deadline, "negotiation cleanup deadline");
        session
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    let report = std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_LOCK_REPORT").unwrap());
    std::fs::write(&report, b"authenticated-and-negotiated").unwrap();
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
        Some(sophia_wm_client::submission::Custody::Submitted)
    );
    assert!(session.fd().is_none() && session.event().is_none());
    std::fs::write(
        report.with_extension("revoked"),
        b"revoked-custody-retained",
    )
    .unwrap();
    session.close();
}
