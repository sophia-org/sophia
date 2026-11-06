//! Compare test-only notice elision with the explicitly negotiated implementation.
//! Both keep real preparation, native ownership and terminal-credit handling.
use super::*;
use std::io::Write;
use std::os::fd::{AsFd, AsRawFd};

pub(super) struct Pending {
    pub(super) stream: FrameStream,
    pub(super) source: u64,
}

#[derive(Default)]
struct Counts {
    outcomes: Vec<u16>,
    acknowledgements: usize,
    input_visits: usize,
    wake_ready: usize,
}

#[test]
fn terminal_probe_coalesced_control() {
    lifecycle(Case::Probe {
        omit_prepared: false,
        split: false,
    });
}

#[test]
fn terminal_probe_coalesced_elided() {
    lifecycle(Case::Probe {
        omit_prepared: true,
        split: false,
    });
}

#[test]
fn terminal_probe_split_control() {
    lifecycle(Case::Probe {
        omit_prepared: false,
        split: true,
    });
}

#[test]
fn terminal_probe_split_elided() {
    lifecycle(Case::Probe {
        omit_prepared: true,
        split: true,
    });
}

fn ready(t: &Transport) -> (bool, bool) {
    let mut fds = t.transport.poll_fds();
    let any =
        rustix::event::poll(&mut fds, Some(&rustix::event::Timespec::default())).unwrap() != 0;
    (any, fds[0].revents().contains(rustix::event::PollFlags::IN))
}

fn host(t: &mut Transport, owner: &mut crate::ShellIsbImageOwner, f: &fixture::Fixture) {
    t.transport.service_owner_turn().unwrap();
    owner
        .service(
            &mut t.transport.state,
            &mut t.transport.content_epochs,
            10,
            Instant::now(),
        )
        .unwrap();
    t.transport.service_content_resources(10).unwrap();
    let contexts = [crate::ContentCandidateContext {
        output: fixture::key(1).output,
        facts_generation: fixture::key(1).facts_generation,
        interaction_generation: 8,
        allocations: &f.allocations,
    }];
    t.transport
        .state
        .service_content_candidates(&mut t.transport.content_epochs, &contexts, 10)
        .unwrap();
    t.transport.poll_io().unwrap();
}

fn peer(client: &mut ShellConnection, stream: &mut FrameStream, counts: &mut Counts) {
    // Drain buffered events before returning to the host, as the peer's progress
    // loop does. No concurrent host can complete two ACKs inside one poll_io.
    for _ in 0..16 {
        let before = client.journal_progress().acknowledged;
        client.poll_io().unwrap();
        counts.acknowledgements += usize::from(client.journal_progress().acknowledged > before);
        let mut consumed = false;
        while let Some(record) = client.take_frame().unwrap() {
            stream.observe(&record).unwrap();
            let ShellFrameRecord::Outcome {
                outcome,
                credit_expires_msec,
                ..
            } = record
            else {
                panic!("unexpected frame control during the copied frame")
            };
            assert_eq!(outcome.candidate_generation, 1);
            match outcome.kind {
                1 => {
                    assert_eq!(credit_expires_msec, 0);
                    assert!(!stream.has_credit());
                }
                2 => assert!(credit_expires_msec > 10 && stream.has_credit()),
                _ => panic!("frame failed"),
            }
            counts.outcomes.push(outcome.kind);
            consumed = true;
        }
        while client.take_content().unwrap().is_some() {
            consumed = true;
        }
        if !consumed {
            return;
        }
    }
    panic!("bounded fixture did not drain its buffered events");
}

fn drain(
    t: &mut Transport,
    owner: &mut crate::ShellIsbImageOwner,
    f: &fixture::Fixture,
    client: &mut ShellConnection,
    pending: &mut Pending,
    counts: &mut Counts,
    expected: usize,
) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "notice/ACK did not drain");
        peer(client, &mut pending.stream, counts);
        assert!(counts.outcomes.len() <= expected, "unexpected extra notice");
        let (input, wake) = ready(t);
        if input || t.transport.output_pending() || owner.pending() != 0 {
            counts.input_visits += 1;
            counts.wake_ready += usize::from(wake);
            host(t, owner, f);
            continue;
        }
        let progress = client.journal_progress();
        if counts.outcomes.len() == expected
            && !progress.ack_pending
            && progress.acknowledged == progress.handled
        {
            return;
        }
    }
}

pub(super) fn run(
    t: &mut Transport,
    owner: &mut crate::ShellIsbImageOwner,
    f: &fixture::Fixture,
    mut client: ShellConnection,
    mut pending: Pending,
    case: Case,
) {
    let (omit_prepared, split, negotiated) = match case {
        Case::Probe {
            omit_prepared,
            split,
        } => (omit_prepared, split, false),
        Case::TerminalOnly { split } => (true, split, true),
        _ => unreachable!(),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut setup = Counts::default();
    loop {
        assert!(Instant::now() < deadline, "copied frame was not admitted");
        peer(&mut client, &mut pending.stream, &mut setup);
        host(t, owner, f);
        if t.candidates().pending_candidate_count() == 1
            && owner.pending() == 0
            && client.image_ring_offer(pending.source).unwrap().state
                == sophia_shell_client::RingOfferState::Settled
        {
            break;
        }
    }
    assert!(setup.outcomes.is_empty());
    drain(t, owner, f, &mut client, &mut pending, &mut setup, 0);
    assert!(!pending.stream.has_credit());
    let key = fixture::key(1);
    let bundle = t
        .transport
        .begin_content_submission(key.output, 1, 10)
        .unwrap();
    let wake = std::fs::read_link(format!(
        "/proc/self/fd/{}",
        t.transport.poll_fds()[0].as_fd().as_raw_fd()
    ))
    .unwrap();
    std::io::stderr()
        .write_all(
            format!(
                "TERMINAL_PROBE_BEGIN omit={omit_prepared} split={split} negotiated={negotiated} wake={}\n",
                wake.display()
            )
            .as_bytes(),
        )
        .unwrap();
    let before = t.transport.state.wire_turn_count();
    let mut counts = Counts::default();
    let reserved = t.candidates().control_occupancy();
    if negotiated {
        assert!(
            t.transport
                .content_presented_at(key.grant, key.output, 1, 40, 20, 30, 10)
                .is_err()
        );
        t.transport
            .state
            .content_limits
            .as_mut()
            .unwrap()
            .max_control_records = 0;
    }
    t.transport
        .content_prepared(key.grant, key.output, 1, 20, 30, 10)
        .unwrap();
    if negotiated {
        assert_eq!(t.candidates().control_occupancy(), reserved - 1);
        assert_eq!(t.transport.state.output.records(), 0);
        assert!(
            t.transport
                .content_prepared(key.grant, key.output, 1, 20, 30, 10)
                .is_err()
        );
        assert_eq!(t.candidates().control_occupancy(), reserved - 1);
        t.transport
            .state
            .content_limits
            .as_mut()
            .unwrap()
            .max_control_records = 64;
    } else if omit_prepared {
        // Test-only elision after real preparation and reservation transfer. This
        // is not a production API or proof of a future mode's refund arithmetic.
        assert_eq!(t.transport.state.output.records(), 1);
        assert!(matches!(t.take_record(), ShellFrameRecord::Outcome {
            outcome, credit_expires_msec: 0, ..
        } if outcome.kind == 1));
    }
    if split {
        t.transport.poll_io().unwrap();
        drain(
            t,
            owner,
            f,
            &mut client,
            &mut pending,
            &mut counts,
            usize::from(!omit_prepared),
        );
        assert!(!pending.stream.has_credit());
    }
    t.transport
        .content_presented_at(key.grant, key.output, 1, 40, 20, 30, 10)
        .unwrap();
    t.transport.poll_io().unwrap();
    drain(
        t,
        owner,
        f,
        &mut client,
        &mut pending,
        &mut counts,
        if omit_prepared { 1 } else { 2 },
    );
    assert_eq!(
        counts.outcomes,
        if omit_prepared { vec![2] } else { vec![1, 2] }
    );
    assert!(pending.stream.has_credit());
    assert_eq!(owner.pending(), 0);
    assert_eq!(bundle.candidate_generation, 1);
    image::assert_pixels(&t.transport.content_epochs, 1);
    let exchanges = 1 + usize::from(split && !omit_prepared);
    assert_eq!(counts.acknowledgements, exchanges);
    assert_eq!(counts.input_visits, exchanges);
    assert_eq!(
        t.transport.state.wire_turn_count() - before,
        2 * u64::try_from(exchanges).unwrap()
    );
    std::io::stderr().write_all(format!(
        "TERMINAL_PROBE_END omit={omit_prepared} split={split} negotiated={negotiated} records={} acks={} input_visits={} wire_services={} wake_ready={}\n",
        counts.outcomes.len(), counts.acknowledgements, counts.input_visits,
        t.transport.state.wire_turn_count() - before, counts.wake_ready,
    ).as_bytes()).unwrap();
}

#[test]
fn negotiated_terminal_only_coalesced() {
    lifecycle(Case::TerminalOnly { split: false });
}

#[test]
fn negotiated_terminal_only_split() {
    lifecycle(Case::TerminalOnly { split: true });
}
