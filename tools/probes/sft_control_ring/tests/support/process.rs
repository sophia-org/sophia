//! Shared device-free process fixture; no production entry point.
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::{
    fs::DirBuilderExt,
    net::{UnixListener, UnixStream},
};

pub struct Child(pub std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Directory(std::path::PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub struct Peer {
    pub child: Child,
    pub stream: UnixStream,
    _directory: Directory,
}
pub fn readable(fd: impl AsFd) {
    let mut fds = [PollFd::new(&fd, PollFlags::IN)];
    assert_eq!(
        poll(
            &mut fds,
            Some(&Timespec {
                tv_sec: 10,
                tv_nsec: 0
            })
        )
        .unwrap(),
        1
    );
    assert!(fds[0].revents().contains(PollFlags::IN));
}
fn bounded(stream: UnixStream) -> UnixStream {
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    stream
}
pub fn spawn(fixture: &str) -> Peer {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("sft-probe-{}-{stamp}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .unwrap();
    let directory = Directory(path);
    let socket = directory.0.join("control");
    let listener = UnixListener::bind(&socket).unwrap();
    let child = Child(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", fixture, "--ignored", "--nocapture"])
            .env("SFT_PROBE_TEST_SOCKET", socket)
            .spawn()
            .unwrap(),
    );
    readable(&listener);
    let (stream, _) = listener.accept().unwrap();
    Peer {
        child,
        stream: bounded(stream),
        _directory: directory,
    }
}
pub fn connect() -> UnixStream {
    bounded(
        UnixStream::connect(
            std::env::var_os("SFT_PROBE_TEST_SOCKET").expect("parent supplied path"),
        )
        .unwrap(),
    )
}
pub fn send_descriptors(stream: &UnixStream, fds: &[BorrowedFd<'_>], marker: u8) {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    use std::io::IoSlice;
    let mut space = [std::mem::MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut ancillary = SendAncillaryBuffer::new(&mut space);
    assert!(ancillary.push(SendAncillaryMessage::ScmRights(fds)));
    assert_eq!(
        sendmsg(
            stream,
            &[IoSlice::new(&[marker])],
            &mut ancillary,
            SendFlags::NOSIGNAL
        )
        .unwrap(),
        1
    );
}
