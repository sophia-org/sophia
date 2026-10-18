use sophia_lock_client::{Candidate, protocol::lock_files::*};
fn resource() -> LockResourceId {
    LockResourceId {
        id: 2,
        generation: 3,
    }
}
fn step(total_bytes: Option<u64>) -> LockResourceStep {
    LockResourceStep {
        transaction: 4,
        resource: resource(),
        total_bytes,
    }
}
fn offer() -> LockNegotiate {
    LockNegotiate {
        minimum_revision: 1,
        maximum_revision: 1,
        requested_capabilities: 1,
        chords: vec![],
    }
}
#[test]
fn every_candidate_has_exact_identity_and_typed_body() {
    let candidates = [
        Candidate::Negotiate(offer()),
        Candidate::ResourceCancel(step(None)),
        Candidate::ResourceRetire(step(None)),
        Candidate::Present(LockCandidate {
            transaction: 4,
            lock_epoch: 5,
            output_id: 6,
            output_generation: 7,
            allocation_id: 8,
            allocation_generation: 9,
            candidate_generation: 10,
            pacing_permit: 11,
            resource: resource(),
        }),
        Candidate::FrameDemand(LockFrameDemand {
            transaction: 4,
            lock_epoch: 5,
            allocation_id: 8,
            allocation_generation: 9,
            demand_id: 12,
        }),
    ];
    for value in candidates {
        let p = value.prepare(17, 23).unwrap();
        let record = decode_lock_file_record(p.bytes(), LockFileClass::Candidate).unwrap();
        assert_eq!(record.header.connection_epoch, 17);
        assert_eq!(record.header.submission_id, 23);
        assert_eq!(record.header.sequence, 0);
        assert_eq!(record.header.kind, p.kind());
        assert_eq!(
            encode_lock_file_record(record.header, record.body).unwrap(),
            p.bytes()
        );
        match value {
            Candidate::Negotiate(v) => assert_eq!(LockNegotiate::decode(record.body).unwrap(), v),
            Candidate::ResourceBegin(v) => {
                assert_eq!(LockResourceBegin::decode(record.body).unwrap(), v)
            }
            Candidate::ResourceEnd(v) => {
                assert_eq!(LockResourceStep::decode(record.body, true).unwrap(), v)
            }
            Candidate::ResourceCancel(v) | Candidate::ResourceRetire(v) => {
                assert_eq!(LockResourceStep::decode(record.body, false).unwrap(), v)
            }
            Candidate::Present(v) => {
                assert_eq!(p.bytes().len(), 128);
                assert_eq!(LockCandidate::decode(record.body).unwrap(), v);
            }
            Candidate::FrameDemand(v) => {
                assert_eq!(LockFrameDemand::decode(record.body).unwrap(), v)
            }
        }
    }
}
#[test]
fn retired_byte_candidates_cannot_prepare_a_submission() {
    for value in [
        Candidate::ResourceBegin(LockResourceBegin {
            transaction: 4,
            resource: resource(),
            width_px: 8,
            height_px: 9,
            slot: 0,
        }),
        Candidate::ResourceEnd(step(Some(288))),
    ] {
        assert!(value.prepare(17, 23).is_err());
    }
}

#[test]
fn frame_demand_bytes_are_literal_and_independent_of_caller_mutation() {
    let mut value = Candidate::FrameDemand(LockFrameDemand {
        transaction: 4,
        lock_epoch: 5,
        allocation_id: 8,
        allocation_generation: 9,
        demand_id: 12,
    });
    let p = value.prepare(17, 23).unwrap();
    if let Candidate::FrameDemand(v) = &mut value {
        v.transaction = 99;
    }
    let mut expected = vec![72, 0, 0, 0, 1, 0, 7, 1];
    for n in [17u64, 23, 0, 4, 5, 8, 9, 12] {
        expected.extend(n.to_le_bytes());
    }
    assert_eq!(p.bytes(), expected);
}
#[test]
fn invalid_shapes_and_offers_are_rejected_without_claiming_authority() {
    assert!(Candidate::ResourceEnd(step(None)).prepare(1, 1).is_err());
    assert!(
        Candidate::ResourceCancel(step(Some(4)))
            .prepare(1, 1)
            .is_err()
    );
    assert!(
        Candidate::ResourceRetire(step(Some(4)))
            .prepare(1, 1)
            .is_err()
    );
    for (epoch, id) in [(0, 1), (1, 0)] {
        assert!(Candidate::Negotiate(offer()).prepare(epoch, id).is_err());
    }
    for caps in [0, 2, 5] {
        let mut v = offer();
        v.requested_capabilities = caps;
        assert!(Candidate::Negotiate(v).prepare(1, 1).is_err());
    }
    let mut v = offer();
    v.chords.push(LockChordRequest {
        keysym: 65,
        modifiers: 1,
    });
    assert!(Candidate::Negotiate(v.clone()).prepare(1, 1).is_err());
    v.requested_capabilities = 3;
    // Shift-only chord refusal belongs to the server; syntax survives encoding.
    assert!(Candidate::Negotiate(v).prepare(1, 1).is_ok());
    let mut v = offer();
    v.minimum_revision = 2;
    v.maximum_revision = 3;
    assert!(Candidate::Negotiate(v).prepare(1, 1).is_ok());
}
