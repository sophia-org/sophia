#![cfg(feature = "experimental-native-frame-transaction")]

//! W2 prototype, store level: a candidate's retire-on-present intent is
//! reserved at End, owned by the exact candidate identity, cancelled by every
//! other outcome at once, and committed once. The transport's FIFO transfer
//! is the only production commit point; these fixtures stand in for it with
//! `commit_retire` before taking the queued Presented.
use sophia_protocol::*;
use sophia_runtime::*;
#[path = "support/content_isb.rs"]
mod isb;

fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    }
}

fn tx(value: u64) -> TransactionId {
    TransactionId::from_raw(value)
}

const A: ContentOutputId = ContentOutputId {
    id: 2,
    generation: 3,
};
const B: ContentOutputId = ContentOutputId {
    id: 3,
    generation: 4,
};

fn resource(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}

fn allocation(output: ContentOutputId) -> ContentAllocationSnapshot {
    ContentAllocationSnapshot {
        native_opening: None,
        output,
        allocation: ContentAllocationId {
            id: output.id,
            generation: 1,
        },
        scale_generation: 4,
        scale_numerator: 1,
        scale_denominator: 1,
        role: 1,
        edge: 1,
        margins: ContentMargins::default(),
        logical: ContentLogicalRect {
            x: 0,
            y: 0,
            width: 64,
            height: 32,
        },
        pixel: ContentPixelRect {
            x: 0,
            y: 0,
            width: 64,
            height: 32,
        },
        parent: ContentAllocationId::default(),
        anchor_parent_rect: ContentPixelRect::default(),
        allowed_reservation_extent: 32,
    }
}

fn retire(transaction: u64, id: u64) -> ContentRetireOnPresent {
    ContentRetireOnPresent {
        retire_transaction: tx(transaction),
        resource: resource(id),
    }
}

fn owner(output: ContentOutputId, generation: u64) -> ContentRetireOwner {
    ContentRetireOwner {
        grant: grant(),
        output,
        candidate_generation: generation,
    }
}

struct Fixture {
    pool: ContentEpochPool,
    allocations: [ContentAllocationSnapshot; 2],
    now: u64,
}

impl Fixture {
    fn new(max_retiring_bytes: u64) -> Self {
        let mut limits = ContentLimits::prototype(grant());
        limits.max_retiring_bytes = max_retiring_bytes;
        limits.max_resource_bytes = limits.max_resource_bytes.min(max_retiring_bytes);
        let mut pool = ContentEpochPool::new(limits.max_session_retiring_bytes).unwrap();
        pool.admit(limits).unwrap();
        let mut fixture = Self {
            pool,
            allocations: [allocation(A), allocation(B)],
            now: 0,
        };
        for id in 1..=4 {
            fixture.upload(id);
        }
        fixture
    }

    fn upload(&mut self, id: u64) {
        let resources = self.pool.active_mut().unwrap();
        isb::accept(
            resources,
            tx(1),
            ContentResourceBegin {
                grant: grant(),
                resource: resource(id),
                width_px: 2,
                height_px: 1,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 8,
            },
            &[0; 8],
            0,
        );
        while resources.take_event().is_some() {}
    }

    fn tick(&mut self) -> u64 {
        self.now += 1;
        self.now
    }

    fn resources(&self) -> &ContentResourceStore {
        self.pool.active().unwrap()
    }

    fn candidates(&mut self) -> &mut ContentCandidateStore {
        self.pool.active_candidates_mut().unwrap()
    }

    /// Permit, Begin and one Chunk drawing `draws` on `output`, then End
    /// carrying `intent`. The candidate's own transaction is always 11.
    fn frame(
        &mut self,
        output: ContentOutputId,
        generation: u64,
        draws: u64,
        intent: Option<ContentRetireOnPresent>,
    ) -> Result<(), ContentCandidateError> {
        let now = self.tick();
        self.candidates()
            .grant_permit(tx(10), output, generation, generation, now)
            .unwrap();
        let now = self.tick();
        self.candidates()
            .begin(
                tx(11),
                ContentCandidateBegin {
                    grant: grant(),
                    candidate_generation: generation,
                    output,
                    facts_generation: 6,
                    pacing_permit: generation,
                    interaction_generation: 8,
                    surface_count: 1,
                    placement_count: 1,
                    target_count: 1,
                },
                now,
            )
            .unwrap();
        let allocation = ContentAllocationId {
            id: output.id,
            generation: 1,
        };
        let now = self.tick();
        self.candidates()
            .chunk(
                tx(12),
                ContentCandidateChunk {
                    grant: grant(),
                    candidate_generation: generation,
                    chunk_ordinal: 0,
                    surfaces: vec![ContentSurface {
                        allocation,
                        scale_generation: 4,
                        role: 1,
                        edge: 1,
                        margins: ContentMargins::default(),
                        reservation_extent: 24,
                        parent_surface_index: u16::MAX,
                        anchor_parent_rect: ContentPixelRect::default(),
                    }],
                    placements: vec![ContentPlacement {
                        resource: resource(draws),
                        surface_index: 0,
                        destination_x_px: 3,
                        destination_y_px: 4,
                    }],
                    targets: vec![ContentTarget {
                        surface_index: 0,
                        action_kind: 1,
                        target_id: 1,
                        target_generation: 1,
                        action_id: 1,
                        bounds_px: ContentPixelRect {
                            x: 3,
                            y: 4,
                            width: 2,
                            height: 1,
                        },
                    }],
                },
                now,
            )
            .unwrap();
        let now = self.tick();
        let end = ContentCandidateEnd {
            grant: grant(),
            candidate_generation: generation,
            surface_count: 1,
            placement_count: 1,
            target_count: 1,
        };
        let context = ContentCandidateContext {
            output,
            facts_generation: 6,
            interaction_generation: 8,
            allocations: &self.allocations,
        };
        let (resources, candidates) = self.pool.active_pair_mut().unwrap();
        match intent {
            Some(intent) => {
                candidates.end_with_retire(tx(13), end, intent, context, resources, now)
            }
            None => candidates.end(tx(13), end, context, resources, now),
        }
    }

    /// Submit, prepare and present; the bundle is returned to model the
    /// renderer's lease.
    fn present(&mut self, output: ContentOutputId, generation: u64) -> ContentRenderBundle {
        let now = self.tick();
        let bundle = self
            .candidates()
            .begin_submission(output, generation, now)
            .unwrap();
        let now = self.tick();
        self.candidates()
            .prepared(output, generation, 20, 30, now)
            .unwrap();
        self.candidates()
            .presented(output, generation, 40, 20, 30)
            .unwrap();
        bundle
    }

    /// The transport's transfer: for each queued event in order, commit a
    /// Presented's intent while it still holds it, then take the event.
    fn transfer_candidate_events(&mut self) -> Vec<ContentCandidateEvent> {
        let mut taken = Vec::new();
        while let Some(owner) = self.candidates().pending_event().map(|event| event.retire) {
            if let Some(owner) = owner {
                self.pool.active_mut().unwrap().commit_retire(&owner);
            }
            taken.push(self.candidates().take_event().unwrap());
        }
        taken
    }

    fn released(&mut self) -> Vec<(TransactionId, ContentResourceId, u16)> {
        let resources = self.pool.active_mut().unwrap();
        std::iter::from_fn(|| resources.take_event())
            .filter_map(|event| match event.record {
                ShellContentRecord::ResourceReleased(value) => {
                    Some((event.transaction, value.resource, value.reason))
                }
                _ => None,
            })
            .collect()
    }

    fn explicit(&mut self, transaction: u64, id: u64) -> Result<(), ContentStoreError> {
        self.pool.active_mut().unwrap().retire(
            tx(transaction),
            &ContentResourceRetire {
                grant: grant(),
                resource: resource(id),
            },
        )
    }
}

fn retire_record(id: u64) -> ShellContentRecord {
    ShellContentRecord::ResourceRetire(ContentResourceRetire {
        grant: grant(),
        resource: resource(id),
    })
}

#[test]
fn one_candidate_transaction_on_two_outputs_keeps_two_exact_owners() {
    let mut f = Fixture::new(16 * 1024);
    // Both candidates use transaction 11 and draw resource 3.
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    f.frame(B, 2, 3, Some(retire(51, 2))).unwrap();
    assert_eq!(f.resources().reserved_retiring(), 16);
    // A live retire ID is not reusable by another candidate.
    let mut g = Fixture::new(16 * 1024);
    g.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    assert_eq!(
        g.frame(B, 2, 3, Some(retire(50, 2))),
        Err(ContentCandidateError::Stale)
    );
    assert_eq!(g.resources().reserved_retiring(), 8);

    let _bundle = f.present(A, 1);
    f.transfer_candidate_events();
    assert_eq!(f.resources().reserved_retiring(), 8);
    assert_eq!(f.resources().usage().retiring, 8);
    assert_eq!(f.released(), [(tx(50), resource(1), 0)]);
    // B's intent is untouched by A's commit.
    assert!(!f.pool.active_mut().unwrap().commit_retire(&owner(A, 1)));
    assert!(
        f.resources().lease(grant(), resource(2)).is_err(),
        "B still holds its intent"
    );
}

#[test]
fn a_second_intent_for_one_predecessor_rejects_the_whole_candidate() {
    let mut f = Fixture::new(16 * 1024);
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    f.transfer_candidate_events();
    let occupancy = f.pool.accounting().response_records;
    assert_eq!(
        f.frame(B, 2, 4, Some(retire(51, 1))),
        Err(ContentCandidateError::Stale)
    );
    let events = f.transfer_candidate_events();
    assert!(events.iter().any(|event| matches!(
        event.record,
        ShellContentRecord::CandidateOutcome(ContentCandidateOutcome {
            candidate_generation: 2,
            kind: 3,
            ..
        })
    )));
    assert_eq!(f.candidates().pending_candidate_count(), 1);
    assert_eq!(f.resources().reserved_retiring(), 8);
    assert_eq!(f.pool.accounting().response_records, occupancy);
    // Nor may a candidate retire what it draws, or name its own transaction.
    assert_eq!(
        f.frame(B, 3, 2, Some(retire(51, 2))),
        Err(ContentCandidateError::Stale)
    );
    assert_eq!(
        f.frame(B, 4, 4, Some(retire(11, 2))),
        Err(ContentCandidateError::Stale)
    );
    assert_eq!(f.resources().reserved_retiring(), 8);
}

#[test]
fn a_named_resource_refuses_new_leases_and_explicit_retire_with_its_own_credit() {
    let mut f = Fixture::new(16 * 1024);
    assert_eq!(
        f.resources()
            .additional_response_credit(tx(60), &retire_record(1)),
        0
    );
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    // Every new lease is denied from End, on every output.
    assert_eq!(
        f.resources().lease(grant(), resource(1)).err(),
        Some(ContentStoreError::Stale)
    );
    assert_eq!(f.frame(B, 2, 1, None), Err(ContentCandidateError::Stale));
    // The conflicting retire's error status needs its own credit.
    assert_eq!(
        f.resources()
            .additional_response_credit(tx(60), &retire_record(1)),
        1
    );
    assert_eq!(f.explicit(60, 1), Err(ContentStoreError::Stale));
    assert_eq!(f.resources().reserved_retiring(), 8);
    assert_eq!(f.resources().usage().retiring, 0);
}

#[test]
fn reserved_bytes_refuse_a_competing_explicit_retire_and_commit_needs_no_headroom() {
    let mut f = Fixture::new(8);
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    assert_eq!(
        f.resources()
            .additional_response_credit(tx(60), &retire_record(2)),
        1,
        "a refusal caused by the reservation is paid for before dequeue"
    );
    assert_eq!(f.explicit(60, 2), Err(ContentStoreError::Budget));
    // A second intent is refused by the same sum.
    assert_eq!(
        f.frame(B, 2, 4, Some(retire(51, 2))),
        Err(ContentCandidateError::Budget)
    );
    let _bundle = f.present(A, 1);
    f.transfer_candidate_events();
    assert_eq!(f.resources().reserved_retiring(), 0);
    assert_eq!(f.resources().usage().retiring, 8);
}

#[test]
fn every_outcome_but_presented_lets_go_at_once_without_a_collect() {
    // Supersession: the replacement reuses the predecessor and the retire ID.
    let mut f = Fixture::new(16 * 1024);
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    f.frame(A, 2, 3, Some(retire(50, 1))).unwrap();
    assert_eq!(f.resources().reserved_retiring(), 8);

    // Pending timeout inside the next admission's own expiry: a plain End
    // may lease the predecessor, and a later intent may name it again.
    let mut f = Fixture::new(16 * 1024);
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    f.now += 2_000;
    f.frame(B, 2, 1, None).unwrap();
    assert_eq!(f.resources().reserved_retiring(), 0);
    f.now += 2_000;
    f.frame(B, 3, 3, Some(retire(50, 1))).unwrap();
    assert_eq!(f.resources().reserved_retiring(), 8);

    // Renderer failure after submission.
    let mut f = Fixture::new(16 * 1024);
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    let now = f.tick();
    let bundle = f.candidates().begin_submission(A, 1, now).unwrap();
    assert_eq!(
        f.resources().reserved_retiring(),
        8,
        "submitted still owns it"
    );
    f.candidates().renderer_failed(A, 1).unwrap();
    assert_eq!(f.resources().reserved_retiring(), 0);
    assert!(f.resources().lease(grant(), resource(1)).is_ok());
    assert_eq!(f.explicit(50, 2), Ok(()), "the retire ID is free again");
    drop(bundle);
    assert_eq!(f.resources().usage().retiring, 8);
}

#[test]
fn a_queued_presented_owns_the_intent_until_its_transfer_commits_it_once() {
    let mut f = Fixture::new(16 * 1024);
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    f.transfer_candidate_events();
    let bundle = f.present(A, 1);
    // Blocked before transfer: collect keeps the reservation.
    f.pool.collect();
    assert_eq!(f.resources().reserved_retiring(), 8);
    let prepared = f.candidates().take_event().unwrap();
    assert!(matches!(
        prepared.record,
        ShellContentRecord::CandidateOutcome(ContentCandidateOutcome { kind: 1, .. })
    ));
    // An inspection clone of the queued Presented owns nothing.
    let peeked = f.candidates().pending_event().cloned().unwrap();
    assert_eq!(peeked.retire, Some(owner(A, 1)));
    drop(peeked);
    f.pool.collect();
    assert_eq!(f.resources().reserved_retiring(), 8);
    assert!(f.released().is_empty());
    let events = f.transfer_candidate_events();
    assert!(matches!(
        events.last().unwrap().record,
        ShellContentRecord::CandidateOutcome(ContentCandidateOutcome { kind: 2, .. })
    ));
    assert!(!f.pool.active_mut().unwrap().commit_retire(&owner(A, 1)));
    assert_eq!(f.resources().usage().retiring, 8);
    assert_eq!(f.released(), [(tx(50), resource(1), 0)]);
    drop(bundle);
}

#[test]
fn taking_a_presented_without_its_commit_cancels_and_never_activates() {
    let mut f = Fixture::new(16 * 1024);
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    let _bundle = f.present(A, 1);
    while f.candidates().take_event().is_some() {}
    assert!(!f.pool.active_mut().unwrap().commit_retire(&owner(A, 1)));
    assert_eq!(f.resources().reserved_retiring(), 0);
    assert_eq!(f.resources().usage().retiring, 0);
    assert!(f.resources().lease(grant(), resource(1)).is_ok());
}

#[test]
fn a_wrong_profile_store_refuses_end_with_retire_before_consuming_anything() {
    let limits = ContentLimits::prototype(grant());
    let mut resources = ContentResourceStore::new(limits.clone()).unwrap();
    for profile in [
        ContentStoreProfile::NativeLauncher,
        ContentStoreProfile::PersistentCatalog,
    ] {
        let mut candidates = ContentCandidateStore::with_profile(limits.clone(), profile).unwrap();
        let end = ContentCandidateEnd {
            grant: grant(),
            candidate_generation: 1,
            surface_count: 0,
            placement_count: 0,
            target_count: 0,
        };
        let context = ContentCandidateContext {
            output: A,
            facts_generation: 1,
            interaction_generation: 1,
            allocations: &[],
        };
        assert_eq!(
            candidates.end_with_retire(tx(13), end, retire(50, 1), context, &mut resources, 100),
            Err(ContentCandidateError::Malformed)
        );
        // Time did not move: an earlier clock still admits a permit.
        candidates.grant_permit(tx(10), A, 1, 1, 0).unwrap();
        assert!(candidates.take_event().is_some());
    }
    assert_eq!(resources.reserved_retiring(), 0);
}

#[test]
fn a_held_retire_id_refuses_an_explicit_retire_in_either_order_until_released() {
    // Intent first: an unrelated explicit retire may not reuse its ID.
    let mut f = Fixture::new(16 * 1024);
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    let record = retire_record(2);
    assert_eq!(f.resources().additional_response_credit(tx(50), &record), 1);
    assert_eq!(f.resources().additional_response_credit(tx(51), &record), 0);
    assert_eq!(f.explicit(50, 2), Err(ContentStoreError::Stale));
    // Still held after commit, until the release.
    let bundle = f.present(A, 1);
    f.transfer_candidate_events();
    assert_eq!(f.explicit(50, 2), Err(ContentStoreError::Stale));
    assert_eq!(f.released(), [(tx(50), resource(1), 0)]);
    f.pool
        .active_mut()
        .unwrap()
        .confirm_isb_release(resource(1))
        .unwrap();
    assert_eq!(f.explicit(50, 2), Ok(()));
    drop(bundle);

    // Explicit first: an intent may not take an unreleased retire's ID.
    let mut f = Fixture::new(16 * 1024);
    f.explicit(50, 2).unwrap();
    // Resource 2 is releasable at once; hold its release journal pending.
    assert_eq!(
        f.frame(A, 1, 3, Some(retire(50, 1))),
        Err(ContentCandidateError::Stale)
    );
    assert_eq!(f.released(), [(tx(50), resource(2), 0)]);
    f.pool
        .active_mut()
        .unwrap()
        .confirm_isb_release(resource(2))
        .unwrap();
    f.frame(A, 2, 3, Some(retire(50, 1))).unwrap();
    assert_eq!(f.resources().reserved_retiring(), 8);
}

#[test]
fn a_lease_that_outlives_presentation_delays_release_until_a_later_collect() {
    let mut f = Fixture::new(16 * 1024);
    // P draws 1 and stays with the renderer; S draws 2 and retires 1.
    f.frame(A, 1, 1, None).unwrap();
    let held = f.present(A, 1);
    f.transfer_candidate_events();
    f.frame(A, 2, 2, Some(retire(50, 1))).unwrap();
    let _shown = f.present(A, 2);
    f.transfer_candidate_events();
    assert_eq!(f.resources().usage().retiring, 8);
    assert!(f.released().is_empty(), "the renderer still holds P");
    drop(held);
    f.pool.collect();
    assert_eq!(f.released(), [(tx(50), resource(1), 0)]);
}

#[test]
fn revocation_cancels_uncommitted_intents_and_keeps_committed_ones() {
    // Pending, submitted and queued-Presented intents die with the grant and
    // release on the original transfer transaction.
    for stage in 0..3 {
        let mut f = Fixture::new(16 * 1024);
        f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
        let bundle = match stage {
            0 => None,
            1 => {
                let now = f.tick();
                Some(f.candidates().begin_submission(A, 1, now).unwrap())
            }
            _ => Some(f.present(A, 1)),
        };
        let (resources, candidates) = f.pool.active_pair_mut().unwrap();
        candidates.revoke();
        resources.revoke();
        assert_eq!(resources.reserved_retiring(), 0, "stage {stage}");
        let released = f.released();
        assert!(released.contains(&(tx(1), resource(1), ContentReason::Revoked as u16)));
        assert!(
            !released
                .iter()
                .any(|(transaction, _, _)| *transaction == tx(50))
        );
        drop(bundle);
    }
    // A committed intent is an ordinary retire: released on its retire ID.
    let mut f = Fixture::new(16 * 1024);
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    let bundle = f.present(A, 1);
    f.transfer_candidate_events();
    let mut released = f.released();
    let (resources, candidates) = f.pool.active_pair_mut().unwrap();
    candidates.revoke();
    resources.revoke();
    released.extend(f.released());
    assert!(released.contains(&(tx(50), resource(1), 0)));
    drop(bundle);
}

#[test]
fn a_late_completion_after_disconnect_neither_commits_nor_touches_the_replacement() {
    let mut f = Fixture::new(16 * 1024);
    f.frame(A, 1, 3, Some(retire(50, 1))).unwrap();
    let now = f.tick();
    let bundle = f.candidates().begin_submission(A, 1, now).unwrap();
    f.candidates().prepared(A, 1, 20, 30, now).unwrap();
    f.pool.disconnect();
    let mut limits = ContentLimits::prototype(ContentGrant {
        connection_epoch: 8,
        content_grant_epoch: 10,
    });
    limits.max_retiring_bytes = 16 * 1024;
    limits.max_resource_bytes = 16 * 1024;
    f.pool.admit(limits).unwrap();
    // The old candidate completes on its retained, revoked store.
    f.pool
        .candidates_mut(grant())
        .unwrap()
        .presented(A, 1, 40, 20, 30)
        .unwrap();
    assert!(
        f.pool
            .candidates_mut(grant())
            .unwrap()
            .take_event()
            .is_none()
    );
    assert_eq!(f.resources().reserved_retiring(), 0);
    assert!(
        f.resources().quiescent(),
        "the replacement grant is untouched"
    );
    drop(bundle);
    f.pool.collect();
}
