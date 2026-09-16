#![cfg(test)]

use super::*;
use sophia_protocol::{
    ContentAllocationId, ContentGrant, ContentLogicalRect, ContentOutputId, ContentPixelRect,
    OutputId,
};

pub(super) fn target() -> PresentedContentTarget {
    PresentedContentTarget {
        continuity: sophia_engine::ContentTargetContinuity::mint(),
        scale_generation: 1,
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
        live: [PendingAction {
            action,
            target,
            deadline_msec: 100,
            ack,
            activation: ActivationState::Awaiting,
            cancel_sent: false,
        }]
        .into_iter()
        .collect(),
    }
}

#[test]
fn indicator_activation_does_not_wait_for_action_ack() {
    assert_eq!(
        ledger(AckState::Awaiting).indicator_admission(&activation(14), 50),
        LinkedIndicatorAdmission::Eligible
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

fn acknowledgement(disposition: u16) -> ContentActionAck {
    let action = action_from_target(&target(), 14, ACTION_ACTIVATE);
    ContentActionAck {
        grant: action.grant,
        output: action.output,
        candidate_generation: action.candidate_generation,
        presentation_epoch: action.presentation_epoch,
        interaction_generation: action.interaction_generation,
        allocation: action.allocation,
        target_id: action.target_id,
        target_generation: action.target_generation,
        action_id: action.action_id,
        event_id: action.event_id,
        disposition,
    }
}

#[test]
fn mismatched_ack_cannot_mutate_the_event_found_by_number() {
    let mut ledger = ledger(AckState::Awaiting);
    let mut wrong = acknowledgement(ACK_REJECTED_STALE);
    wrong.presentation_epoch += 1;
    ledger.acknowledge(&wrong, 50).unwrap();
    assert_eq!(ledger.live[0].ack, AckState::Awaiting);
    assert_eq!(ledger.live[0].activation, ActivationState::Awaiting);
    assert_eq!(
        ledger.indicator_admission(&activation(14), 50),
        LinkedIndicatorAdmission::Eligible
    );
    ledger
        .acknowledge(&acknowledgement(ACK_CONSUMED), 50)
        .unwrap();
    assert_eq!(ledger.live[0].ack, AckState::Consumed);
}

#[test]
fn rejected_or_late_ack_cannot_undo_wm_admission() {
    for (disposition, now) in [(ACK_REJECTED_STALE, 50), (ACK_CONSUMED, 101)] {
        let mut ledger = ledger(AckState::Awaiting);
        ledger.wm_admitted(14, 40);
        ledger
            .acknowledge(&acknowledgement(disposition), now)
            .unwrap();
        assert_eq!(ledger.live[0].activation, ActivationState::WmAdmitted);
        assert_eq!(
            ledger.indicator_admission(&activation(14), 50),
            LinkedIndicatorAdmission::Stale
        );
    }
}

#[test]
fn ack_outcome_does_not_replace_the_wm_admission_decision() {
    let mut ledger = ledger(AckState::Awaiting);
    ledger
        .acknowledge(&acknowledgement(ACK_REJECTED_STALE), 50)
        .unwrap();
    ledger.collect_terminal(50);
    assert_eq!(
        ledger.indicator_admission(&activation(14), 50),
        LinkedIndicatorAdmission::Eligible
    );
    ledger.wm_admitted(14, 51);
    assert!(ledger.live.is_empty());
}

#[test]
fn a_later_rejection_cannot_retract_an_already_admitted_wm_effect() {
    let mut ledger = ledger(AckState::Awaiting);
    ledger.wm_admitted(14, 40);
    ledger.wm_rejected(14, 50);
    assert_eq!(ledger.live[0].activation, ActivationState::WmAdmitted);
    assert_eq!(ledger.live[0].ack, AckState::Awaiting);
    assert_eq!(
        ledger.indicator_admission(&activation(14), 50),
        LinkedIndicatorAdmission::Stale
    );
    ledger
        .acknowledge(&acknowledgement(ACK_REJECTED_STALE), 51)
        .unwrap();
    assert_eq!(ledger.live[0].activation, ActivationState::WmAdmitted);
}

#[test]
fn expiry_keeps_unqueued_cancellation_and_selects_the_same_exact_owner() {
    let mut ledger = ledger(AckState::Awaiting);
    ledger.expire(100);
    assert_eq!(ledger.live.len(), 1);
    assert_eq!(ledger.live[0].activation, ActivationState::Rejected);
    for now in [100, 101, 1000] {
        // This models no enqueue, including a returned enqueue refusal. The
        // transport's refusal/credit behavior is covered by its FIFO tests.
        assert_eq!(ledger.next_cancellation(&[], now), Some(0));
        assert_eq!(ledger.live[0].action.event_id, 14);
        assert!(!ledger.live[0].cancel_sent);
    }
    ledger.cancellation_queued(0);
    assert_eq!(ledger.next_cancellation(&[], 1001), None);
    assert!(ledger.live.is_empty(), "Cancel never waits for its own ACK");
}

#[test]
fn stale_target_cancellation_is_not_lost_to_terminal_collection() {
    let mut ledger = ledger(AckState::Awaiting);
    assert_eq!(ledger.next_cancellation(&[], 50), Some(0));
    ledger.collect_terminal(100);
    assert_eq!(ledger.live.len(), 1);
    assert_eq!(ledger.next_cancellation(&[], 101), Some(0));
}

#[test]
fn timeout_cancellation_does_not_retract_an_irreversible_wm_effect() {
    let mut ledger = ledger(AckState::Awaiting);
    ledger.wm_admitted(14, 40);
    assert_eq!(ledger.next_cancellation(&[], 50), None);
    assert_eq!(ledger.next_cancellation(&[], 100), Some(0));
    ledger.cancellation_queued(0);
    assert_eq!(ledger.live[0].activation, ActivationState::WmAdmitted);
    assert_eq!(
        ledger.indicator_admission(&activation(14), 101),
        LinkedIndicatorAdmission::Stale
    );
    ledger.collect_terminal(101);
    assert!(ledger.live.is_empty());
}
