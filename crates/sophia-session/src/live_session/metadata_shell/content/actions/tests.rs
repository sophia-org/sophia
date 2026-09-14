#![cfg(test)]

use super::*;
use sophia_protocol::{
    ContentAllocationId, ContentGrant, ContentLogicalRect, ContentOutputId, ContentPixelRect,
    OutputId,
};

fn target() -> PresentedContentTarget {
    PresentedContentTarget {
        grant: ContentGrant {
            connection_epoch: 3,
            content_grant_epoch: 4,
        },
        output: ContentOutputId {
            id: 5,
            generation: 6,
        },
        candidate_generation: 7,
        presentation_epoch: 8,
        interaction_generation: 1,
        allocation: ContentAllocationId {
            id: 9,
            generation: 10,
        },
        allocation_logical: ContentLogicalRect {
            x: 0,
            y: 0,
            width: 32,
            height: 32,
        },
        allocation_pixel: ContentPixelRect {
            x: 0,
            y: 0,
            width: 32,
            height: 32,
        },
        target_id: 11,
        target_generation: 12,
        action_id: 13,
        bounds_px: ContentPixelRect {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        },
    }
}

fn activation(event_id: u64) -> ShellIndicatorActivation {
    ShellIndicatorActivation {
        connection_epoch: 3,
        snapshot_generation: 7,
        output: OutputId::from_raw(5),
        indicator: 11,
        action: 13,
        event_id,
    }
}

fn ledger(ack: AckState) -> ContentActionLedger {
    let target = target();
    let action = action_from_target(&target, 14, ACTION_ACTIVATE);
    ContentActionLedger {
        next_event_id: 15,
        issued_high_water: 14,
        live: [(
            14,
            PendingAction {
                action,
                target,
                deadline_msec: 100,
                ack,
                activation: ActivationState::Awaiting,
                cancel_sent: false,
            },
        )]
        .into_iter()
        .collect(),
    }
}

#[test]
fn indicator_activation_requires_the_exact_consumed_action_ack() {
    assert_eq!(
        ledger(AckState::Awaiting).indicator_admission(&activation(14), 50),
        LinkedIndicatorAdmission::Stale
    );
    assert_eq!(
        ledger(AckState::Consumed).indicator_admission(&activation(14), 50),
        LinkedIndicatorAdmission::Eligible
    );
}

#[test]
fn wm_admission_consumes_the_one_use_activation_identity() {
    let mut ledger = ledger(AckState::Consumed);
    assert_eq!(
        ledger.indicator_admission(&activation(14), 50),
        LinkedIndicatorAdmission::Eligible
    );
    ledger.wm_admitted(14, 50);
    assert_eq!(
        ledger.indicator_admission(&activation(14), 50),
        LinkedIndicatorAdmission::Stale
    );
}
