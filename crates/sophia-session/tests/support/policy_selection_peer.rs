#![cfg(test)]

use super::*;

pub(in crate::live_session) fn startup(stream: UnixStream) -> (WmFileLimits, u64, u64, UnixStream) {
    super::startup::tests::selected_transport_startup(stream)
}

pub(in crate::live_session) fn authenticated_startup(
    stream: UnixStream,
) -> (WmFileLimits, u64, u64, UnixStream) {
    let stdin = std::io::stdin();
    rustix::io::fcntl_setfd(&stdin, rustix::io::FdFlags::CLOEXEC).unwrap();
    let relay = UnixStream::from(rustix::io::fcntl_dupfd_cloexec(&stdin, 3).unwrap());
    let mut client = sophia_launch_client::Client::over(
        stream.try_clone().unwrap(),
        relay,
        b"wm",
        b"spatial-policy",
    )
    .unwrap();
    while !client.ready() {
        client
            .poll()
            .expect("normal WM owner must authenticate its protected peer");
        client.wait_for_io(Duration::from_millis(10)).unwrap();
    }
    let admitted = client.into_authenticated().unwrap();
    let root = admitted.root.0;
    drop(admitted);
    stream.set_nonblocking(false).unwrap();
    let mut peer = super::startup::tests::Peer::from_stream(stream);
    if root != 1 {
        assert_eq!(
            peer.rpc(
                110,
                &[
                    root.to_le_bytes().as_slice(),
                    &1u32.to_le_bytes(),
                    &0u16.to_le_bytes()
                ]
                .concat()
            )
            .unwrap()
            .0,
            111
        );
        assert_eq!(peer.rpc(120, &root.to_le_bytes()).unwrap().0, 121);
    }
    peer.open(2, b"events", 0);
    peer.open(3, b"submit", 1);
    peer.open(4, b"ack", 1);
    super::startup::tests::selected_transport_admitted(peer)
}
