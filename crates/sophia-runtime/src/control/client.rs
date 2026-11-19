use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

pub(super) fn connect_stream(path: &Path) -> io::Result<UnixStream> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "control socket must be absolute",
        ));
    }
    let fd = rustix::net::socket_with(
        rustix::net::AddressFamily::UNIX,
        rustix::net::SocketType::STREAM,
        rustix::net::SocketFlags::NONBLOCK | rustix::net::SocketFlags::CLOEXEC,
        None,
    )?;
    let address = rustix::net::SocketAddrUnix::new(path)?;
    match rustix::net::connect(&fd, &address) {
        Ok(()) => {}
        Err(rustix::io::Errno::INPROGRESS) => {
            wait(
                &UnixStream::from(fd.try_clone()?),
                true,
                Instant::now() + Duration::from_secs(2),
            )?;
            rustix::net::sockopt::socket_error(&fd)??;
        }
        Err(e) => return Err(e.into()),
    }
    let stream = UnixStream::from(fd);
    if rustix::net::sockopt::socket_peercred(&stream)?.uid != rustix::process::geteuid() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unexpected control server user",
        ));
    }
    Ok(stream)
}

fn wait(stream: &UnixStream, writing: bool, deadline: Instant) -> io::Result<()> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "control deadline"))?;
    let timeout = rustix::event::Timespec {
        tv_sec: remaining.as_secs() as i64,
        tv_nsec: remaining.subsec_nanos().into(),
    };
    let mut fds = [rustix::event::PollFd::new(
        stream,
        if writing {
            rustix::event::PollFlags::OUT
        } else {
            rustix::event::PollFlags::IN
        },
    )];
    if rustix::event::poll(&mut fds, Some(&timeout))? == 0 {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "control deadline"));
    }
    Ok(())
}
