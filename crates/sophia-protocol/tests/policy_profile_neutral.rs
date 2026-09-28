//! The neutral profile values without the socket codecs: exact identities,
//! outcome codes, and the established validation errors in their precedence.
//! The socket frames and legacy aliases are in `policy_profile_ipc_compat.rs`.
use sophia_protocol::wm_rows::{
    SOPHIA_WM_OUTCOME_PROFILE_ACCEPTED, SOPHIA_WM_OUTCOME_PROFILE_REJECTED_IDENTITY,
    SOPHIA_WM_OUTCOME_PROFILE_REJECTED_STATE,
};
use sophia_protocol::*;

#[test]
fn neutral_profile_identity_keeps_exact_values_and_outcome_codes() {
    assert_eq!(POLICY_PROFILE_DIGEST_BYTES, 32);
    let mut digest = [0; POLICY_PROFILE_DIGEST_BYTES];
    digest[31] = 1;
    let identity = PolicyProfileIdentity::new(9, 7, digest).unwrap();
    assert_eq!(
        identity,
        PolicyProfileIdentity {
            connection_epoch: 9,
            profile_generation: 7,
            profile_digest: digest,
        }
    );
    for (code, outcome) in [
        (
            SOPHIA_WM_OUTCOME_PROFILE_ACCEPTED,
            PolicyProfileOutcome::Accepted,
        ),
        (
            SOPHIA_WM_OUTCOME_PROFILE_REJECTED_IDENTITY,
            PolicyProfileOutcome::RejectedIdentity,
        ),
        (
            SOPHIA_WM_OUTCOME_PROFILE_REJECTED_STATE,
            PolicyProfileOutcome::RejectedState,
        ),
    ] {
        assert_eq!(PolicyProfileOutcome::try_from(code), Ok(outcome));
        assert_eq!(outcome as u16, code);
    }
}

#[test]
fn neutral_profile_validation_keeps_the_established_errors_and_precedence() {
    // Each case is valid in every earlier field, so the first invalid field
    // named is also the first one checked.
    for (epoch, generation, digest, field) in [
        (0, 0, [0; 32], "connection_epoch"),
        (9, 0, [0; 32], "profile_generation"),
        (9, 7, [0; 32], "profile_digest"),
    ] {
        assert_eq!(
            PolicyProfileIdentity::new(epoch, generation, digest),
            Err(BinaryCodecError::InvalidProfileIdentity(field))
        );
    }
    for value in [0, 4, u16::MAX] {
        assert_eq!(
            PolicyProfileOutcome::try_from(value),
            Err(BinaryCodecError::InvalidEnum {
                field: "profile_outcome",
                value: u32::from(value),
            })
        );
    }
}
