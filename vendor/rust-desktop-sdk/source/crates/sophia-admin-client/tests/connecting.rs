use sophia_9p_client::pipeline::PipelineLimits;
use sophia_admin_client::connecting::{Connecting, Error};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};

#[test]
fn version_budget_expiry_and_clock_do_not_send_extra_bytes() {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    let mut connecting = Connecting::new(client, PipelineLimits::default(), 10, 20).unwrap();
    assert_eq!(connecting.next_wake(), Some(20));
    assert_eq!(connecting.dispatch(9, 4096), Err(Error::ClockRegression));
    connecting.dispatch(10, 0).unwrap();
    let mut bytes = [0; 128];
    assert_eq!(
        peer.read(&mut bytes).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    connecting.dispatch(19, 1).unwrap();
    assert_eq!(peer.read(&mut bytes).unwrap(), 1);
    assert_eq!(connecting.dispatch(20, 4096), Err(Error::Deadline));
    assert!(
        connecting.fd().is_none() && !connecting.ready() && connecting.take_session().is_none()
    );
    assert_eq!(peer.read(&mut bytes).unwrap(), 0);
}
#[test]
fn unsupported_version_closes_without_attach_or_fallback() {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    let mut connecting = Connecting::new(client, PipelineLimits::default(), 0, 100).unwrap();
    connecting.dispatch(0, 4096).unwrap();
    let mut header = [0; 7];
    peer.read_exact(&mut header).unwrap();
    assert_eq!(header[4], 100);
    let mut body = vec![0; u32::from_le_bytes(header[..4].try_into().unwrap()) as usize - 7];
    peer.read_exact(&mut body).unwrap();
    let mut response = 16u32.to_le_bytes().to_vec();
    response.push(101);
    response.extend([255, 255]);
    response.extend(4096u32.to_le_bytes());
    response.extend(3u16.to_le_bytes());
    response.extend(b"bad");
    peer.write_all(&response).unwrap();
    assert!(connecting.dispatch(1, 4096).is_err());
    assert!(connecting.fd().is_none());
    assert_eq!(peer.read(&mut header).unwrap(), 0);
}
