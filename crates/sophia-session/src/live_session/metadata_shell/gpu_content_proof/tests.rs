#![cfg(test)]

use super::*;

#[test]
fn production_lom_proof_admits_the_discrete_input_contract() {
    assert!(matches!(
        proof_content_admission_policy(),
        ShellContentAdmissionPolicy::Granted {
            discrete_input: true
        }
    ));
}
