//! Supervised socket connector; the parent speaks protocol on relayed descriptors.
//! Supplied afid verdicts do not establish production launch authentication.
use sophia_runtime::lock_files::*;
use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, SupervisedProcessKind, SupervisorCommand,
};
use std::io::{Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
#[path = "image_test_authority.rs"]
mod authority;
const RELAY: &str = "SOPHIA_TEST_LOCK_IMAGE_RELAY";
const LOCK: &str = "SOPHIA_TEST_LOCK_IMAGE_SOCKET";
const IMAGE: &str = "SOPHIA_TEST_LOCK_IMAGE_ENDPOINT";
fn send_fd(relay: &UnixStream, fd: BorrowedFd<'_>) {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    let rights = [fd];
    assert!(control.push(SendAncillaryMessage::ScmRights(&rights)));
    let sent = sendmsg(
        relay,
        &[std::io::IoSlice::new(b"f")],
        &mut control,
        SendFlags::NOSIGNAL,
    )
    .unwrap();
    assert_eq!(sent, 1);
}

fn receive_fd(relay: &UnixStream) -> OwnedFd {
    // The receive itself does not block; wait for the child's message.
    let mut ready = [rustix::event::PollFd::new(
        relay,
        rustix::event::PollFlags::IN,
    )];
    let timeout = rustix::event::Timespec {
        tv_sec: 5,
        tv_nsec: 0,
    };
    assert_eq!(rustix::event::poll(&mut ready, Some(&timeout)).unwrap(), 1);
    let mut byte = [0u8; 1];
    let mut message = sophia_linux_peer::receive_rights(relay.as_fd(), &mut byte).unwrap();
    assert_eq!((message.bytes, message.count), (1, 1));
    message.descriptors[0].take().unwrap()
}

/// The supervised provider. Inert unless launched by `Provider::start`.
#[test]
fn child() {
    let Ok(relay) = std::env::var(RELAY) else {
        return;
    };
    let mut relay = UnixStream::connect(relay).unwrap();
    let lock = UnixStream::connect(std::env::var(LOCK).unwrap()).unwrap();
    send_fd(&relay, lock.as_fd());
    drop(lock);
    let image = PathBuf::from(std::env::var(IMAGE).unwrap());
    let mut command = [0u8; 1];
    // Each 'i' connects one more image socket; anything else, or EOF, exits.
    while relay.read(&mut command).unwrap_or(0) == 1 && command[0] == b'i' {
        let socket = rustix::net::socket_with(
            rustix::net::AddressFamily::UNIX,
            rustix::net::SocketType::SEQPACKET,
            rustix::net::SocketFlags::CLOEXEC,
            None,
        )
        .unwrap();
        rustix::net::connect(&socket, &rustix::net::SocketAddrUnix::new(&image).unwrap()).unwrap();
        send_fd(&relay, socket.as_fd());
    }
    std::process::exit(0);
}

/// Alive and not yet a zombie.
fn running(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| !stat.contains(") Z "))
}

pub struct Provider {
    supervisor: ProcessSupervisor,
    relay: UnixStream,
}

impl Provider {
    pub fn start(transport: &mut LockFileTransport, directory: &Path) -> (Self, UnixStream) {
        let relay_path = directory.with_extension("relay");
        let _ = std::fs::remove_file(&relay_path);
        let listener = UnixListener::bind(&relay_path).unwrap();
        let spec = ProcessLaunchSpec::new(std::env::current_exe().unwrap())
            .arg("provider::child")
            .arg("--exact")
            .arg("--test-threads=1")
            .arg("-q")
            .env(RELAY, &relay_path)
            .env(LOCK, transport.socket_path())
            .env(IMAGE, directory.join("image.sock"));
        let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::LockProvider, spec);
        supervisor
            .apply(SupervisorCommand::StartProcess {
                process: SupervisedProcessKind::LockProvider,
                delay: Duration::ZERO,
            })
            .unwrap();
        transport
            .authorize_assignee(
                LockFileAssignee::from_supervisor(&supervisor)
                    .unwrap()
                    .with_authentication(Box::new(authority::Factory)),
            )
            .unwrap();
        let (relay, _) = listener.accept().unwrap();
        relay
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let _ = std::fs::remove_file(&relay_path);
        let stream = UnixStream::from(receive_fd(&relay));
        (Self { supervisor, relay }, stream)
    }

    pub fn connect_image(&mut self) -> OwnedFd {
        self.relay.write_all(b"i").unwrap();
        receive_fd(&self.relay)
    }

    /// Ends the provider process; its pidfd becomes readable.
    fn exit(&mut self) {
        let _ = self.relay.write_all(b"q");
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.supervisor.child_id().is_some_and(running) {
            assert!(Instant::now() < deadline, "provider did not exit");
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Provider {
    fn drop(&mut self) {
        self.exit();
        let _ = self.supervisor.poll();
    }
}
