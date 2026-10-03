#![cfg(all(feature = "libdrm-events", feature = "gbm-probe"))]

use sophia_backend_live::{
    LivePresentBufferDisposition, LivePresentFeedbackOutcome, LivePresentProtocolFeedback,
    LiveProductionVisualRuntime,
};
use sophia_engine::HeadlessOutput;
use sophia_protocol::{OutputId, Size, TransactionId};

fn feedback(transaction: u64) -> LivePresentFeedbackOutcome {
    let transaction = TransactionId::from_raw(transaction);
    LivePresentFeedbackOutcome {
        feedback: vec![
            LivePresentProtocolFeedback::Complete {
                transaction,
                ust: transaction.raw(),
                msc: transaction.raw(),
                disposition: LivePresentBufferDisposition::Retained,
            },
            LivePresentProtocolFeedback::Idle { transaction },
        ],
        idle_fence_triggered: false,
        layout_comparison: None,
        clocks: Default::default(),
    }
}

#[test]
fn visual_runtime_drains_owned_present_feedback_in_order() {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 640,
            height: 480,
        },
        scale: 1,
    };
    let mut runtime = LiveProductionVisualRuntime::new(&[output], None).expect("runtime");
    runtime.route_present_feedback(feedback(1));
    runtime.route_present_feedback(feedback(2));

    let mut outcomes = Vec::new();
    runtime
        .drain_present_feedback_into(&mut outcomes)
        .expect("bounded feedback queue");

    assert_eq!(outcomes, [feedback(1), feedback(2)]);
    outcomes.clear();
    runtime
        .drain_present_feedback_into(&mut outcomes)
        .expect("drained feedback queue remains usable");
    assert!(outcomes.is_empty());
}
