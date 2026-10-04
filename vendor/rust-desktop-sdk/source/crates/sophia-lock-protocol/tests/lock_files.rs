//! Lock file rules the KDL states in prose rather than as field constraints.
use sophia_lock_protocol::BinaryCodecError;
use sophia_lock_protocol::lock_files::*;

fn allocation() -> LockAllocation {
    LockAllocation {
        output_id: 1,
        output_generation: 1,
        allocation_id: 2,
        allocation_generation: 1,
        pixel_width: 1920,
        pixel_height: 1080,
        scale_numerator: 1,
        scale_denominator: 1,
    }
}

fn lock(lock_epoch: u64, phase: LockPhase, allocations: usize) -> LockObject {
    LockObject {
        lock_epoch,
        topology_generation: 3,
        phase,
        allocations: vec![allocation(); allocations],
    }
}

#[test]
fn a_zero_lock_epoch_is_exactly_the_unlocked_phase() {
    assert!(lock(0, LockPhase::Unlocked, 0).encode().is_ok());
    assert!(lock(4, LockPhase::Unlocked, 0).encode().is_err());
    for phase in [LockPhase::Locking, LockPhase::Locked, LockPhase::Unlocking] {
        assert!(lock(0, phase, 0).encode().is_err(), "{phase:?}");
        assert!(lock(4, phase, 0).encode().is_ok(), "{phase:?}");
    }
    // The decoder holds the same rule against bytes it never encoded.
    let mut bytes = lock(4, LockPhase::Locked, 0).encode().unwrap();
    bytes[..8].fill(0);
    assert!(LockObject::decode(&bytes).is_err());
}

#[test]
fn allocations_exist_only_while_the_cover_is_drawn() {
    assert!(lock(4, LockPhase::Locking, 2).encode().is_ok());
    assert!(lock(4, LockPhase::Locked, 2).encode().is_ok());
    assert!(lock(4, LockPhase::Unlocking, 1).encode().is_err());
    assert!(lock(0, LockPhase::Unlocked, 1).encode().is_err());
    let mut bytes = lock(4, LockPhase::Locked, 1).encode().unwrap();
    bytes[16..18].copy_from_slice(&(LockPhase::Unlocking as u16).to_le_bytes());
    assert!(LockObject::decode(&bytes).is_err());
    assert!(LockPhase::Locked.covers() && LockPhase::Locking.covers());
    assert!(!LockPhase::Unlocking.covers() && !LockPhase::Unlocked.covers());
}

#[test]
fn a_lock_object_carries_every_allocation_it_counts() {
    let encoded = lock(4, LockPhase::Locked, 3).encode().unwrap();
    assert_eq!(
        LockObject::decode(&encoded).unwrap(),
        lock(4, LockPhase::Locked, 3)
    );
    assert!(LockObject::decode(&encoded[..encoded.len() - 1]).is_err());
    let mut longer = encoded.clone();
    longer.push(0);
    assert!(LockObject::decode(&longer).is_err());
    assert!(
        lock(4, LockPhase::Locked, LOCK_FILE_MAX_OUTPUTS + 1)
            .encode()
            .is_err()
    );
}

#[test]
fn a_shift_only_chord_is_well_formed_but_left_for_the_owner_to_refuse() {
    let chord = |modifiers| LockChordRequest {
        keysym: 0x61,
        modifiers,
    };
    let negotiate = |modifiers| LockNegotiate {
        minimum_revision: 1,
        maximum_revision: 1,
        requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT | LOCK_FILE_CAPABILITY_CHORDS,
        chords: vec![chord(modifiers)],
    };
    let shift_only = negotiate(0b0001).encode().unwrap();
    let decoded = LockNegotiate::decode(&shift_only).unwrap();
    assert!(!decoded.chords[0].has_non_shift_modifier());
    for modifiers in [0b0010, 0b0101, 0b1000, 0b1111] {
        assert!(chord(modifiers).has_non_shift_modifier(), "{modifiers:#b}");
    }
    assert!(negotiate(0).encode().is_err());
    assert!(negotiate(0b1_0000).encode().is_err());
    let mut too_many = negotiate(0b0100);
    too_many.chords = vec![chord(0b0100); LOCK_FILE_MAX_CHORDS + 1];
    assert!(too_many.encode().is_err());
}

#[test]
fn every_encoded_record_round_trips() {
    let resource = LockResourceId {
        id: 3,
        generation: 2,
    };
    let limits = LockFileLimits {
        max_outputs: 16,
        upload_slots: 4,
        max_chords: 0,
        max_width_px: 16_384,
        max_height_px: 16_384,
        max_resource_bytes: 1 << 30,
        max_live_resources: 32,
        journal_records: 256,
        journal_bytes: 65_536,
        assembly_timeout_ms: 12_000,
        ack_progress_timeout_ms: 2_000,
    };
    assert_eq!(
        LockFileLimits::decode(&limits.encode().unwrap()),
        Ok(limits)
    );
    let negotiated = LockNegotiated {
        granted_chords: 0,
        granted_capabilities: LOCK_FILE_CAPABILITY_PRESENT,
    };
    assert_eq!(
        LockNegotiated::decode(&negotiated.encode().unwrap()),
        Ok(negotiated)
    );
    let begin = LockResourceBegin {
        transaction: 1,
        resource,
        width_px: 3,
        height_px: 5,
        slot: 3,
    };
    assert_eq!(begin.total_bytes(), 60);
    assert_eq!(
        LockResourceBegin::decode(&begin.encode().unwrap()),
        Ok(begin)
    );
    let end = LockResourceStep {
        transaction: 1,
        resource,
        total_bytes: Some(60),
    };
    assert_eq!(
        LockResourceStep::decode(&end.encode().unwrap(), true),
        Ok(end)
    );
    // An end read as a cancel, or a cancel as an end, has the wrong length.
    assert!(LockResourceStep::decode(&end.encode().unwrap(), false).is_err());
    let cancel = LockResourceStep {
        total_bytes: None,
        ..end
    };
    assert!(LockResourceStep::decode(&cancel.encode().unwrap(), true).is_err());
    for entry in [
        LockEntryKind::Insert,
        LockEntryKind::Clear,
        LockEntryKind::Submit,
        LockEntryKind::Checking,
        LockEntryKind::Failed,
        LockEntryKind::Unavailable,
    ] {
        let record = LockEntry {
            lock_epoch: 7,
            entry,
            empty_after: false,
        };
        assert_eq!(LockEntry::decode(&record.encode().unwrap()), Ok(record));
    }
    for refusal in [
        LockRefusal::UnsupportedRevision,
        LockRefusal::PresentationRequired,
        LockRefusal::InvalidChord,
    ] {
        assert_eq!(LockRefusal::decode(&refusal.encode()), Ok(refusal));
    }
}

#[test]
fn submitted_names_only_candidate_kinds() {
    for kind in [LockFileKind::Lock, LockFileKind::Entry] {
        let submitted = LockSubmitted {
            submission_id: 1,
            candidate_kind: kind,
        };
        assert!(submitted.encode().is_err(), "{kind:?}");
    }
    let mut bytes = LockSubmitted {
        submission_id: 1,
        candidate_kind: LockFileKind::FrameDemand,
    }
    .encode()
    .unwrap();
    bytes[8..10].copy_from_slice(&257u16.to_le_bytes());
    assert_eq!(
        LockSubmitted::decode(&bytes),
        Err(BinaryCodecError::InvalidRecord("candidate_kind"))
    );
}

#[test]
fn candidates_over_the_candidate_bound_are_refused_whole() {
    let header = LockFileHeader {
        kind: LockFileKind::Negotiate,
        connection_epoch: 1,
        submission_id: 1,
        sequence: 0,
    };
    let body = vec![0; LOCK_FILE_MAX_CANDIDATE_BYTES - LOCK_FILE_HEADER_BYTES + 1];
    assert!(encode_lock_file_record(header, &body).is_err());
    let fits = encode_lock_file_record(header, &body[1..]).unwrap();
    assert!(decode_lock_file_record(&fits, LockFileClass::Candidate).is_ok());
    let mut over = fits.clone();
    over.push(0);
    let size = over.len() as u32;
    over[..4].copy_from_slice(&size.to_le_bytes());
    assert!(decode_lock_file_record(&over, LockFileClass::Candidate).is_err());
}
