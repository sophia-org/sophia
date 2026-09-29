//! Shortcut/reference semantics retained from the socket codec tests. File
//! objects carry one whole catalog; per-frame phase/order checks retired.
use sophia_protocol::shell_files::*;
use sophia_protocol::*;
#[path = "support/descriptor_file.rs"]
mod file;
#[path = "support/reference_fixture.rs"]
mod fixture;

#[test]
fn maximum_catalog_and_reference_round_trip_without_descriptor_limit() {
    file::round_trip(ShellDescriptorRecord::Shortcuts(fixture::catalog(256)));
    file::round_trip(ShellDescriptorRecord::ReferenceCandidate(
        fixture::candidate(256),
    ));
    for operation in [
        ShellReferenceOperation::Startup,
        ShellReferenceOperation::Toggle,
        ShellReferenceOperation::Next,
        ShellReferenceOperation::Previous,
        ShellReferenceOperation::Dismiss,
    ] {
        file::round_trip(ShellDescriptorRecord::ReferenceRequest(
            ShellReferenceRequest {
                connection_epoch: 5,
                catalog_generation: 7,
                request_generation: 8,
                output: OutputId::from_raw(1),
                output_generation: 2,
                presentation_epoch: 10,
                operation,
            },
        ));
    }
    for kind in [
        ShellV1CandidateOutcomeKind::Prepared,
        ShellV1CandidateOutcomeKind::Presented,
    ] {
        file::round_trip(ShellDescriptorRecord::ReferenceOutcome(
            ShellReferenceOutcome {
                connection_epoch: 5,
                catalog_generation: 7,
                request_generation: 8,
                candidate_generation: 9,
                presentation_epoch: 10,
                page: 2,
                pages: 5,
                kind,
            },
        ));
    }
}

#[test]
fn malformed_candidates_and_oversized_catalogs_fail_closed() {
    let mut c = fixture::candidate(2);
    c.entries[1].slot = c.entries[0].slot;
    assert!(file::encode(ShellDescriptorRecord::ReferenceCandidate(c)).is_err());
    let mut c = fixture::candidate(1);
    c.entries[0].label = "a\u{202e}b".into();
    assert!(file::encode(ShellDescriptorRecord::ReferenceCandidate(c)).is_err());
    let mut c = fixture::candidate(1);
    c.style.columns = 0;
    assert!(file::encode(ShellDescriptorRecord::ReferenceCandidate(c)).is_err());
    assert!(file::encode(ShellDescriptorRecord::Shortcuts(fixture::catalog(257))).is_err());
    assert!(
        file::encode(ShellDescriptorRecord::ReferenceCandidate(
            fixture::candidate(257)
        ))
        .is_err()
    );
}
