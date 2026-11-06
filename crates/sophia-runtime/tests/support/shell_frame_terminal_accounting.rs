//! Production terminal-only preparation still owns the complete lifecycle.
use super::*;

fn submitted() -> (Transport, crate::ContentRenderBundle, u64) {
    let mut f = fixture::Fixture::new(3);
    f.open_with_credits(1, 1).unwrap();
    f.candidates().take_frame_stream_event().unwrap();
    f.frame(1, 1, None).unwrap();
    let now = f.tick();
    let bundle = f
        .candidates()
        .begin_submission(fixture::output(), 1, now)
        .unwrap();
    assert_eq!(f.candidates().control_occupancy(), 3);
    let mut t = Transport::take(&mut f);
    t.transport.state.capabilities |= SHELL_FRAME_TERMINAL_ONLY_CAPABILITY;
    (t, bundle, now + 1)
}

fn prepare(t: &mut Transport, now: u64) -> Result<(), ShellTransportError> {
    t.transport.state.content_prepared(
        &mut t.transport.content_epochs,
        fixture::grant(),
        fixture::output(),
        1,
        20,
        30,
        now,
    )
}

#[test]
fn local_preparation_refunds_one_slot_even_when_output_is_full() {
    let (mut t, _bundle, now) = submitted();
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 0;
    assert!(
        t.candidates()
            .presented_at(fixture::output(), 1, 40, 20, 30, now)
            .is_err()
    );
    prepare(&mut t, now).unwrap();
    assert_eq!(t.candidates().control_occupancy(), 2);
    assert_eq!(t.transport.state.output.records(), 0);
    assert!(prepare(&mut t, now).is_err());
    assert_eq!(t.candidates().control_occupancy(), 2);
    assert_eq!(
        t.candidates()
            .begin(fixture::tx(99), fixture::begin(2, 1), now),
        Err(ContentCandidateError::Budget)
    );
    t.candidates()
        .presented_at(fixture::output(), 1, 40, 20, 30, now + 1)
        .unwrap();
    let head = t.candidates().pending_event().cloned().unwrap();
    assert_eq!(
        t.flush(now + 1),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert_eq!(t.candidates().pending_event(), Some(&head));
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 64;
    t.flush(now + 2).unwrap();
    assert!(
        matches!(t.take_record(), ShellFrameRecord::Outcome { outcome, credit_expires_msec, .. } if outcome.kind == 2 && credit_expires_msec > now + 2)
    );
    t.flush(now + 3).unwrap();
    assert_eq!(t.transport.state.output.records(), 0);
}

#[test]
fn failure_and_revoke_settle_before_or_after_local_preparation() {
    for prepared in [false, true] {
        for revoked in [false, true] {
            let (mut t, _bundle, now) = submitted();
            if prepared {
                prepare(&mut t, now).unwrap();
            }
            if revoked {
                t.candidates().revoke();
            }
            t.candidates()
                .renderer_failed_at(fixture::output(), 1, now + 1)
                .unwrap();
            if revoked {
                assert_eq!(t.candidates().control_occupancy(), 0);
                assert!(t.candidates().pending_event().is_none());
            } else {
                assert_eq!(t.candidates().control_occupancy(), 2);
                t.flush(now + 2).unwrap();
                assert!(
                    matches!(t.take_record(), ShellFrameRecord::Outcome { outcome, .. } if outcome.kind == 3)
                );
                t.candidates()
                    .close_frame_stream(fixture::tx(70), fixture::key(1), now + 3)
                    .unwrap();
                t.flush(now + 3).unwrap();
                assert!(matches!(t.take_record(), ShellFrameRecord::Closed { .. }));
                assert_eq!(t.candidates().control_occupancy(), 0);
            }
        }
    }
}

#[test]
fn elapsed_time_does_not_release_submitted_physical_work() {
    let (mut t, _bundle, now) = submitted();
    let timeout = ContentLimits::prototype(fixture::grant()).presentation_timeout_ms as u64;
    prepare(&mut t, now).unwrap();
    t.candidates().expire(now + timeout).unwrap();
    assert!(t.candidates().pending_event().is_none());
    assert_eq!(t.candidates().control_occupancy(), 2);
    // Submitted work is non-cancellable; the native owner must settle it.
    t.candidates()
        .renderer_failed_at(fixture::output(), 1, now + timeout)
        .unwrap();
    t.flush(now + timeout).unwrap();
    assert!(
        matches!(t.take_record(), ShellFrameRecord::Outcome { outcome, .. } if outcome.kind == 3)
    );
}

#[test]
fn other_responses_keep_fifo_progress_before_local_preparation() {
    let (mut t, _bundle, now) = submitted();
    t.candidates()
        .close_frame_stream(fixture::tx(70), fixture::key(1), now)
        .unwrap();
    let occupied = t.candidates().control_occupancy();
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 0;
    assert_eq!(
        prepare(&mut t, now),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert_eq!(t.candidates().control_occupancy(), occupied);
    t.transport
        .state
        .content_limits
        .as_mut()
        .unwrap()
        .max_control_records = 64;
    prepare(&mut t, now).unwrap();
    assert!(matches!(t.take_record(), ShellFrameRecord::Closed { .. }));
    assert_eq!(t.transport.state.output.records(), 0);
    t.candidates()
        .renderer_failed_at(fixture::output(), 1, now + 1)
        .unwrap();
    t.flush(now + 1).unwrap();
    assert!(
        matches!(t.take_record(), ShellFrameRecord::Outcome { outcome, credit_expires_msec: 0, .. } if outcome.kind == 3)
    );
    assert_eq!(t.candidates().control_occupancy(), 0);
}
