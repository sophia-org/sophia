//! Dispatch cost screen: real observed wire turns in both treatments.
use super::*;
use rustix::time::{ClockId, clock_gettime};

#[test]
fn observed_socket_cost_path_preserves_every_record() {
    assert_eq!(run(8).len(), 32);
}

#[test]
fn observed_carrier_cost_path_preserves_every_record() {
    assert_eq!(run(9).len(), 32);
}

#[test]
#[ignore = "untraced release-mode dispatch screen, separately selected"]
fn owner_cpu_socket() {
    assert_eq!(run(10).len(), 4096);
}

#[test]
#[ignore = "untraced release-mode dispatch screen, separately selected"]
fn owner_cpu_split() {
    assert_eq!(run(11).len(), 4096);
}

fn cpu_ns() -> u64 {
    let time = clock_gettime(ClockId::ThreadCPUTime);
    u64::try_from(time.tv_sec).unwrap() * 1_000_000_000 + u64::try_from(time.tv_nsec).unwrap()
}

pub(super) fn owner(
    wire: &mut ShellFileWire,
    peer: &mut process::Peer,
    requests: &Reader,
    responses: &Writer,
    source: &Source,
    binding: Binding,
    mode: u8,
) -> Vec<Inbound> {
    let socket = mode.is_multiple_of(2);
    let expected = attempts(mode) as usize;
    let child_pidfd = pidfd_open(
        Pid::from_raw(peer.child.0.id() as i32).unwrap(),
        PidfdFlags::empty(),
    )
    .unwrap();
    let mut records = Vec::with_capacity(expected);
    let mut captured = Captured::buffer();
    let mut slot = Slot::default();
    let mut services = 0;
    let mut dispatches = 0;
    let mut cpu = 0u64;
    let deadline = Instant::now() + Duration::from_secs(10);
    // The fixture's record epoch is fixed; never infer authority from its header.
    assert_eq!(binding.epoch.0, 1);
    peer.stream.write_all(b"G").unwrap();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "dispatch screen progress deadline");
        let subscription = wire.subscription().unwrap();
        let mut fds = vec![PollFd::new(&peer.stream, PollFlags::IN)];
        wire.server.visit_poll_fds(|fd| fds.push(fd));
        assert_eq!(fds.len(), 3, "one wire wake reader and one actual slot");
        fds.push(PollFd::new(requests, PollFlags::IN));
        fds.push(PollFd::new(&child_pidfd, PollFlags::IN));
        assert!(
            poll(
                &mut fds,
                Some(&Timespec {
                    tv_sec: remaining.as_secs() as i64,
                    tv_nsec: remaining.subsec_nanos() as i64,
                }),
            )
            .unwrap()
                > 0
        );
        assert!(fds[4].revents().is_empty(), "exit precedes service");
        for fd in &fds[..4] {
            assert!(
                !fd.revents()
                    .intersects(PollFlags::HUP | PollFlags::ERR | PollFlags::NVAL)
            );
        }
        // K is sent only after the child validates its last reply.
        if fds[0].revents().contains(PollFlags::IN) {
            assert_eq!(records.len(), expected);
            break;
        }
        let hint_ready = fds[3].revents().contains(PollFlags::IN);
        assert!(!socket || !hint_ready);
        let observation = subscription.observe(&fds[1..3]).unwrap();
        assert!(wire.server.observation_is_current(&observation.wire));
        drop(fds);

        let before = cpu_ns();
        wire.set_readiness(Some(observation));
        // Both treatments retain pre/post authentication poll, expiry and image service.
        wire.turn().unwrap();
        if hint_ready {
            let hint = requests.next(slot.last_sequence()).unwrap().unwrap();
            slot.capture_published(hint, source, &mut captured, ReadPlan::HeaderThenPayload)
                .unwrap();
            binding.process(wire, &mut slot, &captured).unwrap();
            binding.deliver(wire, &mut slot, responses).unwrap();
            dispatches += 1;
        }
        let after = cpu_ns();
        cpu = cpu.checked_add(after.checked_sub(before).unwrap()).unwrap();
        services += 1;
        // Test-owned draining keeps the bounded inbound queue live; no rendering is simulated.
        records.extend(std::iter::from_fn(|| wire.export_mut().take_inbound()));
        assert!(records.len() <= expected);
    }
    assert!(!slot.pending());
    assert_eq!(dispatches, if socket { 0 } else { expected });
    census_line(format_args!(
        "CARRIER_CPU mode={mode} attempts={expected} services={services} carrier_dispatches={dispatches} service_cpu_ns={cpu}"
    ));
    records
}
