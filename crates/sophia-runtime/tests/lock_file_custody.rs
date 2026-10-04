//! t294: custody of one lock provider connection. The provider negotiates,
//! uploads images and offers them for the allocations Session publishes, at
//! the pace Session permits. Nothing it submits enters, leaves or delays the
//! lock, and every candidate it offers is checked against the lock object,
//! its allocation, its image and its permit before Session sees it.
use std::time::{Duration, Instant};

use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::lock_files::*;
use sophia_runtime::lock_files::*;

const EPOCH: u64 = 7;
const LOCK: u64 = 4;
const QID: u64 = 90;

fn limits() -> LockFileLimits {
    LockFileLimits {
        max_outputs: 4,
        upload_slots: 2,
        max_chords: 2,
        max_width_px: 64,
        max_height_px: 64,
        max_resource_bytes: 64 * 64 * 4,
        max_live_resources: 3,
        journal_records: 16,
        journal_bytes: 4096,
        assembly_timeout_ms: 1000,
        ack_progress_timeout_ms: 2000,
    }
}

fn allocation(id: u64, generation: u64) -> LockAllocation {
    LockAllocation {
        output_id: id,
        output_generation: 1,
        allocation_id: 10 + id,
        allocation_generation: generation,
        pixel_width: 4,
        pixel_height: 2,
        scale_numerator: 1,
        scale_denominator: 1,
    }
}

fn locked(epoch: u64, generation: u64) -> LockObject {
    LockObject {
        lock_epoch: epoch,
        topology_generation: 3,
        phase: LockPhase::Locked,
        allocations: vec![allocation(1, generation)],
    }
}

const SUPER: u16 = 0b1000;
const RESERVED: LockChordRequest = LockChordRequest {
    keysym: 0xff1b,
    modifiers: 0b0110,
};

fn custody(lock: LockObject) -> LockFileCustody {
    LockFileCustody::new(LockFileSettings {
        epoch: EPOCH,
        limits: limits(),
        reserved_chords: vec![RESERVED],
        lock,
        lock_qid: QID,
    })
    .unwrap()
}

struct Provider {
    custody: LockFileCustody,
    submission: u64,
    read: u64,
    now: Instant,
}

impl Provider {
    fn new(lock: LockObject) -> Self {
        Self {
            custody: custody(lock),
            submission: 0,
            read: 0,
            now: Instant::now(),
        }
    }

    fn submit(&mut self, kind: LockFileKind, body: &[u8]) -> Result<(), Errno> {
        self.submission += 1;
        let bytes = encode_lock_file_record(
            LockFileHeader {
                kind,
                connection_epoch: EPOCH,
                submission_id: self.submission,
                sequence: 0,
            },
            body,
        )
        .unwrap();
        self.custody.submit(&bytes, self.now)
    }

    /// Every event journaled since the last call, as (kind, body).
    fn events(&mut self) -> Vec<(LockFileKind, Vec<u8>)> {
        let tail = self.custody.position().tail;
        if tail == self.read {
            return Vec::new();
        }
        let ReadOutcome::Ready(bytes) = self.custody.read(self.read, 1 << 16).unwrap() else {
            panic!("journal read not ready");
        };
        self.read += bytes.len() as u64;
        let mut events = Vec::new();
        let mut rest = bytes.as_slice();
        while !rest.is_empty() {
            let size = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
            let record = decode_lock_file_record(&rest[..size], LockFileClass::Event).unwrap();
            events.push((record.header.kind, record.body.to_vec()));
            rest = &rest[size..];
        }
        // As a provider does: acknowledge what was read.
        let sequence = self.custody.position().next_sequence - 1;
        self.custody
            .acknowledge(LockFileAck {
                connection_epoch: EPOCH,
                sequence,
            })
            .unwrap();
        events
    }

    /// Writes through the slot's current binding, or a stale one.
    fn write(&mut self, slot: u8, offset: u64, data: &[u8]) -> Result<u32, Errno> {
        let binding = self.custody.upload_binding(slot).unwrap_or(u64::MAX);
        self.custody.write_upload(slot, binding, offset, data)
    }

    fn kinds(&mut self) -> Vec<LockFileKind> {
        self.events().into_iter().map(|(kind, _)| kind).collect()
    }

    fn negotiate(&mut self, chords: Vec<LockChordRequest>) -> Result<(), Errno> {
        let body = LockNegotiate {
            minimum_revision: 1,
            maximum_revision: 1,
            requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT | LOCK_FILE_CAPABILITY_CHORDS,
            chords,
        }
        .encode()
        .unwrap();
        self.submit(LockFileKind::Negotiate, &body)
    }

    fn ready(lock: LockObject) -> Self {
        let mut provider = Self::new(lock);
        provider.negotiate(Vec::new()).unwrap();
        provider.events();
        assert!(matches!(
            provider.custody.take_inbound(),
            Some(LockInbound::Negotiated { .. })
        ));
        provider
    }

    fn begin(&mut self, id: u64, width_px: u32, height_px: u32, slot: u16) -> Result<(), Errno> {
        let body = LockResourceBegin {
            transaction: 100 + id,
            resource: resource(id),
            width_px,
            height_px,
            slot,
        }
        .encode()
        .unwrap();
        self.submit(LockFileKind::ResourceBegin, &body)
    }

    fn end(&mut self, id: u64, total_bytes: u64) -> Result<(), Errno> {
        let body = LockResourceStep {
            transaction: 100 + id,
            resource: resource(id),
            total_bytes: Some(total_bytes),
        }
        .encode()
        .unwrap();
        self.submit(LockFileKind::ResourceEnd, &body)
    }

    /// A whole 4x2 image in slot 0.
    fn upload(&mut self, id: u64) {
        self.begin(id, 4, 2, 0).unwrap();
        assert_eq!(self.write(0, 0, &[7; 32]), Ok(32));
        self.end(id, 32).unwrap();
        self.events();
        assert!(matches!(
            self.custody.take_inbound(),
            Some(LockInbound::ResourceReady { .. })
        ));
    }

    fn demand(&mut self, demand_id: u64) -> Result<(), Errno> {
        let body = LockFrameDemand {
            transaction: 200 + demand_id,
            lock_epoch: LOCK,
            allocation_id: 11,
            allocation_generation: 1,
            demand_id,
        }
        .encode()
        .unwrap();
        self.submit(LockFileKind::FrameDemand, &body)
    }

    fn permitted(&mut self, demand_id: u64) -> u64 {
        self.demand(demand_id).unwrap();
        assert!(matches!(
            self.custody.take_inbound(),
            Some(LockInbound::Demand(_))
        ));
        let permit = self
            .custody
            .permit(11, demand_id, Duration::from_millis(100), self.now)
            .unwrap();
        self.events();
        permit.pacing_permit
    }

    fn candidate(&mut self, generation: u64, permit: u64, id: u64) -> Result<(), Errno> {
        let body = candidate(generation, permit, id).encode().unwrap();
        self.submit(LockFileKind::Candidate, &body)
    }
}

fn resource(id: u64) -> LockResourceId {
    LockResourceId { id, generation: 1 }
}

fn candidate(generation: u64, permit: u64, id: u64) -> LockCandidate {
    LockCandidate {
        transaction: 300 + generation,
        lock_epoch: LOCK,
        output_id: 1,
        output_generation: 1,
        allocation_id: 11,
        allocation_generation: 1,
        candidate_generation: generation,
        pacing_permit: permit,
        resource: resource(id),
    }
}

fn outcome(events: &[(LockFileKind, Vec<u8>)]) -> LockCandidateOutcome {
    let (_, body) = events
        .iter()
        .find(|(kind, _)| *kind == LockFileKind::CandidateOutcome)
        .expect("an outcome");
    LockCandidateOutcome::decode(body).unwrap()
}

#[test]
fn negotiation_grants_present_and_the_requested_chords_then_publishes_the_lock() {
    let mut provider = Provider::new(locked(LOCK, 1));
    // Nothing but negotiation is accepted first.
    assert_eq!(provider.begin(1, 4, 2, 0), Err(Errno::EACCES));
    let chord = LockChordRequest {
        keysym: 0x62,
        modifiers: SUPER,
    };
    provider.negotiate(vec![chord]).unwrap();
    let events = provider.events();
    assert_eq!(
        events.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
        [
            LockFileKind::Submitted,
            LockFileKind::Negotiated,
            LockFileKind::ObjectPublished
        ]
    );
    let negotiated = LockNegotiated::decode(&events[1].1).unwrap();
    assert_eq!(negotiated.granted_chords, 1);
    assert_eq!(negotiated.granted_capabilities, 3);
    assert_eq!(
        LockObjectPublished::decode(&events[2].1).unwrap().qid_path,
        QID
    );
    match provider.custody.take_inbound() {
        Some(LockInbound::Negotiated { chords }) => assert_eq!(chords, [chord]),
        other => panic!("{other:?}"),
    }
    assert_eq!(provider.negotiate(Vec::new()), Err(Errno::EINVAL), "once");
}

#[test]
fn a_negotiation_session_cannot_honour_is_refused_and_ends_the_connection() {
    let shift_only = LockChordRequest {
        keysym: 0x61,
        modifiers: 0b0001,
    };
    let cases = [
        (
            2,
            3,
            LOCK_FILE_CAPABILITY_PRESENT,
            vec![],
            LockRefusal::UnsupportedRevision,
        ),
        (
            1,
            1,
            LOCK_FILE_CAPABILITY_CHORDS,
            vec![],
            LockRefusal::PresentationRequired,
        ),
        (1, 1, 3, vec![shift_only], LockRefusal::InvalidChord),
        (1, 1, 3, vec![RESERVED], LockRefusal::InvalidChord),
        (
            1,
            1,
            LOCK_FILE_CAPABILITY_PRESENT,
            vec![LockChordRequest {
                keysym: 0x62,
                modifiers: SUPER,
            }],
            LockRefusal::InvalidChord,
        ),
    ];
    for (minimum, maximum, capabilities, chords, expected) in cases {
        let mut provider = Provider::new(locked(LOCK, 1));
        let body = LockNegotiate {
            minimum_revision: minimum,
            maximum_revision: maximum,
            requested_capabilities: capabilities,
            chords,
        }
        .encode()
        .unwrap();
        provider.submit(LockFileKind::Negotiate, &body).unwrap();
        let events = provider.events();
        assert_eq!(events[1].0, LockFileKind::Refused);
        assert_eq!(LockRefusal::decode(&events[1].1), Ok(expected));
        assert!(provider.custody.take_inbound().is_none());
        // Reading and acknowledging the refusal ends the connection.
        assert!(provider.custody.is_revoked(), "{expected:?}");
        assert_eq!(provider.begin(1, 4, 2, 0), Err(Errno::ESTALE));
    }
}

#[test]
fn an_exact_repeat_changes_nothing_and_an_older_submission_is_stale() {
    let mut provider = Provider::ready(locked(LOCK, 1));
    let begin = LockResourceBegin {
        transaction: 1,
        resource: resource(1),
        width_px: 4,
        height_px: 2,
        slot: 0,
    }
    .encode()
    .unwrap();
    let record = |submission_id, body: &[u8]| {
        encode_lock_file_record(
            LockFileHeader {
                kind: LockFileKind::ResourceBegin,
                connection_epoch: EPOCH,
                submission_id,
                sequence: 0,
            },
            body,
        )
        .unwrap()
    };
    let now = provider.now;
    provider.custody.submit(&record(5, &begin), now).unwrap();
    let journaled = provider.custody.position().next_sequence;
    provider.custody.submit(&record(5, &begin), now).unwrap();
    assert_eq!(provider.custody.position().next_sequence, journaled);
    let mut other = begin.clone();
    other[24] = 8;
    assert_eq!(
        provider.custody.submit(&record(5, &other), now),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        provider.custody.submit(&record(4, &begin), now),
        Err(Errno::ESTALE)
    );
    let mut foreign = record(6, &begin);
    foreign[8..16].copy_from_slice(&(EPOCH + 1).to_le_bytes());
    assert_eq!(provider.custody.submit(&foreign, now), Err(Errno::ESTALE));
}

#[test]
fn an_upload_becomes_an_image_only_when_every_byte_arrived() {
    let mut provider = Provider::ready(locked(LOCK, 1));
    provider.begin(1, 4, 2, 0).unwrap();
    let events = provider.events();
    let status = LockResourceStatus::decode(&events[1].1).unwrap();
    assert_eq!(status.status, LockResourceState::Admitted);
    assert_eq!(status.admitted_bytes, 32);
    let first = provider.custody.upload_binding(0).unwrap();
    assert_eq!(provider.write(0, 0, &[1; 20]), Ok(20));
    assert_eq!(
        provider.write(0, 0, &[1; 4]),
        Err(Errno::EINVAL),
        "only at the cursor"
    );
    assert_eq!(
        provider.write(0, 20, &[1; 13]),
        Err(Errno::EINVAL),
        "never past the declared size"
    );
    assert_eq!(provider.write(1, 0, &[1]), Err(Errno::ESTALE));
    // Short: refused, and the slot is free again.
    provider.end(1, 32).unwrap();
    let status = LockResourceStatus::decode(&provider.events()[1].1).unwrap();
    assert_eq!(
        (status.status, status.reason),
        (LockResourceState::Rejected, reason::SIZE_MISMATCH)
    );
    assert!(provider.custody.upload_binding(0).is_none());
    assert!(provider.custody.take_inbound().is_none());

    provider.begin(2, 4, 2, 0).unwrap();
    assert_eq!(
        provider.custody.write_upload(0, first, 0, &[9; 32]),
        Err(Errno::ESTALE),
        "a writer of the earlier binding is fenced"
    );
    provider.write(0, 0, &[9; 32]).unwrap();
    provider.end(2, 32).unwrap();
    let events = provider.events();
    let status = LockResourceStatus::decode(&events.last().unwrap().1).unwrap();
    assert_eq!(status.status, LockResourceState::Accepted);
    match provider.custody.take_inbound() {
        Some(LockInbound::ResourceReady {
            resource: ready,
            width_px: 4,
            height_px: 2,
            pixels,
        }) => {
            assert_eq!(ready, resource(2));
            assert_eq!(&*pixels, &[9; 32]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn image_storage_is_reused_only_after_session_lets_it_go() {
    let mut provider = Provider::ready(locked(LOCK, 1));
    let ready = |provider: &mut Provider, id: u64, fill: u8| {
        provider.begin(id, 4, 2, 0).unwrap();
        assert_eq!(provider.write(0, 0, &[fill; 32]), Ok(32));
        provider.end(id, 32).unwrap();
        provider.events();
        loop {
            match provider.custody.take_inbound() {
                Some(LockInbound::ResourceReady { pixels, .. }) => break pixels,
                Some(_) => continue,
                None => panic!("upload {id} became no image"),
            }
        }
    };
    let retire = |provider: &mut Provider, id: u64| {
        let step = LockResourceStep {
            transaction: 200 + id,
            resource: resource(id),
            total_bytes: None,
        }
        .encode()
        .unwrap();
        provider
            .submit(LockFileKind::ResourceRetire, &step)
            .unwrap();
        provider.events();
        while provider.custody.take_inbound().is_some() {}
    };
    let first = ready(&mut provider, 1, 1);
    let shown = first.as_ptr();
    retire(&mut provider, 1);
    // Session still shows the retired image: its storage is not reused.
    let second = ready(&mut provider, 2, 2);
    assert_ne!(second.as_ptr(), shown);
    assert_eq!(&*first, &[1; 32], "a shown image never changes");
    drop(first);
    // Once Session lets it go, the next upload of that size writes in place.
    let third = ready(&mut provider, 3, 3);
    assert_eq!(third.as_ptr(), shown);
    assert_eq!(&*third, &[3; 32]);
    assert_eq!(&*second, &[2; 32]);
}

#[test]
fn storage_kept_for_reuse_stays_within_the_live_limit() {
    let mut provider = Provider::ready(locked(LOCK, 1));
    let ready = |provider: &mut Provider, id: u64, width_px: u32| {
        let len = width_px as usize * 2 * 4;
        provider.begin(id, width_px, 2, 0).unwrap();
        assert_eq!(provider.write(0, 0, &vec![1; len]), Ok(len as u32));
        provider.end(id, len as u64).unwrap();
        provider.events();
        loop {
            match provider.custody.take_inbound() {
                Some(LockInbound::ResourceReady { pixels, .. }) => break pixels,
                Some(_) => continue,
                None => panic!("upload {id} became no image"),
            }
        }
    };
    let retire = |provider: &mut Provider, id: u64| {
        let step = LockResourceStep {
            transaction: 200 + id,
            resource: resource(id),
            total_bytes: None,
        }
        .encode()
        .unwrap();
        provider
            .submit(LockFileKind::ResourceRetire, &step)
            .unwrap();
        provider.events();
        while provider.custody.take_inbound().is_some() {}
    };
    // Three retired images, released by Session, all kept for reuse. The
    // weak references only observe them.
    let kept: Vec<_> = (1..=3)
        .map(|id| {
            let pixels = ready(&mut provider, id, 4);
            let observed = std::sync::Arc::downgrade(&pixels);
            drop(pixels);
            retire(&mut provider, id);
            observed
        })
        .collect();
    assert!(kept.iter().all(|storage| storage.strong_count() == 1));
    // Three live images of another size fill the limit: none of the kept
    // storage may stay beside them.
    for id in 4..=6 {
        drop(ready(&mut provider, id, 2));
    }
    assert!(
        kept.iter().all(|storage| storage.strong_count() == 0),
        "spare storage beyond the live limit was kept"
    );
}

#[test]
fn uploads_are_bounded_by_the_epoch_limits() {
    let mut provider = Provider::ready(locked(LOCK, 1));
    for (width, height) in [(65, 1), (1, 65)] {
        provider.begin(1, width, height, 0).unwrap();
        let status = LockResourceStatus::decode(&provider.events()[1].1).unwrap();
        assert_eq!(
            (status.status, status.reason),
            (LockResourceState::Rejected, reason::BUDGET)
        );
    }
    assert_eq!(
        provider.begin(1, 4, 2, 2),
        Err(Errno::EINVAL),
        "no such slot"
    );
    provider.begin(1, 4, 2, 0).unwrap();
    assert_eq!(provider.begin(2, 4, 2, 0), Err(Errno::EINVAL), "slot busy");
    assert_eq!(
        provider.begin(1, 4, 2, 1),
        Err(Errno::EINVAL),
        "same resource"
    );
    provider.events();
    // Three live resources at most, uploads included.
    provider.write(0, 0, &[0; 32]).unwrap();
    provider.end(1, 32).unwrap();
    provider.upload(2);
    provider.upload(3);
    provider.begin(4, 4, 2, 0).unwrap();
    let status = LockResourceStatus::decode(&provider.events()[1].1).unwrap();
    assert_eq!(status.reason, reason::BUDGET);
    // Retiring one makes room, and Session is told.
    let retire = LockResourceStep {
        transaction: 9,
        resource: resource(2),
        total_bytes: None,
    }
    .encode()
    .unwrap();
    provider
        .submit(LockFileKind::ResourceRetire, &retire)
        .unwrap();
    assert_eq!(provider.events()[1].0, LockFileKind::ResourceReleased);
    while let Some(inbound) = provider.custody.take_inbound() {
        if let LockInbound::ResourceRetired(retired) = inbound {
            assert_eq!(retired, resource(2));
        }
    }
    provider.begin(4, 4, 2, 0).unwrap();
    let status = LockResourceStatus::decode(&provider.events()[1].1).unwrap();
    assert_eq!(status.status, LockResourceState::Admitted);
    assert_eq!(
        provider.submit(LockFileKind::ResourceRetire, &retire),
        Err(Errno::EINVAL),
        "already retired"
    );
}

#[test]
fn a_permitted_candidate_for_the_current_allocation_reaches_session() {
    let mut provider = Provider::ready(locked(LOCK, 1));
    provider.upload(1);
    let permit = provider.permitted(1);
    provider.candidate(1, permit, 1).unwrap();
    assert_eq!(provider.kinds(), [LockFileKind::Submitted]);
    match provider.custody.take_inbound() {
        Some(LockInbound::Candidate {
            candidate: sent,
            pixels,
        }) => {
            assert_eq!(sent, candidate(1, permit, 1));
            assert_eq!(&*pixels, &[7; 32]);
        }
        other => panic!("{other:?}"),
    }
    // The permit granted one candidate.
    provider.candidate(2, permit, 1).unwrap();
    assert_eq!(outcome(&provider.events()).reason, reason::PERMIT);
    // Session reports the outcome; an unknown one is refused.
    let presented = LockCandidateOutcome {
        transaction: 301,
        lock_epoch: LOCK,
        output_id: 1,
        allocation_id: 11,
        candidate_generation: 1,
        status: LockCandidateStatus::Presented,
        reason: reason::NONE,
    };
    provider.custody.outcome(presented).unwrap();
    assert_eq!(outcome(&provider.events()), presented);
    assert_eq!(provider.custody.outcome(presented), Err(Errno::EINVAL));
}

#[test]
fn a_candidate_that_does_not_match_what_session_granted_is_rejected() {
    let mut provider = Provider::ready(locked(LOCK, 1));
    provider.upload(1);
    provider.begin(2, 2, 2, 0).unwrap();
    provider.write(0, 0, &[0; 16]).unwrap();
    provider.end(2, 16).unwrap();
    provider.events();
    while provider.custody.take_inbound().is_some() {}
    let permit = provider.permitted(1);
    let mut generation = 0;
    let mut reject = |provider: &mut Provider, sent: LockCandidate, expected: u16| {
        generation += 1;
        let sent = LockCandidate {
            candidate_generation: generation,
            ..sent
        };
        provider
            .submit(LockFileKind::Candidate, &sent.encode().unwrap())
            .unwrap();
        let outcome = outcome(&provider.events());
        assert_eq!(
            (outcome.status, outcome.reason),
            (LockCandidateStatus::Rejected, expected)
        );
        assert!(provider.custody.take_inbound().is_none());
    };
    let good = candidate(1, permit, 1);
    reject(
        &mut provider,
        LockCandidate {
            lock_epoch: LOCK + 1,
            ..good
        },
        reason::STALE_LOCK,
    );
    reject(
        &mut provider,
        LockCandidate {
            allocation_generation: 2,
            ..good
        },
        reason::STALE_ALLOCATION,
    );
    reject(
        &mut provider,
        LockCandidate {
            output_id: 2,
            ..good
        },
        reason::STALE_ALLOCATION,
    );
    reject(
        &mut provider,
        LockCandidate {
            resource: resource(9),
            ..good
        },
        reason::UNKNOWN_RESOURCE,
    );
    reject(
        &mut provider,
        LockCandidate {
            resource: resource(2),
            ..good
        },
        reason::SIZE_MISMATCH,
    );
    reject(
        &mut provider,
        LockCandidate {
            pacing_permit: permit + 1,
            ..good
        },
        reason::PERMIT,
    );
    // Generations only move forward.
    assert_eq!(
        provider.candidate(generation, permit, 1),
        Err(Errno::EINVAL)
    );
}

#[test]
fn permits_expire_and_answer_only_the_standing_demand() {
    let mut provider = Provider::ready(locked(LOCK, 1));
    provider.upload(1);
    provider.demand(1).unwrap();
    // A newer demand replaces the standing one.
    provider.demand(2).unwrap();
    let now = provider.now;
    assert_eq!(
        provider
            .custody
            .permit(11, 1, Duration::from_millis(50), now),
        Err(Errno::EINVAL)
    );
    assert_eq!(
        provider
            .custody
            .permit(11, 2, Duration::from_millis(251), now),
        Err(Errno::EINVAL),
        "at most 250 ms"
    );
    let permit = provider
        .custody
        .permit(11, 2, Duration::from_millis(50), now)
        .unwrap();
    assert_eq!(
        provider
            .custody
            .permit(11, 2, Duration::from_millis(50), now),
        Err(Errno::EINVAL),
        "one permit per demand"
    );
    provider.events();
    provider.now += Duration::from_millis(60);
    provider.candidate(1, permit.pacing_permit, 1).unwrap();
    assert_eq!(outcome(&provider.events()).reason, reason::PERMIT);
    // A demand for an allocation Session did not grant is stale.
    let body = LockFrameDemand {
        transaction: 9,
        lock_epoch: LOCK,
        allocation_id: 99,
        allocation_generation: 1,
        demand_id: 3,
    }
    .encode()
    .unwrap();
    assert_eq!(
        provider.submit(LockFileKind::FrameDemand, &body),
        Err(Errno::ESTALE)
    );
}

#[test]
fn a_new_lock_object_revokes_what_it_no_longer_grants() {
    let mut provider = Provider::ready(locked(LOCK, 1));
    provider.upload(1);
    let permit = provider.permitted(1);
    provider.candidate(1, permit, 1).unwrap();
    provider.demand(2).unwrap();
    provider.events();
    while provider.custody.take_inbound().is_some() {}
    // The lock moves to a new topology generation for the allocation.
    let generation = provider
        .custody
        .publish_lock(locked(LOCK, 2), QID + 1)
        .unwrap();
    assert_eq!(generation, 2);
    let events = provider.events();
    assert_eq!(
        events.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
        [
            LockFileKind::CandidateOutcome,
            LockFileKind::ObjectPublished
        ]
    );
    let revoked = outcome(&events);
    assert_eq!(
        (revoked.status, revoked.candidate_generation),
        (LockCandidateStatus::Revoked, 1)
    );
    // The lapsed demand can no longer be answered.
    let now = provider.now;
    assert_eq!(
        provider
            .custody
            .permit(11, 2, Duration::from_millis(50), now),
        Err(Errno::EINVAL)
    );
    // Unlocked: nothing is granted, so nothing is presented.
    let unlocked = LockObject {
        lock_epoch: 0,
        topology_generation: 3,
        phase: LockPhase::Unlocked,
        allocations: Vec::new(),
    };
    provider.custody.publish_lock(unlocked, QID + 2).unwrap();
    provider.events();
    provider.candidate(5, permit, 1).unwrap();
    assert_eq!(outcome(&provider.events()).reason, reason::STALE_LOCK);
}

#[test]
fn entries_and_chords_reach_only_a_negotiated_provider() {
    let mut provider = Provider::new(locked(LOCK, 1));
    let entry = LockEntry {
        lock_epoch: LOCK,
        entry: LockEntryKind::Insert,
        empty_after: false,
    };
    assert_eq!(provider.custody.entry(entry), Err(Errno::EACCES));
    provider
        .negotiate(vec![LockChordRequest {
            keysym: 0x62,
            modifiers: SUPER,
        }])
        .unwrap();
    provider.events();
    provider.custody.entry(entry).unwrap();
    assert_eq!(provider.kinds(), [LockFileKind::Entry]);
    provider
        .custody
        .chord(LockChord {
            lock_epoch: LOCK,
            chord: 0,
        })
        .unwrap();
    assert_eq!(
        provider.custody.chord(LockChord {
            lock_epoch: LOCK,
            chord: 1,
        }),
        Err(Errno::EINVAL),
        "only granted chords"
    );
}

#[test]
fn no_provider_record_names_the_lock_state() {
    // Records of every class but candidates are refused as submissions, so
    // the provider cannot publish a lock object, an entry or a permit.
    let mut provider = Provider::ready(locked(LOCK, 1));
    for kind in [
        LockFileKind::Lock,
        LockFileKind::Limits,
        LockFileKind::Entry,
        LockFileKind::FramePermit,
        LockFileKind::CandidateOutcome,
    ] {
        provider.submission += 1;
        let header = LockFileHeader {
            kind,
            connection_epoch: EPOCH,
            submission_id: provider.submission,
            sequence: 0,
        };
        // The encoder itself refuses these identities; forge the bytes.
        let mut bytes = encode_lock_file_record(
            LockFileHeader {
                kind: LockFileKind::Negotiate,
                ..header
            },
            &[0; 16],
        )
        .unwrap();
        bytes[6..8].copy_from_slice(&(kind as u16).to_le_bytes());
        let now = provider.now;
        assert_eq!(
            provider.custody.submit(&bytes, now),
            Err(Errno::EINVAL),
            "{kind:?}"
        );
    }
    assert_eq!(provider.custody.lock().0, &locked(LOCK, 1));
}

#[test]
fn forwarded_work_holds_room_for_its_answer() {
    let mut provider = Provider::ready(locked(LOCK, 1));
    provider.upload(1);
    let permit = provider.permitted(1);
    provider.candidate(1, permit, 1).unwrap();
    provider.demand(2).unwrap();
    // Fill the journal with entries until it refuses one.
    let entry = LockEntry {
        lock_epoch: LOCK,
        entry: LockEntryKind::Insert,
        empty_after: false,
    };
    while provider.custody.entry(entry).is_ok() {}
    // The reserved answers still fit.
    let now = provider.now;
    provider
        .custody
        .permit(11, 2, Duration::from_millis(50), now)
        .unwrap();
    provider
        .custody
        .outcome(LockCandidateOutcome {
            transaction: 301,
            lock_epoch: LOCK,
            output_id: 1,
            allocation_id: 11,
            candidate_generation: 1,
            status: LockCandidateStatus::Superseded,
            reason: reason::NONE,
        })
        .unwrap();
    // And nothing new is forwarded without room.
    let before = provider.custody.position();
    assert_eq!(provider.demand(3), Err(Errno::EAGAIN));
    assert_eq!(provider.custody.position(), before, "nothing journaled");
}

#[test]
fn a_provider_that_stops_acknowledging_is_revoked() {
    let mut provider = Provider::ready(locked(LOCK, 1));
    // An event the provider never acknowledges starts its clock.
    provider
        .custody
        .entry(LockEntry {
            lock_epoch: LOCK,
            entry: LockEntryKind::Insert,
            empty_after: false,
        })
        .unwrap();
    provider
        .custody
        .expire(Instant::now() + Duration::from_millis(1_900));
    assert!(!provider.custody.is_revoked());
    let later = Instant::now() + Duration::from_millis(2_100);
    provider.custody.expire(later);
    assert!(provider.custody.is_revoked());
    assert_eq!(provider.demand(1), Err(Errno::ESTALE));
}
