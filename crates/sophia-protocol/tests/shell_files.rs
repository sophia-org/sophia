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

#[test]
fn header_identity_rules_per_class() {
    // Object: submission=0, sequence=0
    let h1 = ShellFileHeader {
        kind: ShellFileKind::Limits,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 0,
    };
    assert!(encode_shell_file_record(h1, &[]).is_ok());

    let h2 = ShellFileHeader {
        kind: ShellFileKind::Limits,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    assert_eq!(
        encode_shell_file_record(h2, &[]).unwrap_err(),
        ShellFileCodecError::Identity
    );

    // Candidate: submission!=0, sequence=0
    let h3 = ShellFileHeader {
        kind: ShellFileKind::Negotiate,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    assert!(encode_shell_file_record(h3, &[]).is_ok());

    let h4 = ShellFileHeader {
        kind: ShellFileKind::Negotiate,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 0,
    };
    assert_eq!(
        encode_shell_file_record(h4, &[]).unwrap_err(),
        ShellFileCodecError::Identity
    );

    // Event: submission=0, sequence!=0
    let h5 = ShellFileHeader {
        kind: ShellFileKind::Submitted,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 1,
    };
    assert!(encode_shell_file_record(h5, &[]).is_ok());

    let h6 = ShellFileHeader {
        kind: ShellFileKind::Submitted,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 1,
    };
    assert_eq!(
        encode_shell_file_record(h6, &[]).unwrap_err(),
        ShellFileCodecError::Identity
    );
}

#[test]
fn decode_wrong_class() {
    let bytes = header_bytes(32, 1, 1, 0, 0); // Limits (Object)
    assert_eq!(
        decode_shell_file_record(&bytes, ShellFileClass::Event).unwrap_err(),
        ShellFileCodecError::Class
    );
}

#[test]
fn decode_unknown_kind() {
    let bytes = header_bytes(32, 999, 1, 0, 0);
    assert_eq!(
        decode_shell_file_record(&bytes, ShellFileClass::Object).unwrap_err(),
        ShellFileCodecError::Kind
    );
}

#[test]
fn decode_api_version() {
    let mut bytes = header_bytes(32, 1, 1, 0, 0);
    bytes[4] = 2; // wrong version
    assert_eq!(
        decode_shell_file_record(&bytes, ShellFileClass::Object).unwrap_err(),
        ShellFileCodecError::Version
    );
}

#[test]
fn decode_total_bytes_mismatch() {
    let mut bytes = header_bytes(32, 1, 1, 0, 0);
    bytes.push(0); // 33 bytes actual, 32 claimed
    assert_eq!(
        decode_shell_file_record(&bytes, ShellFileClass::Object).unwrap_err(),
        ShellFileCodecError::Length
    );
}

#[test]
fn candidate_65536_accepted_65537_refused() {
    let h = ShellFileHeader {
        kind: ShellFileKind::Negotiate,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let body_65536 = vec![0u8; 65536 - 32];
    let encoded_65536 = encode_shell_file_record(h, &body_65536).unwrap();
    assert_eq!(encoded_65536.len(), 65536);
    assert!(decode_shell_file_record(&encoded_65536, ShellFileClass::Candidate).is_ok());

    let body_65537 = vec![0u8; 65537 - 32];
    assert_eq!(
        encode_shell_file_record(h, &body_65537).unwrap_err(),
        ShellFileCodecError::Length
    );
    let mut bad_bytes = header_bytes(65537, 256, 1, 1, 0);
    bad_bytes.extend(&body_65537);
    assert_eq!(
        decode_shell_file_record(&bad_bytes, ShellFileClass::Candidate).unwrap_err(),
        ShellFileCodecError::Length
    );
}

#[test]
fn object_4mib_bound() {
    let h = ShellFileHeader {
        kind: ShellFileKind::Limits,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 0,
    };
    let body_4mib = vec![0u8; 4_194_304 - 32];
    let encoded_4mib = encode_shell_file_record(h, &body_4mib).unwrap();
    assert_eq!(encoded_4mib.len(), 4_194_304);
    assert!(decode_shell_file_record(&encoded_4mib, ShellFileClass::Object).is_ok());

    let body_toolarge = vec![0u8; 4_194_305 - 32];
    assert_eq!(
        encode_shell_file_record(h, &body_toolarge).unwrap_err(),
        ShellFileCodecError::Length
    );
}

#[test]
fn negotiate_round_trips_and_bounds() {
    let h = ShellFileHeader {
        kind: ShellFileKind::Negotiate,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let hello = ShellV1ClientHello {
        minimum_revision: 1,
        maximum_revision: 8,
        required_capabilities: 42,
    };
    let encoded = encode_shell_file_negotiate(h, hello).unwrap();
    assert_eq!(encoded.len(), 48); // 32 + 16
    let decoded = decode_shell_file_negotiate(&encoded).unwrap();
    assert_eq!(decoded, hello);

    let mut extra = encoded.clone();
    extra.push(0);
    extra[0] = 49;
    assert_eq!(
        decode_shell_file_negotiate(&extra).unwrap_err(),
        ShellFileCodecError::Length.into()
    );

    let mut short = encoded.clone();
    short.pop();
    short[0] = 47;
    assert_eq!(
        decode_shell_file_negotiate(&short).unwrap_err(),
        ShellFileCodecError::Length.into()
    );

    let mut reserved = encoded.clone();
    reserved[36] = 1; // reserved u32 @4
    assert_eq!(
        decode_shell_file_negotiate(&reserved).unwrap_err(),
        ShellFileCodecError::Reserved.into()
    );

    let hello_bad_min = ShellV1ClientHello {
        minimum_revision: 0,
        maximum_revision: 8,
        required_capabilities: 42,
    };
    assert_eq!(
        encode_shell_file_negotiate(h, hello_bad_min).unwrap_err(),
        ShellFilePayloadError::Value
    );

    let hello_min_gt_max = ShellV1ClientHello {
        minimum_revision: 9,
        maximum_revision: 8,
        required_capabilities: 42,
    };
    assert_eq!(
        encode_shell_file_negotiate(h, hello_min_gt_max).unwrap_err(),
        ShellFilePayloadError::Value
    );
}

#[test]
fn negotiated_round_trips_and_bounds() {
    let h = ShellFileHeader {
        kind: ShellFileKind::Negotiated,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 1,
    };
    let value = ShellFileNegotiated {
        welcome: ShellV1ServerWelcome {
            selected_revision: 8,
            connection_epoch: 1,
            capabilities: 42,
            max_descriptors: 10,
            max_label_bytes: 20,
            max_pending_activations: 5,
        },
        limits_published: true,
    };
    let encoded = encode_shell_file_negotiated(h, value).unwrap();
    assert_eq!(encoded.len(), 64); // 32 + 32
    let decoded = decode_shell_file_negotiated(&encoded).unwrap();
    assert_eq!(decoded, value);

    let mut extra = encoded.clone();
    extra.push(0);
    extra[0] = 65;
    assert_eq!(
        decode_shell_file_negotiated(&extra).unwrap_err(),
        ShellFileCodecError::Length.into()
    );

    let mut short = encoded.clone();
    short.pop();
    short[0] = 63;
    assert_eq!(
        decode_shell_file_negotiated(&short).unwrap_err(),
        ShellFileCodecError::Length.into()
    );

    let mut reserved = encoded.clone();
    reserved[34] = 1; // reserved u16 @2
    assert_eq!(
        decode_shell_file_negotiated(&reserved).unwrap_err(),
        ShellFileCodecError::Reserved.into()
    );

    let mut value_bad_epoch = value;
    value_bad_epoch.welcome.connection_epoch = 2; // mismatched epoch
    assert_eq!(
        encode_shell_file_negotiated(h, value_bad_epoch).unwrap_err(),
        ShellFilePayloadError::Identity
    );
}

#[test]
fn refused_round_trips_and_bounds() {
    let h = ShellFileHeader {
        kind: ShellFileKind::Refused,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 1,
    };
    let value = ContentAdmissionRefused {
        reason: 2,
        denied_capabilities: 42,
    };
    let encoded = encode_shell_file_refused(h, value.clone()).unwrap();
    assert_eq!(encoded.len(), 48); // 32 + 16
    let decoded = decode_shell_file_refused(&encoded).unwrap();
    assert_eq!(decoded, value);

    let mut extra = encoded.clone();
    extra.push(0);
    extra[0] = 49;
    assert_eq!(
        decode_shell_file_refused(&extra).unwrap_err(),
        ShellFileCodecError::Length.into()
    );

    let mut short = encoded.clone();
    short.pop();
    short[0] = 47;
    assert_eq!(
        decode_shell_file_refused(&short).unwrap_err(),
        ShellFileCodecError::Length.into()
    );

    let mut reserved = encoded.clone();
    reserved[34] = 1; // reserved u16 @2
    assert_eq!(
        decode_shell_file_refused(&reserved).unwrap_err(),
        ShellFileCodecError::Reserved.into()
    );

    let value_bad_reason_0 = ContentAdmissionRefused {
        reason: 0,
        denied_capabilities: 42,
    };
    assert_eq!(
        encode_shell_file_refused(h, value_bad_reason_0).unwrap_err(),
        ShellFilePayloadError::Value
    );

    let value_bad_reason_5 = ContentAdmissionRefused {
        reason: 5,
        denied_capabilities: 42,
    };
    assert_eq!(
        encode_shell_file_refused(h, value_bad_reason_5).unwrap_err(),
        ShellFilePayloadError::Value
    );
}

#[test]
fn submitted_round_trips_and_bounds() {
    let h = ShellFileHeader {
        kind: ShellFileKind::Submitted,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 1,
    };
    let value = ShellFileSubmitted {
        submission_id: 7,
        candidate_kind: ShellFileKind::Negotiate, // Candidate class
    };
    let encoded = encode_shell_file_submitted(h, value).unwrap();
    assert_eq!(encoded.len(), 48); // 32 + 16
    let decoded = decode_shell_file_submitted(&encoded).unwrap();
    assert_eq!(decoded, value);

    let mut extra = encoded.clone();
    extra.push(0);
    extra[0] = 49;
    assert_eq!(
        decode_shell_file_submitted(&extra).unwrap_err(),
        ShellFileCodecError::Length.into()
    );

    let mut short = encoded.clone();
    short.pop();
    short[0] = 47;
    assert_eq!(
        decode_shell_file_submitted(&short).unwrap_err(),
        ShellFileCodecError::Length.into()
    );

    let mut reserved = encoded.clone();
    reserved[42] = 1; // reserved @10
    assert_eq!(
        decode_shell_file_submitted(&reserved).unwrap_err(),
        ShellFileCodecError::Reserved.into()
    );

    let value_bad_kind = ShellFileSubmitted {
        submission_id: 7,
        candidate_kind: ShellFileKind::Limits, // Object class, not Candidate
    };
    assert_eq!(
        encode_shell_file_submitted(h, value_bad_kind).unwrap_err(),
        ShellFileCodecError::Kind.into()
    );
}

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    }
}

fn allocation_request() -> ShellContentRecord {
    ShellContentRecord::AllocationRequest(ContentAllocationRequest {
        grant: grant(),
        output: ContentOutputId {
            id: 2,
            generation: 1,
        },
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

/// The rejected-result shape the allocation owner emits.
fn allocation_result() -> ShellContentRecord {
    ShellContentRecord::AllocationResult(ContentAllocationResult {
        grant: grant(),
        allocation_request_id: 1,
        status: 2,
        reason: ContentReason::OutputLost as u16,
        output: ContentOutputId {
            id: 2,
            generation: 1,
        },
        allocation: ContentAllocationId::default(),
        parent: ContentAllocationId::default(),
        scale_generation: 0,
        logical: ContentLogicalRect::default(),
        pixel: ContentPixelRect::default(),
        scale_numerator: 0,
        scale_denominator: 0,
        allowed_reservation_extent: 0,
        margins: ContentMargins::default(),
        acknowledged_anchor: ContentPixelRect::default(),
    })
}

#[test]
fn allocation_request_result_round_trips_and_bounds() {
    let h_req = ShellFileHeader {
        kind: ShellFileKind::AllocationRequest,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let tx = TransactionId::from_raw(99);
    let tx_record_req = ShellFileTransactionRecord {
        transaction: tx,
        record: allocation_request(),
    };
    let encoded_req = encode_shell_file_allocation_request(h_req, tx_record_req.clone()).unwrap();
    assert_eq!(
        decode_shell_file_allocation_request(&encoded_req).unwrap(),
        tx_record_req
    );
    // After the 32-byte header and 8-byte transaction, the body is exactly
    // the existing IPC payload of the same record.
    let frame = encode_shell_content_frame(tx, &allocation_request()).unwrap();
    assert_eq!(&encoded_req[40..], &frame[SOPHIA_IPC_HEADER_LEN..]);
    assert_eq!(
        u64::from_le_bytes(encoded_req[32..40].try_into().unwrap()),
        99
    );

    let mut bad_tx = tx_record_req.clone();
    bad_tx.transaction = TransactionId::INVALID;
    assert_eq!(
        encode_shell_file_allocation_request(h_req, bad_tx).unwrap_err(),
        ShellFilePayloadError::Identity
    );
    let mut zero_tx = encoded_req.clone();
    zero_tx[32..40].fill(0);
    assert_eq!(
        decode_shell_file_allocation_request(&zero_tx).unwrap_err(),
        ShellFilePayloadError::Identity
    );

    let h_res = ShellFileHeader {
        kind: ShellFileKind::AllocationResult,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 1,
    };
    let tx_record_res = ShellFileTransactionRecord {
        transaction: tx,
        record: allocation_result(),
    };
    let encoded_res = encode_shell_file_allocation_result(h_res, tx_record_res.clone()).unwrap();
    assert_eq!(
        decode_shell_file_allocation_result(&encoded_res).unwrap(),
        tx_record_res
    );
    assert_eq!(
        encode_shell_file_allocation_result_body(&tx_record_res).unwrap(),
        encoded_res[32..]
    );
    // A request record cannot be encoded as a result, or the reverse.
    assert!(encode_shell_file_allocation_result(h_res, tx_record_req).is_err());
    // A result body relabelled as a request candidate is refused.
    let mut relabelled = encoded_res.clone();
    relabelled[6..8].copy_from_slice(&257u16.to_le_bytes());
    relabelled[16..24].copy_from_slice(&1u64.to_le_bytes());
    relabelled[24..32].fill(0);
    assert!(decode_shell_file_allocation_request(&relabelled).is_err());
    // An event is not a candidate.
    assert!(decode_shell_file_allocation_request(&encoded_res).is_err());
}

#[test]
fn limits_round_trips_and_byte_identity() {
    let h = ShellFileHeader {
        kind: ShellFileKind::Limits,
        connection_epoch: 1,
        submission_id: 0,
        sequence: 0,
    };
    let limits = ContentLimits::prototype(grant());
    let encoded = encode_shell_file_limits(h, limits.clone()).unwrap();
    assert_eq!(decode_shell_file_limits(&encoded).unwrap(), limits);
    let frame =
        encode_shell_content_frame(TransactionId::INVALID, &ShellContentRecord::Limits(limits))
            .unwrap();
    assert_eq!(&encoded[32..], &frame[SOPHIA_IPC_HEADER_LEN..]);
}

#[test]
fn submit_and_ack() {
    let submit = ShellFileSubmit {
        connection_epoch: 1,
        submission_id: 2,
        candidate_bytes: 64,
    };
    let encoded_sub = encode_shell_file_submit(submit).unwrap();
    assert_eq!(encoded_sub.len(), 24);
    let decoded_sub = decode_shell_file_submit(&encoded_sub).unwrap();
    assert_eq!(decoded_sub, submit);

    let ack = ShellFileAck {
        connection_epoch: 1,
        sequence: 3,
    };
    let encoded_ack = encode_shell_file_ack(ack).unwrap();
    assert_eq!(encoded_ack.len(), 16);
    let decoded_ack = decode_shell_file_ack(&encoded_ack).unwrap();
    assert_eq!(decoded_ack, ack);

    // Identity zero
    let mut bad_sub = submit;
    bad_sub.connection_epoch = 0;
    assert_eq!(
        encode_shell_file_submit(bad_sub).unwrap_err(),
        ShellFileCodecError::Identity
    );

    // Bounds candidate_bytes
    let mut bad_sub2 = submit;
    bad_sub2.candidate_bytes = 31;
    assert_eq!(
        encode_shell_file_submit(bad_sub2).unwrap_err(),
        ShellFileCodecError::Length
    );
    bad_sub2.candidate_bytes = 65537;
    assert_eq!(
        encode_shell_file_submit(bad_sub2).unwrap_err(),
        ShellFileCodecError::Length
    );
}
