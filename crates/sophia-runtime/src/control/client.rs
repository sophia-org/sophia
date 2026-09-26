use sophia_protocol::{
    ControlCatalog, ControlCommand, ControlMessage, ControlOutcome, decode_control_frame,
    decode_control_header, encode_control_frame,
};
use std::io::{self, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::{Duration, Instant};

/// Synchronous convenience client. A failed invocation is never reconnected
/// or replayed; callers can distinguish an uncertain send from no invocation.
pub struct ControlClient {
    stream: UnixStream,
    next_id: u64,
    catalog: Option<ControlCatalog>,
    frame_timeout: Duration,
    command_timeout: Duration,
    invocation_pending: bool,
    failed: bool,
    revision: u16,
}

impl ControlClient {
    pub fn connect(path: &Path) -> io::Result<Self> {
        Self::connect_revisions(path, 1, sophia_protocol::CONTROL_REVISION)
    }

    /// Connect offering exactly `minimum..=maximum`; the server selects the
    /// highest revision both speak.
    pub fn connect_revisions(path: &Path, minimum: u16, maximum: u16) -> io::Result<Self> {
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
        let mut client = Self {
            stream,
            next_id: 1,
            catalog: None,
            frame_timeout: Duration::from_secs(2),
            command_timeout: Duration::from_secs(2),
            invocation_pending: false,
            failed: false,
            revision: 0,
        };
        let reply = client.exchange(
            0,
            ControlMessage::Hello {
                minimum_revision: minimum,
                maximum_revision: maximum,
                required_features: 0,
            },
        )?;
        let ControlMessage::Welcome(welcome) = reply else {
            return Err(invalid("expected control welcome"));
        };
        if !(minimum..=maximum).contains(&welcome.revision) {
            return Err(invalid("control welcome selected an unoffered revision"));
        }
        client.revision = welcome.revision;
        client.frame_timeout = Duration::from_millis(welcome.frame_timeout_ms.into());
        client.command_timeout = Duration::from_millis(welcome.command_timeout_ms.into());
        Ok(client)
    }
    /// The negotiated revision.
    pub fn revision(&self) -> u16 {
        self.revision
    }
    pub fn invocation_pending(&self) -> bool {
        self.invocation_pending
    }
    pub fn commands(&mut self) -> io::Result<ControlCatalog> {
        match self.request(ControlMessage::Commands)? {
            ControlMessage::Catalog(catalog) => {
                self.catalog = Some(catalog.clone());
                Ok(catalog)
            }
            _ => self.protocol_failure("expected command catalog"),
        }
    }
    pub fn invoke(&mut self, command: ControlCommand) -> io::Result<(u64, ControlOutcome, String)> {
        let catalog = match &self.catalog {
            Some(c) => c.clone(),
            None => self.commands()?,
        };
        if !catalog.commands.contains(&command) {
            return Err(invalid("command is not advertised by this session"));
        }
        let generation = catalog.generation;
        self.invocation_pending = true;
        let reply = self.request(ControlMessage::Invoke {
            generation,
            command: command.clone(),
        })?;
        let ControlMessage::Outcome {
            generation: echoed,
            outcome,
            detail,
        } = reply
        else {
            return self.protocol_failure("expected command outcome");
        };
        if echoed != generation
            || (outcome.success()
                && !match command.owner {
                    sophia_protocol::ControlOwner::Policy => outcome == ControlOutcome::Committed,
                    sophia_protocol::ControlOwner::Session => {
                        outcome == ControlOutcome::Completed
                            || (command.name == "reload-profile"
                                && outcome == ControlOutcome::Unchanged)
                    }
                })
        {
            return self.protocol_failure("incorrect command settlement");
        }
        self.invocation_pending = false;
        if command.owner == sophia_protocol::ControlOwner::Session
            || matches!(
                outcome,
                ControlOutcome::Stale | ControlOutcome::Denied | ControlOutcome::Indeterminate
            )
        {
            self.catalog = None;
        }
        Ok((generation, outcome, detail))
    }
    fn protocol_failure<T>(&mut self, text: &str) -> io::Result<T> {
        self.failed = true;
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        Err(invalid(text))
    }
    fn request(&mut self, message: ControlMessage) -> io::Result<ControlMessage> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| invalid("request IDs exhausted"))?;
        self.exchange(id, message)
    }
    fn exchange(&mut self, id: u64, message: ControlMessage) -> io::Result<ControlMessage> {
        if self.failed {
            return Err(invalid(
                "control connection failed; mutation must not be replayed",
            ));
        }
        let result = self.exchange_inner(id, message);
        if result.is_err() {
            self.failed = true;
            let _ = self.stream.shutdown(std::net::Shutdown::Both);
        }
        result
    }
    fn exchange_inner(&mut self, id: u64, message: ControlMessage) -> io::Result<ControlMessage> {
        let frame =
            encode_control_frame(id, &message).map_err(|_| invalid("invalid control request"))?;
        let deadline = Instant::now() + self.frame_timeout;
        let mut written = 0;
        while written < frame.len() {
            check_deadline(deadline)?;
            match self.stream.write(&frame[written..]) {
                Ok(0) => return Err(invalid("control write EOF")),
                Ok(n) => written += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    wait(&self.stream, true, deadline)?
                }
                Err(e) => return Err(e),
            }
        }
        let mut bytes = vec![0; 24];
        receive(
            &self.stream,
            &mut bytes[..1],
            Instant::now() + self.command_timeout + self.frame_timeout,
        )?;
        let deadline = Instant::now() + self.frame_timeout;
        receive(&self.stream, &mut bytes[1..], deadline)?;
        let (_, _, len) =
            decode_control_header(&bytes).map_err(|_| invalid("invalid control header"))?;
        bytes.resize(24 + len, 0);
        receive(&self.stream, &mut bytes[24..], deadline)?;
        let (echoed, reply) =
            decode_control_frame(&bytes).map_err(|_| invalid("invalid control reply"))?;
        if let ControlMessage::ProtocolError { code } = reply {
            return Err(invalid(&format!("control protocol error {code}")));
        }
        if echoed != id {
            return Err(invalid("control reply correlation"));
        }
        Ok(reply)
    }
}

fn invalid(text: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, text.to_owned())
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
fn receive(stream: &UnixStream, mut bytes: &mut [u8], deadline: Instant) -> io::Result<()> {
    while !bytes.is_empty() {
        check_deadline(deadline)?;
        match sophia_linux_peer::recv_plain(stream.as_fd(), bytes, false) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "control disconnected",
                ));
            }
            Ok(n) => bytes = &mut bytes[n..],
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => wait(stream, false, deadline)?,
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn check_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(io::ErrorKind::TimedOut, "control deadline"))
    } else {
        Ok(())
    }
}
