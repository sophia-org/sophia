//! Generic production-export peer. The parent owns command tickets and admission.
use sophia_9p_client::pipeline::{IoInterest, PipelineLimits};
use sophia_admin_client::{
    command::Custody,
    connecting::{Connecting, Error as ConnectingError},
    protocol::{Command, Outcome, Owner},
    session::Session,
};
use std::{
    os::{fd::BorrowedFd, unix::net::UnixStream},
    time::{Duration, Instant},
};
struct Clock(Instant);
impl Clock {
    fn now(&self) -> u64 {
        let now = self.0.elapsed().as_millis() as u64;
        assert!(now < 10000);
        now
    }
    fn wait(&self, fd: BorrowedFd<'_>, interest: IoInterest, wake: Option<u64>) {
        if interest.immediate {
            return;
        }
        let mut flags = rustix::event::PollFlags::empty();
        if interest.readable {
            flags |= rustix::event::PollFlags::IN;
        }
        if interest.writable {
            flags |= rustix::event::PollFlags::OUT;
        }
        let timeout = rustix::event::Timespec::try_from(Duration::from_millis(
            wake.unwrap_or(10000).min(10000).saturating_sub(self.now()),
        ))
        .unwrap();
        let mut fds = [rustix::event::PollFd::new(&fd, flags)];
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(_) | Err(rustix::io::Errno::INTR) => (),
            Err(e) => panic!("{e}"),
        }
    }
    fn turn(&self, s: &mut Session) {
        self.wait(s.fd().unwrap(), s.io_interest().unwrap(), s.next_wake());
        let _ = s.dispatch(self.now(), 65536);
    }
    fn outcome(&self, s: &mut Session, id: u64, expected: Outcome) {
        while s.latest_custody() != Some((id, Custody::Settled)) {
            assert!(s.error().is_none(), "{:?}", s.error());
            self.turn(s);
        }
        assert_eq!(s.outcome().unwrap().0, expected);
        assert_eq!(s.next_wake(), None);
    }
}
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let path = &args[1];
    let report = &args[2];
    assert!(
        args[3] == "settle"
            || args[3] == "revoke"
            || args[3] == "loss"
            || args[3] == "recover"
            || args[3] == "denied"
    );
    let expect_loss = args[3] != "settle";
    let clock = Clock(Instant::now());
    let mut connecting = Connecting::new(
        UnixStream::connect(path).unwrap(),
        PipelineLimits::default(),
        0,
        2000,
    )
    .unwrap();
    while !connecting.ready() {
        clock.wait(
            connecting.fd().unwrap(),
            connecting.io_interest().unwrap(),
            connecting.next_wake(),
        );
        if let Err(error) = connecting.dispatch(clock.now(), 65536) {
            assert_eq!(args[3], "denied");
            // HostDomain denial closes the reachable socket before negotiation.
            assert!(
                matches!(
                    error,
                    ConnectingError::Wire(sophia_9p_client::pipeline::PipelineError::Io(
                        std::io::ErrorKind::UnexpectedEof
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::BrokenPipe
                    ))
                ),
                "{error:?}"
            );
            assert!(!connecting.ready());
            assert!(connecting.take_session().is_none());
            assert!(connecting.fd().is_none());
            return;
        }
    }
    assert_ne!(args[3], "denied", "foreign namespace admitted");
    let mut s = connecting.take_session().unwrap();
    assert_eq!(s.catalog_generation(), Some(7));
    let command = Command {
        owner: Owner::Policy,
        name: "focus next",
    };
    assert!(s.commands().any(|c| c == command));
    assert_eq!(s.invoke(command, clock.now()).unwrap(), 1);
    if expect_loss {
        while s.error().is_none() {
            clock.turn(&mut s);
        }
        assert_eq!(s.latest_custody(), Some((1, Custody::Unknown)));
        assert!(s.fd().is_none());
        assert!(s.invoke(command, clock.now()).is_err());
        if args[3] == "recover" {
            let prior_info = s.info();
            std::fs::write(report, b"lost\n").unwrap();
            let path = loop {
                let text = std::fs::read_to_string(report).unwrap();
                if let Some(path) = text
                    .strip_prefix("recover ")
                    .and_then(|p| p.strip_suffix('\n'))
                {
                    break path.to_owned();
                }
                clock.now();
                std::thread::sleep(Duration::from_millis(1));
            };
            let now = clock.now();
            let mut c = Connecting::new(
                UnixStream::connect(path).unwrap(),
                PipelineLimits::default(),
                now,
                now + 2000,
            )
            .unwrap();
            while !c.ready() {
                clock.wait(c.fd().unwrap(), c.io_interest().unwrap(), c.next_wake());
                c.dispatch(clock.now(), 65536).unwrap();
            }
            let mut fresh = c.take_session().unwrap();
            assert_ne!(
                (prior_info.session_low, prior_info.session_high),
                (fresh.info().session_low, fresh.info().session_high)
            );
            assert_eq!(fresh.catalog_generation(), Some(9));
            let probe = Command {
                owner: Owner::Policy,
                name: "probe",
            };
            assert!(fresh.commands().any(|c| c == probe));
            assert_eq!(fresh.invoke(probe, clock.now()).unwrap(), 1);
            clock.outcome(&mut fresh, 1, Outcome::Rejected);
            assert_eq!(s.latest_custody(), Some((1, Custody::Unknown)));
            fresh.close();
        }
    } else {
        clock.outcome(&mut s, 1, Outcome::Committed);
        std::fs::write(report, b"first\n").unwrap();
        while std::fs::read_to_string(report).unwrap() != "continue\n" {
            clock.now();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(s.invoke(command, clock.now()).unwrap(), 2);
        clock.outcome(&mut s, 2, Outcome::Stale);
        assert_eq!(s.catalog_generation(), None);
        s.refresh_catalog(clock.now()).unwrap();
        while s.catalog_generation().is_none() {
            assert!(s.error().is_none());
            clock.turn(&mut s);
        }
        assert_eq!(s.catalog_generation(), Some(8));
        assert_eq!(s.invoke(command, clock.now()).unwrap(), 3);
        clock.outcome(&mut s, 3, Outcome::Committed);
        s.close();
        assert_eq!(s.latest_custody(), Some((3, Custody::Settled)));
    }
    std::fs::write(report, b"ok\n").unwrap();
}
