//! Coverage for the candidate/pacing/action codec in
//! `sophia_protocol::shell_files::candidates`: the six single-payload
//! transaction kinds (`FrameDemand`, `FrameDemandCancel`, `ActionAck`,
//! `CandidateOutcome`, `FramePermit`, `Action`) and the composite `Candidate`
//! record (Begin + one Chunk + End under one transaction).
use sophia_protocol::shell_files::*;
use sophia_protocol::*;

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
/// followed by the unchanged IPC payload.
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
    let frame = encode_shell_content_frame(tx_record.transaction, &tx_record.record).unwrap();
    assert_eq!(&body[8..], &frame[SOPHIA_IPC_HEADER_LEN..]);

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

/// A coherent candidate: one grant and candidate generation shared by the
/// Begin, its ordinal-0 Chunk and the End, with `surfaces`/`placements`/
/// `targets` rows matching the declared counts on Begin and End.
fn candidate_with_counts(
    transaction: TransactionId,
    surfaces: u32,
    placements: u32,
    targets: u32,
) -> ShellFileCandidate {
    let grant = grant();
    let candidate_generation = 1;
    let surface_span = surfaces.max(1);
    ShellFileCandidate {
        transaction,
        begin: ContentCandidateBegin {
            grant,
            candidate_generation,
            output: output_id(),
            facts_generation: 3,
            pacing_permit: 1,
            interaction_generation: 4,
            surface_count: surfaces,
            placement_count: placements,
            target_count: targets,
        },
        chunk: ContentCandidateChunk {
            grant,
            candidate_generation,
            chunk_ordinal: 0,
            surfaces: (0..surfaces).map(surface_row).collect(),
            placements: (0..placements)
                .map(|i| placement_row((i % surface_span) as u16, i))
                .collect(),
            targets: (0..targets)
                .map(|i| target_row((i % surface_span) as u16, i))
                .collect(),
        },
        end: ContentCandidateEnd {
            grant,
            candidate_generation,
            surface_count: surfaces,
            placement_count: placements,
            target_count: targets,
        },
    }
}

#[test]
fn candidate_records_return_begin_chunk_end_in_order() {
    let candidate = candidate_with_counts(TransactionId::from_raw(50), 1, 1, 1);
    let records = candidate.records();
    assert!(matches!(records[0], ShellContentRecord::CandidateBegin(_)));
    assert!(matches!(records[1], ShellContentRecord::CandidateChunk(_)));
    assert!(matches!(records[2], ShellContentRecord::CandidateEnd(_)));
}

#[test]
fn candidate_round_trips_with_layout_offsets() {
    let header = candidate_header();
    let candidate = candidate_with_counts(TransactionId::from_raw(60), 1, 1, 1);
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    assert_eq!(decode_shell_file_candidate(&encoded).unwrap(), candidate);

    // Body bytes 8..12 and 12..16 are the Begin and Chunk payload lengths.
    let body = &encoded[32..];
    let begin_len = u32::from_le_bytes(body[8..12].try_into().unwrap()) as usize;
    let chunk_len = u32::from_le_bytes(body[12..16].try_into().unwrap()) as usize;
    let begin_frame = encode_shell_content_frame(
        candidate.transaction,
        &ShellContentRecord::CandidateBegin(candidate.begin.clone()),
    )
    .unwrap();
    let chunk_frame = encode_shell_content_frame(
        candidate.transaction,
        &ShellContentRecord::CandidateChunk(candidate.chunk.clone()),
    )
    .unwrap();
    assert_eq!(begin_len, begin_frame.len() - SOPHIA_IPC_HEADER_LEN);
    assert_eq!(chunk_len, chunk_frame.len() - SOPHIA_IPC_HEADER_LEN);
    // 16 (tx + two length fields) + begin + chunk + end must equal the body.
    let end_frame = encode_shell_content_frame(
        candidate.transaction,
        &ShellContentRecord::CandidateEnd(candidate.end.clone()),
    )
    .unwrap();
    let end_len = end_frame.len() - SOPHIA_IPC_HEADER_LEN;
    assert_eq!(16 + begin_len + chunk_len + end_len, body.len());
}

/// The absolute byte offset of the Chunk payload within a full candidate
/// encoding: 32-byte header + 8-byte tx + 4+4 length fields + Begin payload.
fn chunk_payload_offset(encoded: &[u8]) -> usize {
    let begin_len = u32::from_le_bytes(encoded[40..44].try_into().unwrap()) as usize;
    48 + begin_len
}

#[test]
fn candidate_refuses_nonzero_chunk_ordinal() {
    let header = candidate_header();
    let mut candidate = candidate_with_counts(TransactionId::from_raw(61), 1, 1, 1);
    candidate.chunk.chunk_ordinal = 1;
    assert_eq!(
        encode_shell_file_candidate(header, &candidate).unwrap_err(),
        ShellFilePayloadError::Identity
    );

    candidate.chunk.chunk_ordinal = 0;
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    let chunk_at = chunk_payload_offset(&encoded);
    let mut bad_bytes = encoded.clone();
    // chunk_ordinal sits 24 bytes into the Chunk payload (grant + generation).
    bad_bytes[chunk_at + 24..chunk_at + 28].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        decode_shell_file_candidate(&bad_bytes).unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

#[test]
fn candidate_refuses_mismatched_grant() {
    let header = candidate_header();
    let mut candidate = candidate_with_counts(TransactionId::from_raw(62), 1, 1, 1);
    candidate.chunk.grant.connection_epoch = 2;
    assert_eq!(
        encode_shell_file_candidate(header, &candidate).unwrap_err(),
        ShellFilePayloadError::Identity
    );

    candidate.chunk.grant.connection_epoch = 1;
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    let chunk_at = chunk_payload_offset(&encoded);
    let mut bad_bytes = encoded.clone();
    // grant.connection_epoch is the first 8 bytes of the Chunk payload.
    bad_bytes[chunk_at..chunk_at + 8].copy_from_slice(&2u64.to_le_bytes());
    assert_eq!(
        decode_shell_file_candidate(&bad_bytes).unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

#[test]
fn candidate_refuses_mismatched_candidate_generation() {
    let header = candidate_header();
    let mut candidate = candidate_with_counts(TransactionId::from_raw(63), 1, 1, 1);
    candidate.chunk.candidate_generation = 2;
    assert_eq!(
        encode_shell_file_candidate(header, &candidate).unwrap_err(),
        ShellFilePayloadError::Identity
    );

    candidate.chunk.candidate_generation = 1;
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    let chunk_at = chunk_payload_offset(&encoded);
    let mut bad_bytes = encoded.clone();
    // candidate_generation sits 16 bytes into the Chunk payload (after grant).
    bad_bytes[chunk_at + 16..chunk_at + 24].copy_from_slice(&2u64.to_le_bytes());
    assert_eq!(
        decode_shell_file_candidate(&bad_bytes).unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

#[test]
fn candidate_refuses_zero_transaction() {
    let header = candidate_header();
    let mut candidate = candidate_with_counts(TransactionId::from_raw(64), 1, 1, 1);
    candidate.transaction = TransactionId::INVALID;
    assert_eq!(
        encode_shell_file_candidate(header, &candidate).unwrap_err(),
        ShellFilePayloadError::Identity
    );

    candidate.transaction = TransactionId::from_raw(64);
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    let mut bad_bytes = encoded.clone();
    bad_bytes[32..40].fill(0);
    let err = decode_shell_file_candidate(&bad_bytes).unwrap_err();
    // `decode_shell_file_transaction` (the single-payload sibling) has an
    // explicit `transaction.is_valid()` guard before it calls into the IPC
    // decoder, so a zero transaction there always comes back as `Identity`,
    // matching the encode-side rejection above. `decode_shell_file_candidate`
    // has no equivalent guard: it hands the zero transaction straight to
    // `decode_shell_content_payload` for the Begin part, which rejects it at
    // the IPC layer as `InvalidTransaction(0)` before `coherent()` ever runs.
    // That looks like a real asymmetry/bug in candidates.rs; see the report.
    assert_eq!(
        err,
        ShellFilePayloadError::Identity,
        "expected Identity (matching encode and the single-payload sibling), got {err:?}"
    );
}

#[test]
fn candidate_decode_refuses_lengths_past_the_body() {
    let header = candidate_header();
    let candidate = candidate_with_counts(TransactionId::from_raw(65), 1, 1, 1);
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    let mut bad_bytes = encoded.clone();
    // Inflate the Chunk length field (body bytes 12..16, absolute 44..48)
    // far past what the record actually holds.
    bad_bytes[44..48].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_shell_file_candidate(&bad_bytes).unwrap_err(),
        ShellFileCodecError::Length.into()
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
    // The IPC validators' maximum candidate shape: 8 surfaces, 32
    // placements, 64 targets (crates/sophia-protocol/src/ipc/shell_content/
    // validation.rs `counts()` and `validate_candidate_chunk_profile`).
    let header = candidate_header();
    let candidate = candidate_with_counts(TransactionId::from_raw(70), 8, 32, 64);
    let encoded = encode_shell_file_candidate(header, &candidate).unwrap();
    assert!(encoded.len() <= SHELL_FILE_MAX_CANDIDATE_BYTES);
    assert_eq!(decode_shell_file_candidate(&encoded).unwrap(), candidate);
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
