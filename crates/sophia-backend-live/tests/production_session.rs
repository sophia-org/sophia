#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
use sophia_backend_live::{
    LiveProductionCompletionTimestamp, LiveProductionKmsCompletionSource,
    reduce_live_production_completion_timestamp,
};
use sophia_backend_live::{
    LiveProductionOutputRuntimeAdapter, LiveProductionPageFlipRetirement,
    LiveProductionPageFlipTracker, LiveProductionPageFlipTrackerError,
    LiveProductionPresentationAdapter,
};
use sophia_engine::{
    EngineHeadRegistry, HeadRenderTarget, OutputPresentationFeedback, OutputPresentationSchedule,
    ProductionOutputRuntimeAdapter, ProductionPresentationAdapter, ProductionRetirement,
};
use sophia_protocol::{CommittedSurfaceState, OutputId, Size, TransactionCommit};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn live_output_runtime_adapter_keeps_projection_and_invocation_in_one_callback() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let observed = Rc::clone(&calls);
    let mut adapter = LiveProductionOutputRuntimeAdapter::new(
        2,
        move |index, committed: &[CommittedSurfaceState]| {
            observed.borrow_mut().push((index, committed.len()));
            Ok::<_, String>(index + committed.len())
        },
    );

    assert_eq!(adapter.output_count(), 2);
    assert_eq!(adapter.run_output(0, &[]).unwrap(), 0);
    assert_eq!(adapter.run_output(1, &[]).unwrap(), 1);
    assert_eq!(*calls.borrow(), [(0, 0), (1, 0)]);
}

#[test]
fn live_adapter_keeps_frame_and_retirement_inside_ordered_callbacks() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let compose_calls = Rc::clone(&calls);
    let submit_calls = Rc::clone(&calls);
    let feedback_calls = Rc::clone(&calls);
    let retire_calls = Rc::clone(&calls);
    let pending = Rc::new(RefCell::new(Vec::new()));
    let submitted = Rc::clone(&pending);
    let retired = Rc::clone(&pending);
    let mut adapter = LiveProductionPresentationAdapter::new(
        move |cycle,
              committed: &[CommittedSurfaceState],
              _authority_commits: &[TransactionCommit]| {
            compose_calls.borrow_mut().push(("compose", cycle));
            Ok::<_, &str>(committed.len())
        },
        move |cycle, frame| {
            submit_calls.borrow_mut().push(("submit", cycle));
            submitted.borrow_mut().push(ProductionRetirement {
                cycle,
                retirement: frame + 1,
            });
            Ok::<_, &str>(frame)
        },
        move || {
            retire_calls.borrow_mut().push(("retire", 7));
            Ok::<_, &str>(retired.borrow_mut().drain(..).collect())
        },
        move |cycle, retirement| {
            feedback_calls.borrow_mut().push(("feedback", cycle));
            Ok::<_, &str>(retirement + 1)
        },
    );

    let frame = adapter.compose(7, &[], &[]).unwrap();
    let submission = adapter.submit_frame(7, frame).unwrap();
    let retirement = adapter.poll_retirements().unwrap().pop().unwrap();
    let evidence = adapter
        .route_protocol_feedback(retirement.cycle, retirement.retirement)
        .unwrap();

    assert_eq!(submission, 0);
    assert_eq!(evidence, 2);
    assert_eq!(
        *calls.borrow(),
        [
            ("compose", 7),
            ("submit", 7),
            ("retire", 7),
            ("feedback", 7),
        ]
    );
}

fn production_outputs() -> EngineHeadRegistry {
    let mut outputs = EngineHeadRegistry::new();
    assert!(
        outputs
            .admit(HeadRenderTarget {
                head: sophia_engine::RenderHeadId::from_raw(1),
                output: OutputId::from_raw(7),
                target_generation: 1,
                native_size: Size {
                    width: 1920,
                    height: 1080,
                },
                scale: 1,
                refresh_millihz: 60_000,
                transform: sophia_protocol::OutputTransform::Normal,
                mapping: sophia_protocol::OutputHeadMapping::Fit,
            })
            .is_admitted()
    );
    outputs
}

#[test]
fn page_flip_tracker_emits_only_matching_retirements_with_origin_cycle() {
    let output = OutputId::from_raw(7);
    let mut tracker = LiveProductionPageFlipTracker::from_outputs(&production_outputs());

    let frame = tracker.submit(output, 41).unwrap();
    assert_eq!(frame, 1);
    assert!(tracker.drain_retirements().is_empty());

    tracker.observe_page_flip(output, 99, 12_345).unwrap();
    assert_eq!(
        tracker.drain_retirements(),
        [ProductionRetirement {
            cycle: 41,
            retirement: LiveProductionPageFlipRetirement {
                output,
                ust: 12_345,
                msc: 99,
            },
        }]
    );
}

#[test]
fn page_flip_tracker_fails_closed_for_overlap_and_non_monotonic_feedback() {
    let output = OutputId::from_raw(7);
    let mut tracker = LiveProductionPageFlipTracker::from_outputs(&production_outputs());

    let _ = tracker.submit(output, 1).unwrap();
    assert!(matches!(
        tracker.submit(output, 2),
        Err(LiveProductionPageFlipTrackerError::Schedule(
            OutputPresentationSchedule::WaitingForRetirement { .. }
        ))
    ));
    tracker.observe_page_flip(output, 10, 5_000).unwrap();
    let _ = tracker.drain_retirements();

    let _ = tracker.submit(output, 3).unwrap();
    assert!(matches!(
        tracker.observe_page_flip(output, 10, 6_000),
        Err(LiveProductionPageFlipTrackerError::Feedback(
            OutputPresentationFeedback::NonMonotonicSequence { .. }
        ))
    ));
    assert!(tracker.drain_retirements().is_empty());
    assert_eq!(tracker.submit(output, 4), Ok(3));
}

#[test]
fn page_flip_tracker_accepts_source_changes_in_one_monotonic_clock_domain() {
    let output = OutputId::from_raw(7);
    let mut tracker = LiveProductionPageFlipTracker::from_outputs(&production_outputs());
    let kernel_ust = 98_765_432_100;

    let _ = tracker.submit(output, 1).unwrap();
    tracker.observe_page_flip(output, 10, kernel_ust).unwrap();
    let _ = tracker.drain_retirements();

    let _ = tracker.submit(output, 2).unwrap();
    tracker
        .observe_page_flip(output, 11, kernel_ust + 16_667)
        .unwrap();
    assert_eq!(tracker.drain_retirements().len(), 1);
    assert_eq!(tracker.submit(output, 3), Ok(3));
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
#[test]
fn completion_sources_share_one_monotonic_timestamp_domain() {
    let kernel_ust = 604_000_000_000;
    let out_fence_ust = kernel_ust + 16_000;
    let missing_kernel_ust = out_fence_ust + 16_000;

    let page_flip = reduce_live_production_completion_timestamp(
        LiveProductionKmsCompletionSource::PageFlipEvent,
        Some(kernel_ust),
        1,
    );
    let out_fence = reduce_live_production_completion_timestamp(
        LiveProductionKmsCompletionSource::OutFence,
        None,
        out_fence_ust,
    );
    let missing_kernel = reduce_live_production_completion_timestamp(
        LiveProductionKmsCompletionSource::PageFlipEvent,
        None,
        missing_kernel_ust,
    );

    assert_eq!(
        page_flip,
        LiveProductionCompletionTimestamp {
            clock: None,
            ust_usec: kernel_ust,
            used_kernel_timestamp: true,
            missing_kernel_timestamp: false,
        }
    );
    assert_eq!(
        out_fence,
        LiveProductionCompletionTimestamp {
            clock: None,
            ust_usec: out_fence_ust,
            used_kernel_timestamp: false,
            missing_kernel_timestamp: false,
        }
    );
    assert_eq!(
        missing_kernel,
        LiveProductionCompletionTimestamp {
            clock: None,
            ust_usec: missing_kernel_ust,
            used_kernel_timestamp: false,
            missing_kernel_timestamp: true,
        }
    );
    assert!(page_flip.ust_usec < out_fence.ust_usec);
    assert!(out_fence.ust_usec < missing_kernel.ust_usec);
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
#[test]
fn out_fence_completion_ignores_a_stale_kernel_timestamp() {
    let timestamp = reduce_live_production_completion_timestamp(
        LiveProductionKmsCompletionSource::OutFence,
        Some(604_000_000_000),
        604_000_016_000,
    );

    assert_eq!(timestamp.ust_usec, 604_000_016_000);
    assert!(!timestamp.used_kernel_timestamp);
    assert!(!timestamp.missing_kernel_timestamp);
}

#[cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]
#[test]
fn mixed_diagnostic_completion_has_stable_reduced_schema() {
    let report = sophia_backend_live::LiveNativeMixedDiagnosticComplete {
        status: sophia_backend_live::LiveRendererScanoutBufferExportStatus::Exported,
        detail: sophia_backend_live::LiveRendererScanoutBufferExportDetail::Exported,
        cpu_layers: 1,
        dmabuf_layers: 1,
        live_sources: 0,
        live_fences: 0,
        live_transactions: 0,
    };

    assert_eq!(
        report.reduced_log_line("completed"),
        "sophia_native_egl_mixed schema=1 case=mixed status=Exported stage=Exported cpu_layers=1 dmabuf_layers=1 child_outcome=completed live_sources=0 live_fences=0 live_transactions=0"
    );
}
