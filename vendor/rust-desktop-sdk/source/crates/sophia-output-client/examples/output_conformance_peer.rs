//! Independent peer for Sophia's supplied-admission output export gate.
//! Topology and validation outcome are supplied; this does not drive hardware.
use sophia_9p_client::pipeline::{IoInterest, PipelineLimits};
use sophia_output_client::{journal::Event, protocol::*, submission::Custody, *};
use std::{
    io::Read,
    os::{fd::BorrowedFd, unix::net::UnixStream},
    time::{Duration, Instant},
};

struct Clock(Instant);
impl Clock {
    fn now(&self) -> u64 {
        self.0.elapsed().as_millis() as u64
    }
    fn wait(&self, fd: BorrowedFd<'_>, interest: IoInterest, wake: Option<u64>) {
        let now = self.now();
        assert!(now < 5000, "fixture deadline");
        if interest.immediate {
            return;
        }
        let millis = wake.unwrap_or(5000).min(5000).saturating_sub(now);
        let mut flags = rustix::event::PollFlags::empty();
        if interest.readable {
            flags |= rustix::event::PollFlags::IN;
        }
        if interest.writable {
            flags |= rustix::event::PollFlags::OUT;
        }
        let mut fds = [rustix::event::PollFd::new(&fd, flags)];
        let timeout = rustix::event::Timespec::try_from(Duration::from_millis(millis)).unwrap();
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(_) | Err(rustix::io::Errno::INTR) => (),
            Err(e) => panic!("poll: {e}"),
        }
    }
}
fn main() {
    let path = std::env::args_os().nth(1).expect("socket path");
    let mut gate = [0];
    std::io::stdin().read_exact(&mut gate).unwrap();
    assert_eq!(gate, [b'G']);
    let clock = Clock(Instant::now());
    let mut connecting = Connecting::direct(
        UnixStream::connect(path).unwrap(),
        PipelineLimits {
            msize: 4096,
            ..PipelineLimits::default()
        },
        OutputV1ClientHello {
            minimum_revision: 1,
            maximum_revision: 1,
            capabilities: SOPHIA_OUTPUT_CAPABILITY_OBSERVE | SOPHIA_OUTPUT_CAPABILITY_CONFIGURE,
        },
        clock.now(),
        5000,
    )
    .unwrap();
    while !connecting.ready() {
        clock.wait(
            connecting.fd().unwrap(),
            connecting.io_interest().unwrap(),
            connecting.wake_deadline(),
        );
        connecting.dispatch(clock.now(), 65536).unwrap();
    }
    let mut session = connecting.take_session().unwrap();
    let mut ticket = None;
    let mut terminal = false;
    loop {
        clock.wait(
            session.fd().unwrap(),
            session.io_interest().unwrap(),
            session.wake_deadline(),
        );
        session.dispatch(clock.now(), 65536).unwrap();
        if let Some((_, event)) = session.event() {
            match event {
                Event::ObjectPublished(_) => {
                    assert!(ticket.is_none());
                    let topology = &session.topology().unwrap().snapshot;
                    assert_eq!(topology.topology_epoch, 4);
                    assert_eq!(topology.heads.len(), 1);
                    assert_eq!(topology.groups.len(), 1);
                    let head = &topology.heads[0];
                    let group = &topology.groups[0];
                    let proposal = OutputV1Proposal {
                        connection_epoch: session.epoch(),
                        candidate: OutputTopologyCandidate {
                            base_topology_epoch: topology.topology_epoch,
                            intent: OutputTopologyIntent::ValidateOnly,
                            primary_group_index: 0,
                            heads: vec![OutputHeadTargetProposal {
                                head: head.head,
                                head_generation: head.generation,
                                mode: head.current_mode.unwrap(),
                                transform: OutputTransform::Normal,
                                vrr: OutputVrrPolicy::Disabled,
                            }],
                            groups: vec![OutputLogicalGroupProposal {
                                output: group.output,
                                logical: group.logical,
                                members: group.members.clone(),
                            }],
                        },
                    };
                    session.consume().unwrap();
                    let transaction = TransactionId::from_raw(session.next_transaction().unwrap());
                    ticket = Some(session.submit(transaction, &proposal, 5000).unwrap());
                }
                Event::Outcome(_, outcome) => {
                    assert_eq!(outcome.kind, OutputV1OutcomeKind::Validated);
                    assert_eq!(session.custody(ticket.unwrap()), Some(Custody::Submitted));
                    session.consume().unwrap();
                    terminal = true;
                }
            }
        }
        if terminal && session.acked() >= 5 {
            break;
        }
    }
    session.close();
    println!("output Rust SDK: negotiated, exact topology read, proposal, terminal ack");
}
