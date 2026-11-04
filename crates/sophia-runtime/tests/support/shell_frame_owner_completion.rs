use super::*;

fn submitted() -> Transport {
    let mut f = fixture::Fixture::new(64);
    f.open_with_credits(1, 1).unwrap();
    f.frame(1, 1, None).unwrap();
    let _display = f.submit(1);
    let now = f.now;
    let mut t = Transport::take(&mut f);
    t.flush(now).unwrap();
    assert!(matches!(t.take_record(), ShellFrameRecord::Opened { .. }));
    assert!(
        matches!(t.take_record(), ShellFrameRecord::Outcome { outcome, credit_expires_msec: 0, .. } if outcome.kind == 1)
    );
    t
}

fn complete(
    t: &mut Transport,
    failed: bool,
    generation: u64,
    now: u64,
) -> Result<(), ShellTransportError> {
    if failed {
        t.transport
            .content_renderer_failed_at(fixture::grant(), fixture::output(), generation, now)
    } else {
        t.transport.content_presented_at(
            fixture::grant(),
            fixture::output(),
            generation,
            40,
            20,
            30,
            now,
        )
    }
}

#[test]
fn timed_terminals_preserve_identity_and_retry_after_capacity_without_double_credit() {
    for failed in [false, true] {
        let mut t = submitted();
        assert_eq!(
            complete(&mut t, failed, 2, 20),
            Err(ShellTransportError::ContentCandidate(
                ContentCandidateError::Stale
            ))
        );
        let legacy = if failed {
            t.transport
                .content_renderer_failed(fixture::grant(), fixture::output(), 1)
        } else {
            t.transport
                .content_presented(fixture::grant(), fixture::output(), 1, 40, 20, 30)
        };
        assert_eq!(
            legacy,
            Err(ShellTransportError::ContentCandidate(
                ContentCandidateError::Stale
            ))
        );
        t.transport
            .state
            .content_limits
            .as_mut()
            .unwrap()
            .max_control_records = 0;
        assert_eq!(
            complete(&mut t, failed, 1, 30),
            Err(ShellTransportError::ContentQueueSaturated)
        );
        assert!(t.transport.state.output.front().is_none());
        t.transport
            .state
            .content_limits
            .as_mut()
            .unwrap()
            .max_control_records = 64;
        complete(&mut t, failed, 1, 50).unwrap();
        let ShellFrameRecord::Outcome {
            outcome,
            credit_expires_msec,
            ..
        } = t.take_record()
        else {
            panic!()
        };
        assert_eq!(outcome.kind, if failed { 3 } else { 2 });
        assert_eq!(
            credit_expires_msec,
            50 + u64::from(ContentLimits::prototype(fixture::grant()).permit_timeout_ms)
        );
        assert_eq!(
            complete(&mut t, failed, 1, 51),
            Err(ShellTransportError::ContentCandidate(
                ContentCandidateError::Stale
            ))
        );
        assert!(t.transport.state.output.front().is_none());
        assert_eq!(
            t.candidates()
                .begin(fixture::tx(90), fixture::begin(2, 1), 52),
            Ok(())
        );
        assert_eq!(
            t.candidates()
                .begin(fixture::tx(91), fixture::begin(3, 1), 53),
            Err(ContentCandidateError::Stale)
        );
    }
}

#[test]
fn timed_completion_after_close_or_clock_fault_settles_without_replenishing() {
    let mut t = submitted();
    t.candidates()
        .close_frame_stream(fixture::tx(80), fixture::key(1), 20)
        .unwrap();
    complete(&mut t, false, 1, 30).unwrap();
    assert!(matches!(t.take_record(), ShellFrameRecord::Closed { .. }));
    assert!(
        matches!(t.take_record(), ShellFrameRecord::Outcome { outcome, credit_expires_msec: 0, .. } if outcome.kind == 2)
    );
    let mut t = submitted();
    assert_eq!(
        complete(&mut t, true, 1, 0),
        Err(ShellTransportError::ContentCandidate(
            ContentCandidateError::ClockRegression
        ))
    );
    assert!(t.transport.state.output.front().is_none());
    complete(&mut t, true, 1, 50).unwrap();
    assert!(
        matches!(t.take_record(), ShellFrameRecord::Closed { reason, .. }
        if reason == sophia_protocol::ContentReason::Malformed as u16)
    );
    assert!(
        matches!(t.take_record(), ShellFrameRecord::Outcome { outcome, credit_expires_msec: 0, .. }
        if outcome.kind == 3)
    );
}

#[test]
fn retired_completion_settles_only_its_exact_grant_without_publishing_to_successor() {
    for failed in [false, true] {
        let mut t = submitted();
        let grant = fixture::grant();
        let successor = sophia_protocol::ContentGrant {
            connection_epoch: grant.connection_epoch + 1,
            content_grant_epoch: grant.content_grant_epoch + 1,
        };
        t.transport.state.content_grant = Some(successor);
        assert_eq!(
            complete(&mut t, failed, 1, 50),
            Err(ShellTransportError::WrongContentGrant)
        );
        assert!(t.transport.state.output.front().is_none());
        t.transport.state.content_grant = Some(grant);
        assert!(t.transport.content_epochs.disconnect(grant));
        t.transport
            .content_epochs
            .admit(ContentLimits::prototype(successor))
            .unwrap();
        t.transport.state.store_grant = successor;
        t.transport.state.content_grant = Some(successor);
        let before = t.transport.content_epochs.control_occupancy(successor);
        complete(&mut t, failed, 1, 50).unwrap();
        assert_eq!(
            t.transport.content_epochs.control_occupancy(successor),
            before
        );
        assert!(t.transport.state.output.front().is_none());
        assert!(
            t.transport.content_epochs.candidates_mut(grant).is_none(),
            "settled retired epoch collected"
        );
    }
}

#[test]
fn renderer_failure_before_prepared_returns_one_terminal_and_releases_both_reservations() {
    let mut f = fixture::Fixture::new(64);
    f.open_with_credits(1, 1).unwrap();
    f.frame(1, 1, None).unwrap();
    let now = f.tick();
    let _display = f
        .candidates()
        .begin_submission(fixture::output(), 1, now)
        .unwrap();
    let mut t = Transport::take(&mut f);
    t.flush(now).unwrap();
    assert!(matches!(t.take_record(), ShellFrameRecord::Opened { .. }));
    let before = t.candidates().control_occupancy();
    complete(&mut t, true, 1, 50).unwrap();
    let ShellFrameRecord::Outcome {
        outcome,
        credit_expires_msec,
        ..
    } = t.take_record()
    else {
        panic!()
    };
    assert_eq!(outcome.kind, 3);
    assert_eq!(
        credit_expires_msec,
        50 + u64::from(ContentLimits::prototype(fixture::grant()).permit_timeout_ms)
    );
    assert_eq!(t.candidates().control_occupancy() + 2, before);
    assert!(t.transport.state.output.front().is_none());
}
