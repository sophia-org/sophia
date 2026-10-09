//! t306: characterize a provider candidate across real lock-object publication.
//! This is a custody/pacing test, not proof that returning native heads retire.
use std::time::{Duration, Instant};

use sophia_9p::{Errno, ReadOutcome};
use sophia_engine::{SessionLockEpoch, SessionLockImageIdentity};
use sophia_protocol::lock_files::*;
use sophia_protocol::{DisplayHeadId, OutputId, Size};
use sophia_runtime::lock_files::{LockFileCustody, LockFileSettings, LockInbound};

use super::super::lock_provider::LockPublication;
use crate::session_lock::SessionLockPhase;
use crate::session_lock_frames::SessionLockFrames;

const CONNECTION: u64 = 7;
const LOCK: u64 = 3;

fn topology(epoch: u64, outputs: &[(u64, u64)]) -> sophia_protocol::OutputAuthoritySnapshot {
    let mut snapshot = super::snapshot(epoch);
    let mut head = snapshot.heads.pop().unwrap();
    let mut group = snapshot.groups.pop().unwrap();
    head.modes[0].pixel_size = Size {
        width: 4,
        height: 2,
    };
    group.logical.width = 4;
    group.logical.height = 2;
    for &(id, generation) in outputs {
        head.head = DisplayHeadId::from_raw(id);
        group.output = OutputId::from_raw(id);
        group.generation = generation;
        group.members[0].head = head.head;
        snapshot.heads.push(head.clone());
        snapshot.groups.push(group.clone());
    }
    snapshot
}

struct Fixture {
    publication: LockPublication,
    custody: LockFileCustody,
    frames: SessionLockFrames,
    object: LockObject,
    now: Instant,
    submission: u64,
    read: u64,
}

impl Fixture {
    fn new() -> Self {
        let snapshot = topology(1, &[(1, 1), (2, 1)]);
        let mut publication = LockPublication::default();
        let object = publication
            .update(super::locked(None), Some(1), || Some(snapshot.clone()))
            .unwrap();
        let custody = LockFileCustody::new(LockFileSettings {
            epoch: CONNECTION,
            limits: crate::session_lock_object::session_lock_file_limits(Some(&snapshot)),
            reserved_chords: Vec::new(),
            lock: object.clone(),
            lock_qid: 90,
        })
        .unwrap();
        let mut frames = SessionLockFrames::default();
        frames.lock(SessionLockEpoch::from_raw(LOCK));
        frames.connected(CONNECTION);
        frames.set_pacing_diagnostics(true);
        frames.retain_pacing(&object.allocations);
        let mut fixture = Self {
            publication,
            custody,
            frames,
            object,
            now: Instant::now(),
            submission: 0,
            read: 0,
        };
        fixture.submit(
            LockFileKind::Negotiate,
            &LockNegotiate {
                minimum_revision: 1,
                maximum_revision: 1,
                requested_capabilities: LOCK_FILE_CAPABILITY_PRESENT,
                chords: Vec::new(),
            }
            .encode()
            .unwrap(),
        );
        assert!(matches!(
            fixture.custody.take_inbound(),
            Some(LockInbound::Negotiated { .. })
        ));
        fixture.events();
        fixture.upload();
        fixture
    }

    fn submit(&mut self, kind: LockFileKind, body: &[u8]) {
        self.submission += 1;
        let bytes = encode_lock_file_record(
            LockFileHeader {
                kind,
                connection_epoch: CONNECTION,
                submission_id: self.submission,
                sequence: 0,
            },
            body,
        )
        .unwrap();
        self.custody.submit(&bytes, self.now).unwrap();
    }

    fn events(&mut self) -> Vec<(LockFileKind, Vec<u8>)> {
        if self.read == self.custody.position().tail {
            return Vec::new();
        }
        let ReadOutcome::Ready(bytes) = self.custody.read(self.read, 1 << 16).unwrap() else {
            panic!("journal not ready");
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
        self.custody
            .acknowledge(LockFileAck {
                connection_epoch: CONNECTION,
                sequence: self.custody.position().next_sequence - 1,
            })
            .unwrap();
        events
    }

    fn upload(&mut self) {
        self.submit(
            LockFileKind::ResourceBegin,
            &LockResourceBegin {
                transaction: 100,
                resource: resource(),
                width_px: 4,
                height_px: 2,
                slot: 0,
            }
            .encode()
            .unwrap(),
        );
        let binding = self.custody.upload_binding(0).unwrap();
        assert_eq!(self.custody.write_upload(0, binding, 0, &[7; 32]), Ok(32));
        self.submit(
            LockFileKind::ResourceEnd,
            &LockResourceStep {
                transaction: 100,
                resource: resource(),
                total_bytes: Some(32),
            }
            .encode()
            .unwrap(),
        );
        let Some(LockInbound::ResourceReady {
            resource,
            width_px,
            height_px,
            pixels,
        }) = self.custody.take_inbound()
        else {
            panic!("custody must admit the uploaded image");
        };
        self.frames
            .resource_ready(CONNECTION, resource, width_px, height_px, pixels);
        self.events();
    }

    fn allocation(&self, output: u64) -> LockAllocation {
        *self
            .object
            .allocations
            .iter()
            .find(|a| a.output_id == output)
            .unwrap()
    }

    fn demand(&mut self, output: u64, id: u64) -> LockFrameDemand {
        let allocation = self.allocation(output);
        let demand = LockFrameDemand {
            transaction: 200 + id,
            lock_epoch: self.object.lock_epoch,
            allocation_id: allocation.allocation_id,
            allocation_generation: allocation.allocation_generation,
            demand_id: id,
        };
        self.submit(LockFileKind::FrameDemand, &demand.encode().unwrap());
        let Some(LockInbound::Demand(admitted)) = self.custody.take_inbound() else {
            panic!("custody must admit the demand");
        };
        assert_eq!(admitted, demand);
        self.frames.demand(CONNECTION, admitted);
        self.events();
        demand
    }

    fn place(&mut self, output: u64) -> LockCandidate {
        let demand = self.demand(output, 1);
        assert_eq!(self.frames.permits(), [demand]);
        let permit = self
            .custody
            .permit(
                demand.allocation_id,
                demand.demand_id,
                Duration::from_millis(100),
                self.now,
            )
            .unwrap();
        self.events();
        let a = self.allocation(output);
        let candidate = LockCandidate {
            transaction: 301,
            lock_epoch: self.object.lock_epoch,
            output_id: a.output_id,
            output_generation: a.output_generation,
            allocation_id: a.allocation_id,
            allocation_generation: a.allocation_generation,
            candidate_generation: 1,
            pacing_permit: permit.pacing_permit,
            resource: resource(),
        };
        self.submit(LockFileKind::Candidate, &candidate.encode().unwrap());
        let Some(LockInbound::Candidate {
            candidate: admitted,
            ..
        }) = self.custody.take_inbound()
        else {
            panic!("custody must admit the candidate");
        };
        assert_eq!(admitted, candidate);
        let (changed, owed) = self.frames.candidate(CONNECTION, admitted);
        assert!(changed && owed.is_empty());
        self.events();
        candidate
    }

    fn publish(&mut self, epoch: u64, outputs: &[(u64, u64)]) -> Vec<LockCandidateOutcome> {
        let snapshot = topology(epoch, outputs);
        self.object = self
            .publication
            .update(super::locked(None), Some(epoch), || Some(snapshot))
            .unwrap();
        // These are the owner's production operations; retain_pacing only
        // changes diagnostics. There is no inbound revocation to deliver.
        self.custody
            .publish_lock(self.object.clone(), 90 + epoch)
            .unwrap();
        self.frames.retain_pacing(&self.object.allocations);
        assert!(self.custody.take_inbound().is_none());
        let events = self.events();
        assert_eq!(events.last().unwrap().0, LockFileKind::ObjectPublished);
        events
            .into_iter()
            .filter_map(|(kind, body)| {
                (kind == LockFileKind::CandidateOutcome)
                    .then(|| LockCandidateOutcome::decode(&body).unwrap())
            })
            .collect()
    }

    fn shown(&self) -> (SessionLockImageIdentity, u64) {
        let images = self.frames.images();
        let placement = &images[&OutputId::from_raw(1)];
        (placement.image.identity, placement.generation)
    }
}

fn resource() -> LockResourceId {
    LockResourceId {
        id: 1,
        generation: 1,
    }
}

#[test]
fn publication_revokes_custody_but_retirement_of_the_old_image_releases_session_pacing() {
    let mut f = Fixture::new();
    let candidate = f.place(1);
    let shown = f.shown();
    let revoked = f.publish(2, &[(2, 1)]);
    assert_eq!(revoked.len(), 1);
    assert_eq!(
        revoked[0].candidate_generation,
        candidate.candidate_generation
    );
    assert_eq!(revoked[0].status, LockCandidateStatus::Revoked);
    assert_eq!(
        f.frames.waiting().collect::<Vec<_>>(),
        [OutputId::from_raw(1)]
    );
    assert_eq!(
        f.shown(),
        shown,
        "publication does not withdraw Session's old image"
    );
    assert!(f.publish(3, &[(1, 2), (2, 1)]).is_empty());
    let demand = f.demand(1, 2);
    assert_eq!(demand.allocation_generation, 2);
    assert!(
        f.frames.permits().is_empty(),
        "the older generation still paces this allocation id"
    );

    // Supplying the real all-head retirement witness releases it. This test
    // does not claim that a native topology transition supplies that witness.
    let outcome = f
        .frames
        .presented(OutputId::from_raw(1), Some(shown))
        .unwrap();
    assert_eq!(outcome.status, LockCandidateStatus::Presented);
    assert_eq!(outcome.transaction, candidate.transaction);
    assert_eq!(outcome.candidate_generation, candidate.candidate_generation);
    assert_eq!(
        f.custody.outcome(outcome),
        Err(Errno::EINVAL),
        "custody has already revoked it"
    );
    assert_eq!(f.frames.permits(), [demand]);
}

#[test]
fn a_withdrawn_pending_output_does_not_block_another_outputs_permit() {
    let mut f = Fixture::new();
    f.place(1);
    f.publish(2, &[(2, 1)]);
    let demand = f.demand(2, 2);
    assert_eq!(f.frames.permits(), [demand]);
    assert_eq!(
        f.frames.waiting().collect::<Vec<_>>(),
        [OutputId::from_raw(1)]
    );
}

#[test]
fn an_output_returning_without_a_pending_candidate_receives_a_permit() {
    let mut f = Fixture::new();
    assert!(f.publish(2, &[]).is_empty());
    assert!(f.frames.waiting().next().is_none());
    assert!(f.publish(3, &[(1, 2)]).is_empty());
    let demand = f.demand(1, 2);
    assert_eq!(f.frames.permits(), [demand]);
}

#[test]
fn wrong_retirement_identity_or_candidate_generation_cannot_release_the_wait() {
    let mut f = Fixture::new();
    f.place(1);
    f.publish(2, &[]);
    f.publish(3, &[(1, 2)]);
    f.demand(1, 2);
    let (identity, generation) = f.shown();
    for wrong in [
        (
            SessionLockImageIdentity {
                connection_epoch: CONNECTION + 1,
                ..identity
            },
            generation,
        ),
        (
            SessionLockImageIdentity {
                output: OutputId::from_raw(2),
                ..identity
            },
            generation,
        ),
        (
            SessionLockImageIdentity {
                resource_id: 2,
                ..identity
            },
            generation,
        ),
        (
            SessionLockImageIdentity {
                resource_generation: 2,
                ..identity
            },
            generation,
        ),
        (identity, generation + 1),
    ] {
        assert!(
            f.frames
                .presented(OutputId::from_raw(1), Some(wrong))
                .is_none()
        );
        assert!(f.frames.permits().is_empty());
    }
}

#[test]
fn resource_retirement_keeps_the_placed_image_and_its_pending_retirement() {
    let mut f = Fixture::new();
    f.place(1);
    let shown = f.shown();
    f.publish(2, &[]);
    f.submit(
        LockFileKind::ResourceRetire,
        &LockResourceStep {
            transaction: 400,
            resource: resource(),
            total_bytes: None,
        }
        .encode()
        .unwrap(),
    );
    let Some(LockInbound::ResourceRetired(resource)) = f.custody.take_inbound() else {
        panic!("resource retirement must reach Session");
    };
    f.frames.resource_retired(CONNECTION, resource);
    assert_eq!(f.shown(), shown);
    assert_eq!(
        f.frames.images()[&OutputId::from_raw(1)]
            .image
            .pixels
            .as_ref(),
        &[7; 32]
    );
    assert!(
        f.frames
            .presented(OutputId::from_raw(1), Some(shown))
            .is_some()
    );
}

#[test]
fn a_new_lock_or_provider_connection_clears_the_withdrawn_candidate() {
    for reconnect in [false, true] {
        let mut f = Fixture::new();
        f.place(1);
        f.publish(2, &[]);
        if reconnect {
            assert!(f.frames.connected(CONNECTION + 1));
        } else {
            let epoch = SessionLockEpoch::from_raw(LOCK + 1).unwrap();
            let object = f
                .publication
                .update(
                    SessionLockPhase::Locked {
                        epoch,
                        attempt: None,
                    },
                    Some(2),
                    || Some(topology(2, &[])),
                )
                .unwrap();
            f.custody.publish_lock(object, 100).unwrap();
            assert!(f.frames.lock(Some(epoch)));
        }
        assert!(f.frames.waiting().next().is_none());
        assert!(f.frames.images().is_empty());
        assert!(f.frames.permits().is_empty());
    }
}
