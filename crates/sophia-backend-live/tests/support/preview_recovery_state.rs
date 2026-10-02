use super::*;
fn failure(now: Instant) -> LivePreviewFrameFailure {
    LivePreviewFrameFailure {
        output: OutputId::from_raw(2),
        frame: LiveProductionNativeFrameId::from_raw(10),
        owner_epoch: 7,
        publication: 8,
        source: sophia_protocol::SurfaceId::new(9, 1),
        required_retirement: true,
        detail: crate::LiveRendererScanoutBufferExportDetail::InvalidRendererImageId,
        recorded_at: now,
        recovery_started_at: None,
    }
}
#[test]
fn preview_recovery_blocks_only_its_output_and_has_a_deadline_without_worker_work() {
    let now = Instant::now();
    let f = failure(now);
    let mut failures = BTreeMap::new();
    record_failure(&mut failures, f).unwrap();
    begin_recoveries(&mut failures, now);
    assert_eq!(recovery_blocks(&failures, f.output, now), Ok(true));
    assert_eq!(
        recovery_blocks(&failures, OutputId::from_raw(1), now),
        Ok(false)
    );
    assert_eq!(
        recovery_blocks(
            &failures,
            f.output,
            now + crate::LIVE_RENDERER_WORKER_HARD_STALL - Duration::from_nanos(1)
        ),
        Ok(true)
    );
    assert_eq!(
        recovery_blocks(
            &failures,
            f.output,
            now + crate::LIVE_RENDERER_WORKER_HARD_STALL
        ),
        Err(crate::LiveRendererScanoutBufferExportDetail::WorkerStalled)
    );
}

#[test]
fn an_idle_gap_before_the_first_recovery_attempt_is_not_a_worker_stall() {
    let recorded = Instant::now();
    let f = failure(recorded);
    let mut failures = BTreeMap::new();
    record_failure(&mut failures, f).unwrap();
    let first_attempt = recorded + Duration::from_secs(5);
    assert_eq!(
        recovery_blocks(&failures, f.output, first_attempt),
        Ok(true)
    );
    let first = begin_recoveries(&mut failures, first_attempt);
    assert_eq!(first[0].check_deadline(first_attempt), Ok(()));
    let later = first_attempt + crate::LIVE_RENDERER_WORKER_HARD_STALL;
    let second = begin_recoveries(&mut failures, later);
    assert_eq!(second[0].recovery_started_at, Some(first_attempt));
    assert_eq!(
        second[0].check_deadline(later),
        Err(crate::LiveRendererScanoutBufferExportDetail::WorkerStalled)
    );
}
#[test]
fn second_preview_failure_cannot_replace_the_first_or_restart_its_deadline() {
    let first = failure(Instant::now());
    let second = LivePreviewFrameFailure {
        frame: LiveProductionNativeFrameId::from_raw(11),
        recorded_at: first.recorded_at + Duration::from_millis(100),
        ..first
    };
    let mut failures = BTreeMap::new();
    record_failure(&mut failures, first).unwrap();
    assert!(record_failure(&mut failures, second).is_err());
    assert_eq!(failures[&first.output], first);
}
#[test]
fn preview_recovery_does_not_hide_slot_or_client_import_failures() {
    use crate::LiveRendererScanoutBufferExportDetail as D;
    assert!(recoverable(D::InvalidRendererImageId));
    assert!(recoverable(D::RendererImageStoreFull));
    for detail in [
        D::RetainedBufferMissing,
        D::DmaBufImportCacheFull,
        D::InvalidTarget,
        D::WorkerStalled,
        D::EglContextUnavailable,
    ] {
        assert!(!recoverable(detail), "{detail:?}");
    }
}

#[test]
fn detach_removes_started_recovery_before_a_long_suspend_or_rollback() {
    let now = Instant::now();
    let f = failure(now);
    let mut failures = BTreeMap::new();
    record_failure(&mut failures, f).unwrap();
    begin_recoveries(&mut failures, now);
    let detached = take_detached_failures(&mut failures);
    assert_eq!(detached.len(), 1);
    assert_eq!(detached[0].frame, f.frame);
    assert_eq!(recovery_blocks(&failures, f.output, now + Duration::from_secs(30)), Ok(false));
    assert!(begin_recoveries(&mut failures, now + Duration::from_secs(30)).is_empty());
}
