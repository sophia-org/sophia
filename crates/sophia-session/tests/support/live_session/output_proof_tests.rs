use super::super::output_proof::{
    validate_output_proof_rollback_after_apply, validate_prepared_output_proof_candidate,
};
use std::time::Duration;

#[test]
fn post_apply_output_rollback_proof_fails_closed() {
    let bounded = Some(Duration::from_secs(1));
    for requirements in [
        (false, true, true, bounded, true, false),
        (true, false, true, bounded, true, false),
        (false, false, false, bounded, true, false),
        (true, true, true, None, true, false),
        (true, true, true, bounded, false, false),
        (true, true, true, bounded, true, true),
    ] {
        let (native, normal, wm_process, runtime, armed, other_proof) = requirements;
        assert!(
            validate_output_proof_rollback_after_apply(
                true,
                native,
                normal,
                wm_process,
                runtime,
                armed,
                other_proof,
            )
            .is_err()
        );
    }
    assert!(
        validate_output_proof_rollback_after_apply(true, true, true, true, bounded, true, false,)
            .is_ok()
    );
    assert!(
        validate_output_proof_rollback_after_apply(false, false, false, false, None, false, true,)
            .is_ok()
    );
    assert!(validate_prepared_output_proof_candidate(true, false).is_err());
    assert!(validate_prepared_output_proof_candidate(true, true).is_ok());
    assert!(validate_prepared_output_proof_candidate(false, false).is_ok());
}
