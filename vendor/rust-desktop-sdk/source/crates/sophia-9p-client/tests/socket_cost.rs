//! Opt-in local syscall-wrapper diagnostic; no native performance verdict.
#![cfg(target_os = "linux")]
use std::hint::black_box;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Instant;

use rustix::net::{RecvFlags, SendFlags};

#[derive(Clone, Copy, Debug)]
enum Wrapper {
    Std,
    Rustix,
}

fn receive(wrapper: Wrapper, mut socket: &UnixStream, bytes: &mut [u8]) -> io::Result<usize> {
    match wrapper {
        Wrapper::Std => socket.read(bytes),
        Wrapper::Rustix => rustix::net::recv(socket, bytes, RecvFlags::empty())
            .map(|(_, count)| count)
            .map_err(Into::into),
    }
}

fn send(wrapper: Wrapper, mut socket: &UnixStream, bytes: &[u8]) -> io::Result<usize> {
    match wrapper {
        Wrapper::Std => socket.write(bytes),
        Wrapper::Rustix => {
            rustix::net::send(socket, bytes, SendFlags::NOSIGNAL).map_err(Into::into)
        }
    }
}

#[test]
fn socket_wrappers_preserve_empty_data_eof_and_failed_write() {
    for wrapper in [Wrapper::Std, Wrapper::Rustix] {
        let (socket, mut peer) = UnixStream::pair().unwrap();
        socket.set_nonblocking(true).unwrap();
        let mut bytes = [0; 4096];
        assert_eq!(
            receive(wrapper, &socket, &mut bytes).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        peer.write_all(b"reply").unwrap();
        assert_eq!(receive(wrapper, &socket, &mut bytes).unwrap(), 5);
        assert_eq!(&bytes[..5], b"reply");
        assert_eq!(send(wrapper, &socket, b"request").unwrap(), 7);
        peer.read_exact(&mut bytes[..7]).unwrap();
        assert_eq!(&bytes[..7], b"request");
        peer.shutdown(std::net::Shutdown::Write).unwrap();
        assert_eq!(receive(wrapper, &socket, &mut bytes).unwrap(), 0);
        drop(peer);
        assert_eq!(
            send(wrapper, &socket, b"closed").unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
    }
}

#[test]
#[ignore = "local release-only timing diagnostic"]
fn paired_socket_wrapper_cost() {
    assert!(!black_box(cfg!(debug_assertions)), "run in release mode");
    let (socket, mut peer) = UnixStream::pair().unwrap();
    socket.set_nonblocking(true).unwrap();
    let mut bytes = [0; 4096];
    const CALLS: usize = 20_000;
    for data in [false, true] {
        let mut timings = [Vec::new(), Vec::new()];
        for round in 0..36 {
            for index in if round % 2 == 0 { [0, 1] } else { [1, 0] } {
                let wrapper = [Wrapper::Std, Wrapper::Rustix][index];
                let start = Instant::now();
                for _ in 0..CALLS {
                    if data {
                        assert_eq!(
                            send(black_box(wrapper), &socket, black_box(b"12345678")).unwrap(),
                            8
                        );
                        peer.read_exact(&mut bytes[..8]).unwrap();
                    } else {
                        assert_eq!(
                            receive(black_box(wrapper), &socket, black_box(&mut bytes))
                                .unwrap_err()
                                .kind(),
                            io::ErrorKind::WouldBlock
                        );
                    }
                }
                let elapsed = start.elapsed().as_nanos();
                if round >= 4 {
                    timings[index].push(elapsed / CALLS as u128);
                }
            }
        }
        for (index, values) in timings.iter_mut().enumerate() {
            values.sort_unstable();
            println!(
                "socket_wrapper_cost wrapper={:?} case={} median_ns={} min_ns={} calls={CALLS} rounds={}",
                [Wrapper::Std, Wrapper::Rustix][index],
                if data {
                    "send_and_peer_read"
                } else {
                    "empty_recv"
                },
                values[values.len() / 2],
                values[0],
                values.len()
            );
        }
    }
}
