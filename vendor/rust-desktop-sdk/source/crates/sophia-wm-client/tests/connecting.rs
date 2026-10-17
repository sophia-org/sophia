use sophia_9p_client::pipeline::PipelineLimits;
use sophia_wm_client::{
    Connecting, SessionError, SessionState, protocol::wm_files::WmFileNegotiationOffer,
};
use std::{
    io::{ErrorKind, Read, Write},
    os::unix::net::UnixStream,
};

fn offer() -> WmFileNegotiationOffer {
    WmFileNegotiationOffer {
        required_capabilities: 0,
        optional_capabilities: 0,
    }
}

#[test]
fn version_and_file_startup_share_the_original_deadline() {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let mut c = Connecting::direct(client, PipelineLimits::default(), offer(), 10, 20).unwrap();
    assert_eq!(c.state(), SessionState::Negotiating);
    assert_eq!(c.epoch(), None);
    assert_eq!(c.limits(), None);
    assert_eq!(c.capabilities(), None);
    assert_eq!(c.obligations().deadline, Some(20));
    assert_eq!(c.remote_error(), None);
    assert_eq!(c.dispatch(9, 65536), Err(SessionError::ClockRegression));
    c.dispatch(10, 0).unwrap();
    let mut request = [0; 21];
    assert_eq!(
        peer.read(&mut request).unwrap_err().kind(),
        ErrorKind::WouldBlock
    );
    c.dispatch(10, 65536).unwrap();
    peer.read_exact(&mut request).unwrap();
    request[4] = 101;
    peer.write_all(&request).unwrap();
    c.dispatch(19, 65536).unwrap();
    // Version has completed; the queued attach must not send at expiry.
    assert_eq!(c.deadline(), 20);
    assert!(c.dispatch(20, 65536).is_err());
    assert!(c.fd().is_none());
    assert!(!c.ready());
    assert_eq!(c.state(), SessionState::Closed);
    assert!(c.error().is_some());
    assert_eq!(c.obligations().deadline, None);
    assert_eq!(peer.read(&mut request).unwrap(), 0);
}

#[test]
fn startup_preserves_remote_errno_and_terminal_state_after_close() {
    for (errno, expected) in [(13u32, SessionState::Failed), (116, SessionState::Stale)] {
        let (client, mut peer) = UnixStream::pair().unwrap();
        let mut c =
            Connecting::direct(client, PipelineLimits::default(), offer(), 10, 100).unwrap();
        c.dispatch(10, 65536).unwrap();
        peer.read_exact(&mut [0; 21]).unwrap();
        let mut response = 11u32.to_le_bytes().to_vec();
        response.push(7);
        response.extend(u16::MAX.to_le_bytes());
        response.extend(errno.to_le_bytes());
        peer.write_all(&response).unwrap();
        assert!(c.dispatch(11, 65536).is_err());
        assert_eq!(c.remote_error(), Some(errno));
        assert_eq!(c.state(), expected);
        c.close();
        assert_eq!(c.remote_error(), Some(errno));
        assert_eq!(c.state(), expected);
        assert!(c.fd().is_none());
        assert_eq!(c.obligations().deadline, None);
    }
}

#[test]
fn invalid_offer_is_rejected_without_network_io() {
    let (client, mut peer) = UnixStream::pair().unwrap();
    let bad = WmFileNegotiationOffer {
        required_capabilities: 1,
        optional_capabilities: 1,
    };
    assert!(Connecting::direct(client, PipelineLimits::default(), bad, 10, 20).is_err());
    assert_eq!(peer.read(&mut [0; 1]).unwrap(), 0);
}
