//! Coverage for the candidate/pacing/action codec in
//! `sophia_shell_protocol::shell_files::candidates`: the six single-payload
//! transaction kinds (`FrameDemand`, `FrameDemandCancel`, `ActionAck`,
//! `CandidateOutcome`, `FramePermit`, `Action`) and the composite `Candidate`
//! record (Begin + one Chunk + End under one transaction).
use sophia_shell_protocol::shell::encoding::ValueError;
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;

fn header_bytes(size: u32, kind: u16, epoch: u64, submission: u64, sequence: u64) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(size.to_le_bytes());
    b.extend(1u16.to_le_bytes()); // api version
    b.extend(kind.to_le_bytes());
    b.extend(epoch.to_le_bytes());
    b.extend(submission.to_le_bytes());
    b.extend(sequence.to_le_bytes());
    b
}

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    }
}

fn output_id() -> ContentOutputId {
    ContentOutputId {
        id: 2,
        generation: 1,
    }
}

fn frame_demand_record() -> ShellContentRecord {
    ShellContentRecord::FrameDemand(ContentFrameDemand {
        grant: grant(),
        output: output_id(),
        allocation: ContentAllocationId::default(),
        demand_id: 1,
        reason: 1,
    })
}

fn frame_demand_cancel_record() -> ShellContentRecord {
    ShellContentRecord::FrameDemandCancel(ContentFrameDemandCancel {
        grant: grant(),
        output: output_id(),
        demand_id: 1,
        permit_id: 0,
    })
}

fn frame_permit_record() -> ShellContentRecord {
    ShellContentRecord::FramePermit(ContentFramePermit {
        grant: grant(),
        output: output_id(),
        demand_id: 1,
        permit_id: 1,
        state: 1,
        reason: 0,
        ttl_ms: 10,
        max_candidate_bytes: 100,
    })
}

fn candidate_outcome_record() -> ShellContentRecord {
    ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
        grant: grant(),
        candidate_generation: 1,
        output: output_id(),
        kind: 1,
        reason: 0,
        presentation_epoch: 0,
        work_area_generation: 0,
        wm_commit_generation: 0,
    })
}

fn action_record() -> ShellContentRecord {
    ShellContentRecord::Action(ContentAction {
        grant: grant(),
        output: output_id(),
        candidate_generation: 4,
        presentation_epoch: 5,
        interaction_generation: 1,
        allocation: ContentAllocationId {
            id: 6,
            generation: 7,
        },
        target_id: 8,
        target_generation: 9,
        action_id: 10,
        event_id: 11,
        kind: 1,
        reason: ContentReason::None as u16,
    })
}

fn action_ack_record() -> ShellContentRecord {
    ShellContentRecord::ActionAck(ContentActionAck {
        grant: grant(),
        output: output_id(),
        candidate_generation: 4,
        presentation_epoch: 5,
        interaction_generation: 1,
        allocation: ContentAllocationId {
            id: 6,
            generation: 7,
        },
        target_id: 8,
        target_generation: 9,
        action_id: 10,
        event_id: 11,
        disposition: 1,
    })
}

/// Not a record `shell_file_transaction_kind` carries: used to prove `None`.
fn allocation_request_record() -> ShellContentRecord {
    ShellContentRecord::AllocationRequest(ContentAllocationRequest {
        grant: grant(),
        output: output_id(),
        allocation_request_id: 1,
        operation: 1,
        role: 1,
        edge: 1,
        prior: ContentAllocationId::default(),
        parent: ContentAllocationId::default(),
        parent_presentation_epoch: 0,
        anchor_parent_rect: ContentPixelRect::default(),
        desired_width: 64,
        desired_height: 32,
        margins: ContentMargins::default(),
    })
}

/// Round-trips one single-payload transaction record and checks the body
/// layout `encode_shell_file_transaction_body` promises: `tx.to_le_bytes()`
/// followed by the native record body.
fn assert_transaction_round_trips(
    header: ShellFileHeader,
    kind: ShellFileKind,
    tx_record: &ShellFileTransactionRecord,
) {
    let encoded = encode_shell_file_transaction(header, tx_record).unwrap();
    assert_eq!(
        decode_shell_file_transaction(&encoded, kind).unwrap(),
        tx_record.clone()
    );

    let (body_kind, body) = encode_shell_file_transaction_body(tx_record).unwrap();
    assert_eq!(body_kind, kind);
    assert_eq!(body, encoded[32..]);
    assert_eq!(
        &body[..8],
        tx_record.transaction.raw().to_le_bytes().as_slice()
    );

    assert_eq!(shell_file_transaction_kind(&tx_record.record), Some(kind));
}

#[test]
fn frame_demand_round_trips_and_bounds() {
    let header = ShellFileHeader {
        kind: ShellFileKind::FrameDemand,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(101),
        record: frame_demand_record(),
    };
    assert_transaction_round_trips(header, ShellFileKind::FrameDemand, &tx_record);
}

#[test]
fn frame_demand_cancel_round_trips_and_bounds() {
    let header = ShellFileHeader {
        kind: ShellFileKind::FrameDemandCancel,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(102),
        record: frame_demand_cancel_record(),
    };
    assert_transaction_round_trips(header, ShellFileKind::FrameDemandCancel, &tx_record);
}

#[test]
fn action_ack_round_trips_and_bounds() {
    let header = ShellFileHeader {
        kind: ShellFileKind::ActionAck,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(103),
        record: action_ack_record(),
    };
    assert_transaction_round_trips(header, ShellFileKind::ActionAck, &tx_record);
}

#[test]
fn candidate_outcome_round_trips_and_bounds() {
    let header = ShellFileHeader {
        kind: ShellFileKind::CandidateOutcome,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 1,
    };
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(104),
        record: candidate_outcome_record(),
    };
    assert_transaction_round_trips(header, ShellFileKind::CandidateOutcome, &tx_record);
}

#[test]
fn frame_permit_round_trips_and_bounds() {
    let header = ShellFileHeader {
        kind: ShellFileKind::FramePermit,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 1,
    };
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(105),
        record: frame_permit_record(),
    };
    assert_transaction_round_trips(header, ShellFileKind::FramePermit, &tx_record);
}

#[test]
fn action_round_trips_and_bounds() {
    let header = ShellFileHeader {
        kind: ShellFileKind::Action,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 1,
    };
    let tx_record = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(106),
        record: action_record(),
    };
    assert_transaction_round_trips(header, ShellFileKind::Action, &tx_record);
}

#[test]
fn shell_file_transaction_kind_maps_new_kinds_and_rejects_allocation_request() {
    assert_eq!(
        shell_file_transaction_kind(&frame_demand_record()),
        Some(ShellFileKind::FrameDemand)
    );
    assert_eq!(
        shell_file_transaction_kind(&frame_demand_cancel_record()),
        Some(ShellFileKind::FrameDemandCancel)
    );
    assert_eq!(
        shell_file_transaction_kind(&action_ack_record()),
        Some(ShellFileKind::ActionAck)
    );
    assert_eq!(
        shell_file_transaction_kind(&candidate_outcome_record()),
        Some(ShellFileKind::CandidateOutcome)
    );
    assert_eq!(
        shell_file_transaction_kind(&frame_permit_record()),
        Some(ShellFileKind::FramePermit)
    );
    assert_eq!(
        shell_file_transaction_kind(&action_record()),
        Some(ShellFileKind::Action)
    );
    assert_eq!(
        shell_file_transaction_kind(&allocation_request_record()),
        None
    );
}

#[test]
fn single_payload_transaction_refusals() {
    // Header kind not matching the record: the header says ActionAck, the
    // record is a FrameDemand.
    let mismatched_header = ShellFileHeader {
        kind: ShellFileKind::ActionAck,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let demand = ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(200),
        record: frame_demand_record(),
    };
    assert_eq!(
        encode_shell_file_transaction(mismatched_header, &demand).unwrap_err(),
        ShellFileCodecError::Kind.into()
    );

    // A validly encoded FrameDemand decoded with the wrong kind argument:
    // FrameDemand and ActionAck share the Candidate class, so the class
    // check passes and the mismatch surfaces as `Kind`.
    let demand_header = ShellFileHeader {
        kind: ShellFileKind::FrameDemand,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let encoded = encode_shell_file_transaction(demand_header, &demand).unwrap();
    assert_eq!(
        decode_shell_file_transaction(&encoded, ShellFileKind::ActionAck).unwrap_err(),
        ShellFileCodecError::Kind.into()
    );

    // A kind that carries no single-payload record at all is refused before
    // the bytes are even inspected.
    assert_eq!(
        decode_shell_file_transaction(&[], ShellFileKind::ResourceEnd).unwrap_err(),
        ShellFileCodecError::Kind.into()
    );

    // A body shorter than the 8-byte transaction prefix.
    let mut short = header_bytes(36, ShellFileKind::FrameDemand as u16, 1, 1, 0);
    short.extend_from_slice(&[0u8; 4]);
    assert_eq!(
        decode_shell_file_transaction(&short, ShellFileKind::FrameDemand).unwrap_err(),
        ShellFileCodecError::Length.into()
    );

    // Trailing garbage after the IPC payload is caught by the IPC decoder.
    let mut trailing = encoded.clone();
    trailing.push(0);
    let trailing_len = trailing.len() as u32;
    trailing[0..4].copy_from_slice(&trailing_len.to_le_bytes());
    assert!(matches!(
        decode_shell_file_transaction(&trailing, ShellFileKind::FrameDemand).unwrap_err(),
        ShellFilePayloadError::Records(_)
    ));

    // Transaction 0 is refused both on encode and on decode.
    let mut zero_tx = demand.clone();
    zero_tx.transaction = TransactionId::INVALID;
    assert_eq!(
        encode_shell_file_transaction(demand_header, &zero_tx).unwrap_err(),
        ShellFilePayloadError::Identity
    );
    let mut zero_bytes = encoded.clone();
    zero_bytes[32..40].fill(0);
    assert_eq!(
        decode_shell_file_transaction(&zero_bytes, ShellFileKind::FrameDemand).unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

fn candidate_header() -> ShellFileHeader {
    ShellFileHeader {
        kind: ShellFileKind::Candidate,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    }
}

fn surface_row(index: u32) -> ContentSurface {
    ContentSurface {
        allocation: ContentAllocationId {
            id: u64::from(index) + 1,
            generation: 1,
        },
        scale_generation: 1,
        role: 1,
        edge: 1,
        margins: ContentMargins::default(),
        reservation_extent: 24,
        parent_surface_index: u16::MAX,
        anchor_parent_rect: ContentPixelRect::default(),
    }
}

fn placement_row(surface_index: u16, index: u32) -> ContentPlacement {
    ContentPlacement {
        resource: ContentResourceId {
            id: u64::from(index) + 1,
            generation: 1,
        },
        surface_index,
        destination_x_px: 3,
        destination_y_px: 4,
    }
}

fn target_row(surface_index: u16, index: u32) -> ContentTarget {
    ContentTarget {
        surface_index,
        action_kind: 1,
        target_id: u64::from(index) + 1,
        target_generation: 1,
        action_id: u64::from(index) + 1,
        bounds_px: ContentPixelRect {
            x: 3,
            y: 4,
            width: 2,
            height: 1,
        },
    }
}

/// One whole candidate value: the header fields shared by the owner's
/// Begin/Chunk/End, plus `surfaces`/`placements`/`targets` rows. Since
/// `ContentCandidate` stores each row vector once, the counts `parts()`
/// reports on Begin/End are always in lock-step with the Chunk's rows -- a
/// count mismatch between parts is no longer representable.
fn content_candidate_with_counts(surfaces: u32, placements: u32, targets: u32) -> ContentCandidate {
    let surface_span = surfaces.max(1);
    ContentCandidate {
        grant: grant(),
        candidate_generation: 1,
        output: output_id(),
        facts_generation: 3,
        pacing_permit: 1,
        interaction_generation: 4,
        surfaces: (0..surfaces).map(surface_row).collect(),
        placements: (0..placements)
            .map(|i| placement_row((i % surface_span) as u16, i))
            .collect(),
        targets: (0..targets)
            .map(|i| target_row((i % surface_span) as u16, i))
            .collect(),
    }
}

fn shell_file_candidate_with_counts(
    transaction: TransactionId,
    surfaces: u32,
    placements: u32,
    targets: u32,
) -> ShellFileCandidate {
    ShellFileCandidate {
        transaction,
        candidate: content_candidate_with_counts(surfaces, placements, targets),
    }
}

#[test]
fn content_candidate_parts_return_begin_chunk_end_in_order() {
    let candidate = content_candidate_with_counts(2, 3, 4);
    let parts = candidate.parts();

    let begin = match &parts[0] {
        ShellContentRecord::CandidateBegin(v) => v,
        other => panic!("expected CandidateBegin, got {other:?}"),
    };
    let chunk = match &parts[1] {
        ShellContentRecord::CandidateChunk(v) => v,
        other => panic!("expected CandidateChunk, got {other:?}"),
    };
    let end = match &parts[2] {
        ShellContentRecord::CandidateEnd(v) => v,
        other => panic!("expected CandidateEnd, got {other:?}"),
    };

    assert_eq!(chunk.chunk_ordinal, 0);
    assert_eq!(chunk.surfaces.len(), 2);
    assert_eq!(chunk.placements.len(), 3);
    assert_eq!(chunk.targets.len(), 4);
    for (label, s, p, t) in [
        (
            "begin",
            begin.surface_count,
            begin.placement_count,
            begin.target_count,
        ),
        (
            "end",
            end.surface_count,
            end.placement_count,
            end.target_count,
        ),
    ] {
        assert_eq!((s, p, t), (2, 3, 4), "{label} counts");
    }

    // The grant and candidate_generation are shared, not per-part state.
    for label_grant in [begin.grant, chunk.grant, end.grant] {
        assert_eq!(label_grant, candidate.grant);
    }
    for label_gen in [
        begin.candidate_generation,
        chunk.candidate_generation,
        end.candidate_generation,
    ] {
        assert_eq!(label_gen, candidate.candidate_generation);
    }
}

#[test]
fn candidate_round_trips() {
    let header = candidate_header();
    let candidate = shell_file_candidate_with_counts(TransactionId::from_raw(60), 2, 3, 4);
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    assert_eq!(decode_shell_file_candidate(&encoded).unwrap(), candidate);
}

#[test]
fn candidate_layout_offsets_are_exact() {
    let header = candidate_header();
    let candidate = ShellFileCandidate {
        transaction: TransactionId::from_raw(0xABCD),
        candidate: ContentCandidate {
            grant: ContentGrant {
                connection_epoch: 0x1111_1111_1111_1111,
                content_grant_epoch: 0x2222_2222_2222_2222,
            },
            candidate_generation: 0x3333_3333_3333_3333,
            output: ContentOutputId {
                id: 0x4444_4444_4444_4444,
                generation: 0x5555_5555_5555_5555,
            },
            facts_generation: 0x6666_6666_6666_6666,
            pacing_permit: 0x7777_7777_7777_7777,
            interaction_generation: 0x8888_8888_8888_8888,
            surfaces: vec![surface_row(0)],
            placements: vec![placement_row(0, 0)],
            targets: vec![target_row(0, 0)],
        },
    };
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    // Body layout: tx (8) then the candidate header (grant, candidate_generation,
    // output, facts_generation, pacing_permit, interaction_generation, three
    // row counts, one reserved u16) at the offsets
    // `encode_content_candidate`/`decode_content_candidate` document.
    let body = &encoded[32..];
    assert_eq!(
        &body[0..8],
        candidate.transaction.raw().to_le_bytes().as_slice()
    );
    assert_eq!(
        &body[8..16],
        0x1111_1111_1111_1111u64.to_le_bytes().as_slice()
    );
    assert_eq!(
        &body[16..24],
        0x2222_2222_2222_2222u64.to_le_bytes().as_slice()
    );
    assert_eq!(
        &body[24..32],
        0x3333_3333_3333_3333u64.to_le_bytes().as_slice()
    );
    assert_eq!(
        &body[32..40],
        0x4444_4444_4444_4444u64.to_le_bytes().as_slice()
    );
    assert_eq!(
        &body[40..48],
        0x5555_5555_5555_5555u64.to_le_bytes().as_slice()
    );
    assert_eq!(
        &body[48..56],
        0x6666_6666_6666_6666u64.to_le_bytes().as_slice()
    );
    assert_eq!(
        &body[56..64],
        0x7777_7777_7777_7777u64.to_le_bytes().as_slice()
    );
    assert_eq!(
        &body[64..72],
        0x8888_8888_8888_8888u64.to_le_bytes().as_slice()
    );
    assert_eq!(&body[72..74], 1u16.to_le_bytes().as_slice()); // surface_count
    assert_eq!(&body[74..76], 1u16.to_le_bytes().as_slice()); // placement_count
    assert_eq!(&body[76..78], 1u16.to_le_bytes().as_slice()); // target_count
    assert_eq!(&body[78..80], 0u16.to_le_bytes().as_slice()); // reserved

    assert_eq!(decode_shell_file_candidate(&encoded).unwrap(), candidate);
}

#[test]
fn candidate_refuses_zero_transaction() {
    let header = candidate_header();
    let mut candidate = shell_file_candidate_with_counts(TransactionId::from_raw(64), 1, 1, 1);
    candidate.transaction = TransactionId::INVALID;
    assert_eq!(
        encode_shell_file_candidate(header, &candidate).unwrap_err(),
        ShellFilePayloadError::Identity
    );

    candidate.transaction = TransactionId::from_raw(64);
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    let mut bad_bytes = encoded.clone();
    bad_bytes[32..40].fill(0);
    // `decode_shell_file_candidate` checks `transaction.is_valid()` directly
    // after reading the raw u64, the same guard `decode_shell_file_transaction`
    // uses for the single-payload kinds, so both sides now agree on
    // `Identity` -- the encode/decode asymmetry the old Begin/Chunk/End
    // layout had here is gone under the native layout.
    assert_eq!(
        decode_shell_file_candidate(&bad_bytes).unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

#[test]
fn candidate_refuses_reserved_nonzero() {
    let header = candidate_header();
    let candidate = shell_file_candidate_with_counts(TransactionId::from_raw(66), 1, 1, 1);
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    let mut bad_bytes = encoded.clone();
    // The candidate header's reserved u16 sits at body offset 78 (8-byte tx
    // + 70-byte candidate header prefix); record offset 32 + 78 = 110.
    bad_bytes[110..112].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_candidate(&bad_bytes).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::ReservedNonZero(1))
    );
}

#[test]
fn candidate_refuses_count_above_maximum() {
    let header = candidate_header();
    let candidate = shell_file_candidate_with_counts(TransactionId::from_raw(67), 1, 1, 1);
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    let mut bad_bytes = encoded.clone();
    // surface_count sits at body offset 72, record offset 32 + 72 = 104; the
    // maximum is 8.
    bad_bytes[104..106].copy_from_slice(&9u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_candidate(&bad_bytes).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::CountTooLarge { count: 9, max: 8 })
    );
}

#[test]
fn candidate_refuses_count_exceeding_rows_present() {
    let header = candidate_header();
    // Zero rows in every table is itself a valid candidate (the validators
    // only enforce maximums), so the encoded body ends right after the
    // 72-byte candidate header with no row bytes at all.
    let candidate = shell_file_candidate_with_counts(TransactionId::from_raw(68), 0, 0, 0);
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    let mut bad_bytes = encoded.clone();
    // Claim one surface row (record offset 104..106) that was never written.
    bad_bytes[104..106].copy_from_slice(&1u16.to_le_bytes());
    assert_eq!(
        decode_shell_file_candidate(&bad_bytes).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::Truncated)
    );
}

#[test]
fn candidate_refuses_trailing_bytes() {
    let header = candidate_header();
    let candidate = shell_file_candidate_with_counts(TransactionId::from_raw(69), 1, 1, 1);
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    let mut trailing = encoded.clone();
    trailing.push(0);
    let trailing_len = trailing.len() as u32;
    trailing[0..4].copy_from_slice(&trailing_len.to_le_bytes());
    assert_eq!(
        decode_shell_file_candidate(&trailing).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::TrailingBytes(1))
    );
}

#[test]
fn candidate_refuses_invalid_row_content() {
    let header = candidate_header();
    let mut candidate = shell_file_candidate_with_counts(TransactionId::from_raw(71), 1, 1, 1);
    // A zero `pacing_permit` fails the `content candidate begin` validator
    // (every part is validated on encode, before any bytes are written).
    candidate.candidate.pacing_permit = 0;
    assert_eq!(
        encode_shell_file_candidate(header, &candidate).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidRecord("content candidate begin"))
    );

    // The same validator runs again on decode: patch a validly encoded
    // candidate's pacing_permit (body offset 56, record offset 88) to 0.
    candidate.candidate.pacing_permit = 1;
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    let mut bad_bytes = encoded.clone();
    bad_bytes[88..96].fill(0);
    assert_eq!(
        decode_shell_file_candidate(&bad_bytes).unwrap_err(),
        ShellFilePayloadError::Records(ValueError::InvalidRecord("content candidate begin"))
    );
}

#[test]
fn candidate_decode_refuses_bytes_over_the_max_cap() {
    // A record whose total size already exceeds the cap is refused before
    // the header or any field is parsed.
    let bytes = vec![0u8; SHELL_FILE_MAX_CANDIDATE_BYTES + 1];
    assert_eq!(
        decode_shell_file_candidate(&bytes).unwrap_err(),
        ShellFileCodecError::Length.into()
    );
}

#[test]
fn maximal_candidate_fits_within_the_cap() {
    // The content validators' maximum candidate shape: 8 surfaces, 32
    // placements, 64 targets (crates/sophia-protocol/src/shell/content/
    // validation.rs `counts()` and `validate_candidate_chunk_profile`).
    let header = candidate_header();
    let candidate = shell_file_candidate_with_counts(TransactionId::from_raw(70), 8, 32, 64);
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    assert!(encoded.len() <= SHELL_FILE_MAX_CANDIDATE_BYTES);
    assert_eq!(decode_shell_file_candidate(&encoded).unwrap(), candidate);
    // 32-byte record header + 8-byte tx + 72-byte candidate header
    // + 8 surface rows (64 B each) + 32 placement rows (32 B each)
    // + 64 target rows (48 B each) = 4720 bytes, well under the 8192 cap.
    assert_eq!(encoded.len(), 4720);
}

#[test]
fn shell_file_class_for_new_kinds() {
    for (kind, class) in [
        (ShellFileKind::CandidateOutcome, ShellFileClass::Event),
        (ShellFileKind::FramePermit, ShellFileClass::Event),
        (ShellFileKind::Action, ShellFileClass::Event),
        (ShellFileKind::Candidate, ShellFileClass::Candidate),
        (ShellFileKind::FrameDemand, ShellFileClass::Candidate),
        (ShellFileKind::FrameDemandCancel, ShellFileClass::Candidate),
        (ShellFileKind::ActionAck, ShellFileClass::Candidate),
    ] {
        assert_eq!(shell_file_class(kind), class);
    }
}

#[test]
fn decode_shell_file_record_accepts_new_raw_kind_values() {
    for (raw, class) in [
        (35u16, ShellFileClass::Event),
        (36, ShellFileClass::Event),
        (37, ShellFileClass::Event),
        (262, ShellFileClass::Candidate),
        (263, ShellFileClass::Candidate),
        (264, ShellFileClass::Candidate),
        (265, ShellFileClass::Candidate),
    ] {
        let (submission, sequence) = match class {
            ShellFileClass::Event => (0, 1),
            ShellFileClass::Candidate => (1, 0),
            ShellFileClass::Object => unreachable!("no new Object kinds"),
        };
        let bytes = header_bytes(32, raw, 1, submission, sequence);
        let record = decode_shell_file_record(&bytes, class).unwrap();
        assert_eq!(record.header.kind as u16, raw);
    }
}
