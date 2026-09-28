//! Every legacy scalar WM wrapper, over each cause and outcome, framed. The
//! bytes were captured at f5235c04, before the scalar semantics moved to their
//! neutral owner, and must not change. IPC-only: it retires with the socket
//! wire (t269). Includers name the neutral values module `fixture`.
#![allow(dead_code)]

use super::fixture::*;
use sophia_protocol::*;

pub fn legacy_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for cause in ordinary_causes() {
        let wire = encode_wm_v1_policy_projection_request(&request(cause)).unwrap();
        bytes.extend(encode_wm_v1_projection_request_frame(TRANSACTION, &wire).unwrap());
    }
    let wire = encode_wm_output_action_request(&output_action()).unwrap();
    bytes.extend(encode_wm_v1_output_action_request_frame(TRANSACTION, &wire).unwrap());
    for target in [false, true] {
        let wire = encode_wm_presentation_action_request(&presentation_action(target)).unwrap();
        bytes.extend(encode_wm_v1_presentation_action_request_frame(TRANSACTION, &wire).unwrap());
    }
    for outcome in OUTCOMES {
        let wire = encode_wm_v1_policy_projection_outcome(3, 17, 11, outcome).unwrap();
        bytes.extend(encode_wm_v1_projection_outcome_frame(TRANSACTION, &wire).unwrap());
        let wire = encode_wm_v1_policy_session_operation_outcome(PolicySessionOperationOutcome {
            connection_epoch: 3,
            request_id: 47,
            outcome,
        })
        .unwrap();
        bytes.extend(encode_wm_v1_session_operation_outcome_frame(TRANSACTION, &wire).unwrap());
    }
    let wire = encode_wm_v1_policy_dirty(&dirty()).unwrap();
    bytes.extend(encode_wm_v1_policy_dirty_frame(TRANSACTION, &wire).unwrap());
    for target in session_targets() {
        let wire =
            encode_wm_v1_policy_session_operation_request(session_operation(target)).unwrap();
        bytes.extend(encode_wm_v1_session_operation_request_frame(TRANSACTION, &wire).unwrap());
    }
    for outcome in PRESENTATION_OUTCOMES {
        let wire = encode_wm_presentation_receipt(receipt(outcome)).unwrap();
        bytes.extend(encode_wm_v1_presentation_outcome_frame(TRANSACTION, &wire).unwrap());
    }
    bytes
}
