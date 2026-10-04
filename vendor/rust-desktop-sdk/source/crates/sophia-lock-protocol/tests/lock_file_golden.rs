//! The lock files' golden records: one canonical record of every kind,
//! encoded by Sophia's codec. spec/golden/sophia-lock-files-v1.records is
//! Sophia's file, copied unmodified; this crate's codec must render it byte
//! for byte. It is never regenerated here: a layout change comes from Sophia
//! with the KDL.
use sophia_lock_protocol::lock_files::*;

const GOLDEN: &str = "../../spec/golden/sophia-lock-files-v1.records";
const EPOCH: u64 = 7;

fn record(kind: LockFileKind, body: &[u8]) -> Vec<u8> {
    let (submission_id, sequence) = match kind.class() {
        LockFileClass::Object => (0, 0),
        LockFileClass::Candidate => (11, 0),
        LockFileClass::Event => (0, 13),
    };
    encode_lock_file_record(
        LockFileHeader {
            kind,
            connection_epoch: EPOCH,
            submission_id,
            sequence,
        },
        body,
    )
    .unwrap()
}

fn resource() -> LockResourceId {
    LockResourceId {
        id: 21,
        generation: 2,
    }
}

fn records() -> Vec<(&'static str, Vec<u8>)> {
    use LockFileKind as K;
    let allocation = |output: u64| LockAllocation {
        output_id: output,
        output_generation: 3,
        allocation_id: 40 + output,
        allocation_generation: 4,
        pixel_width: 2560,
        pixel_height: 1440,
        scale_numerator: 3,
        scale_denominator: 2,
    };
    vec![
        (
            "limits",
            record(
                K::Limits,
                &LockFileLimits {
                    max_outputs: 16,
                    upload_slots: 2,
                    max_chords: 8,
                    max_width_px: 2560,
                    max_height_px: 1440,
                    max_resource_bytes: 2560 * 1440 * 4,
                    max_live_resources: 4,
                    journal_records: 128,
                    journal_bytes: 32_768,
                    assembly_timeout_ms: 2000,
                    ack_progress_timeout_ms: 2000,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "lock_locked",
            record(
                K::Lock,
                &LockObject {
                    lock_epoch: 5,
                    topology_generation: 9,
                    phase: LockPhase::Locked,
                    allocations: vec![allocation(1), allocation(2)],
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "lock_unlocked",
            record(
                K::Lock,
                &LockObject {
                    lock_epoch: 0,
                    topology_generation: 9,
                    phase: LockPhase::Unlocked,
                    allocations: Vec::new(),
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "negotiate",
            record(
                K::Negotiate,
                &LockNegotiate {
                    minimum_revision: 1,
                    maximum_revision: 1,
                    requested_capabilities: 3,
                    chords: vec![
                        LockChordRequest {
                            keysym: 0x62,
                            modifiers: 0b0100,
                        },
                        LockChordRequest {
                            keysym: 0xff1b,
                            modifiers: 0b1001,
                        },
                    ],
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "negotiated",
            record(
                K::Negotiated,
                &LockNegotiated {
                    granted_chords: 2,
                    granted_capabilities: 3,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "refused",
            record(K::Refused, &LockRefusal::InvalidChord.encode()),
        ),
        (
            "submitted",
            record(
                K::Submitted,
                &LockSubmitted {
                    submission_id: 11,
                    candidate_kind: K::Candidate,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "object_published",
            record(
                K::ObjectPublished,
                &LockObjectPublished {
                    object_generation: 6,
                    qid_path: 90,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "resource_begin",
            record(
                K::ResourceBegin,
                &LockResourceBegin {
                    transaction: 30,
                    resource: resource(),
                    width_px: 2560,
                    height_px: 1440,
                    slot: 1,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "resource_end",
            record(
                K::ResourceEnd,
                &LockResourceStep {
                    transaction: 31,
                    resource: resource(),
                    total_bytes: Some(2560 * 1440 * 4),
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "resource_cancel",
            record(
                K::ResourceCancel,
                &LockResourceStep {
                    transaction: 32,
                    resource: resource(),
                    total_bytes: None,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "resource_retire",
            record(
                K::ResourceRetire,
                &LockResourceStep {
                    transaction: 33,
                    resource: resource(),
                    total_bytes: None,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "resource_status",
            record(
                K::ResourceStatus,
                &LockResourceStatus {
                    transaction: 30,
                    resource: resource(),
                    status: LockResourceState::Admitted,
                    reason: 0,
                    admitted_bytes: 2560 * 1440 * 4,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "resource_released",
            record(
                K::ResourceReleased,
                &LockResourceReleased {
                    transaction: 33,
                    resource: resource(),
                    reason: 8,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "candidate",
            record(
                K::Candidate,
                &LockCandidate {
                    transaction: 34,
                    lock_epoch: 5,
                    output_id: 1,
                    output_generation: 3,
                    allocation_id: 41,
                    allocation_generation: 4,
                    candidate_generation: 12,
                    pacing_permit: 77,
                    resource: resource(),
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "candidate_outcome",
            record(
                K::CandidateOutcome,
                &LockCandidateOutcome {
                    transaction: 34,
                    lock_epoch: 5,
                    output_id: 1,
                    allocation_id: 41,
                    candidate_generation: 12,
                    status: LockCandidateStatus::Presented,
                    reason: 0,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "frame_demand",
            record(
                K::FrameDemand,
                &LockFrameDemand {
                    transaction: 35,
                    lock_epoch: 5,
                    allocation_id: 41,
                    allocation_generation: 4,
                    demand_id: 3,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "frame_permit",
            record(
                K::FramePermit,
                &LockFramePermit {
                    lock_epoch: 5,
                    allocation_id: 41,
                    allocation_generation: 4,
                    demand_id: 3,
                    pacing_permit: 77,
                    expires_after_ms: 100,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "entry",
            record(
                K::Entry,
                &LockEntry {
                    lock_epoch: 5,
                    entry: LockEntryKind::Insert,
                    empty_after: false,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "chord",
            record(
                K::Chord,
                &LockChord {
                    lock_epoch: 5,
                    chord: 1,
                }
                .encode()
                .unwrap(),
            ),
        ),
        (
            "submit",
            encode_lock_file_submit(LockFileSubmit {
                connection_epoch: EPOCH,
                submission_id: 11,
                candidate_bytes: 128,
            })
            .unwrap()
            .to_vec(),
        ),
        (
            "ack",
            encode_lock_file_ack(LockFileAck {
                connection_epoch: EPOCH,
                sequence: 13,
            })
            .unwrap()
            .to_vec(),
        ),
    ]
}

fn render() -> String {
    let mut text = String::from(
        "# @generated by sophia-protocol tests/lock_file_golden.rs; do not edit.\n# record-name|record-hex\n",
    );
    for (name, bytes) in records() {
        text.push_str(name);
        text.push('|');
        for byte in bytes {
            text.push_str(&format!("{byte:02x}"));
        }
        text.push('\n');
    }
    text
}

#[test]
fn golden_lock_records_match_the_codec() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(GOLDEN);
    let rendered = render();
    let checked_in = std::fs::read_to_string(&path).expect("spec/golden lock records");
    assert_eq!(
        checked_in, rendered,
        "the golden lock records drifted from the codec"
    );
}
