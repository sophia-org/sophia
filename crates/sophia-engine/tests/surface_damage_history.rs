use sophia_engine::{
    CompositorDisplayCommand, CompositorDisplayList, HeadlessOutput, OutputFrameDamageSnapshot,
    SURFACE_DAMAGE_BYTES, SurfaceDamageHistory, SurfaceDamageIdentity, output_frame_damage,
    output_frame_damage_snapshot,
};
use sophia_protocol::{
    BufferSource, CommittedSurfaceState, OutputId, Rect, Region, Size, SurfaceContentSet, SurfaceId,
};

#[test]
fn oversized_candidate_is_refused_before_any_provenance_is_discarded() {
    let history = SurfaceDamageHistory::default();
    let candidate: Vec<_> = (0..=sophia_engine::MAX_OUTPUT_FRAME_SURFACES)
        .map(|index| {
            let mut surface = state(1, vec![rect(1)]);
            surface.surface = SurfaceId::new(index as u32 + 1, 1);
            surface
        })
        .collect();
    assert_eq!(
        history.for_candidate(&[], &candidate, None).unwrap_err(),
        sophia_engine::OutputFrameDamageError::SurfaceCapacityExceeded,
    );
}
fn rect(x: i32) -> Rect {
    Rect {
        x,
        y: 1,
        width: 2,
        height: 3,
    }
}
fn state(generation: u64, damage: Vec<Rect>) -> CommittedSurfaceState {
    let size = Size {
        width: 64,
        height: 64,
    };
    let source = BufferSource::DmaBuf {
        handle: generation + 1,
    };
    let mut variant = SurfaceContentSet::singleton(source, size)
        .canonical_variant()
        .clone();
    variant.damage = Region { rects: damage };
    CommittedSurfaceState {
        surface: SurfaceId::new(1, 1),
        committed_generation: generation,
        geometry: Rect {
            x: 10,
            y: 20,
            width: 64,
            height: 64,
        },
        content: SurfaceContentSet::new(size, vec![variant]).unwrap(),
        damage: Region::empty(),
    }
}
fn primed_history(initial: &CommittedSurfaceState) -> SurfaceDamageHistory {
    let mut history = SurfaceDamageHistory::default();
    let mut before = initial.clone();
    before.committed_generation -= 1;
    history.record_committed(
        &[before],
        std::slice::from_ref(initial),
        &SurfaceDamageIdentity::default(),
    );
    history
}
fn tracked_snapshot(
    history: &SurfaceDamageHistory,
    state: &CommittedSurfaceState,
) -> OutputFrameDamageSnapshot {
    let mut snapshot = snapshot(state);
    snapshot.damage_history = history
        .for_candidate(
            std::slice::from_ref(state),
            std::slice::from_ref(state),
            None,
        )
        .unwrap();
    snapshot
}
fn snapshot(state: &CommittedSurfaceState) -> OutputFrameDamageSnapshot {
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size: Size {
            width: 128,
            height: 128,
        },
        scale: 1,
    };
    output_frame_damage_snapshot(
        output,
        CompositorDisplayList {
            output: output.id,
            commands: vec![CompositorDisplayCommand::Surface {
                surface: state.surface,
            }],
        },
        std::slice::from_ref(state),
        None,
    )
    .unwrap()
}
#[test]
fn reused_buffer_accumulates_every_committed_delta_plus_its_candidate() {
    let a = state(1, vec![rect(1)]);
    let b = state(2, vec![rect(4)]);
    let c = state(3, vec![rect(12)]);
    let mut history = primed_history(&a);
    let original = tracked_snapshot(&history, &a);
    history.record_committed(
        std::slice::from_ref(&a),
        std::slice::from_ref(&b),
        &SurfaceDamageIdentity::default(),
    );
    let bytes = history.retained_bytes();
    let mut candidate = snapshot(&c);
    candidate.damage_history = history
        .for_candidate(&[b], &[c], Some(&SurfaceDamageIdentity::default()))
        .unwrap();
    let damage = output_frame_damage(Some(&original), &candidate).unwrap();
    assert_eq!(
        damage.rects,
        vec![
            Rect {
                x: 14,
                y: 21,
                width: 2,
                height: 3
            },
            Rect {
                x: 22,
                y: 21,
                width: 2,
                height: 3
            }
        ]
    );
    assert_eq!(
        history.retained_bytes(),
        bytes,
        "candidate did not commit or evict history"
    );
}
#[test]
fn discarded_candidate_cannot_contaminate_the_next_one() {
    let a = state(1, vec![rect(1)]);
    let rejected = state(2, vec![rect(4)]);
    let replacement = state(2, vec![rect(12)]);
    let history = primed_history(&a);
    let original = tracked_snapshot(&history, &a);
    let discarded = history
        .for_candidate(
            std::slice::from_ref(&a),
            &[rejected],
            Some(&SurfaceDamageIdentity::default()),
        )
        .unwrap();
    let mut next = snapshot(&replacement);
    next.damage_history = history
        .for_candidate(
            std::slice::from_ref(&a),
            &[replacement],
            Some(&SurfaceDamageIdentity::default()),
        )
        .unwrap();
    assert_ne!(next.damage_history, discarded);
    assert_eq!(
        output_frame_damage(Some(&original), &next).unwrap().rects,
        vec![Rect {
            x: 22,
            y: 21,
            width: 2,
            height: 3
        }]
    );
}

#[test]
fn a_regressed_unassociated_view_cannot_reuse_an_old_committed_endpoint() {
    let a = state(1, vec![rect(1)]);
    let b = state(2, vec![rect(4)]);
    let c = state(3, vec![rect(12)]);
    let mut history = primed_history(&a);
    let original = tracked_snapshot(&history, &a);
    for (before, after) in [(&a, &b), (&b, &c)] {
        history.record_committed(
            std::slice::from_ref(before),
            std::slice::from_ref(after),
            &SurfaceDamageIdentity::default(),
        );
    }
    // A complete chain to the actual committed endpoint stays precise.
    let committed = tracked_snapshot(&history, &c);
    assert_eq!(
        output_frame_damage(Some(&original), &committed)
            .unwrap()
            .rects,
        vec![
            Rect {
                x: 14,
                y: 21,
                width: 2,
                height: 3
            },
            Rect {
                x: 22,
                y: 21,
                width: 2,
                height: 3
            },
        ]
    );
    // The unassociated view names the old buffer and generation, but its
    // origin identity does not prove that it contains those committed pixels.
    let mut regressed = snapshot(&b);
    regressed.damage_history = history.for_candidate(&[c], &[b], None).unwrap();
    assert_eq!(
        output_frame_damage(Some(&original), &regressed)
            .unwrap()
            .rects,
        vec![a.geometry, a.geometry],
        "a chain must reach the target preparation, not just its public fields"
    );
}
#[test]
fn missing_or_evicted_generations_and_complex_damage_fall_back() {
    let a = state(1, vec![rect(1)]);
    let gap = state(3, vec![rect(4)]);
    let mut history = SurfaceDamageHistory::default();
    let mut next = snapshot(&gap);
    next.damage_history = history
        .for_candidate(
            std::slice::from_ref(&a),
            &[gap],
            Some(&SurfaceDamageIdentity::default()),
        )
        .unwrap();
    assert_eq!(
        output_frame_damage(Some(&snapshot(&a)), &next)
            .unwrap()
            .rects,
        vec![a.geometry, a.geometry]
    );
    let complex = state(2, vec![rect(1); 33]);
    next = snapshot(&complex);
    next.damage_history = history
        .for_candidate(
            std::slice::from_ref(&a),
            &[complex],
            Some(&SurfaceDamageIdentity::default()),
        )
        .unwrap();
    assert_eq!(
        output_frame_damage(Some(&snapshot(&a)), &next)
            .unwrap()
            .rects,
        vec![a.geometry, a.geometry]
    );
    let mut previous = a.clone();
    for generation in 2..=19 {
        let current = state(generation, vec![rect(1)]);
        history.record_committed(
            &[previous],
            std::slice::from_ref(&current),
            &SurfaceDamageIdentity::default(),
        );
        previous = current;
    }
    next = snapshot(&previous);
    next.damage_history = history
        .for_candidate(
            std::slice::from_ref(&previous),
            std::slice::from_ref(&previous),
            None,
        )
        .unwrap();
    assert_eq!(next.damage_history.len(), 16);
    assert_eq!(
        output_frame_damage(Some(&snapshot(&a)), &next)
            .unwrap()
            .rects,
        vec![a.geometry, a.geometry]
    );
    history.record_committed(&[], &[], &SurfaceDamageIdentity::default());
    assert_eq!(history.retained_bytes(), 0);
}
#[test]
fn history_budget_is_bounded_and_missing_edges_remain_full() {
    let mut history = SurfaceDamageHistory::default();
    let mut previous: Vec<_> = (0..1024)
        .map(|id| {
            let mut state = state(1, vec![rect(1); 32]);
            state.surface = SurfaceId::new(id + 1, 1);
            state
        })
        .collect();
    for generation in 2..=18 {
        let next: Vec<_> = previous
            .iter()
            .map(|old| {
                let mut s = state(generation, vec![rect(1); 32]);
                s.surface = old.surface;
                s
            })
            .collect();
        history.record_committed(&previous, &next, &SurfaceDamageIdentity::default());
        assert!(history.retained_bytes() <= SURFACE_DAMAGE_BYTES);
        if generation == 18 {
            // Every pending endpoint survives global history eviction, including
            // early surfaces whose history would be trimmed by a flat prefix.
            let rejected_id = SurfaceDamageIdentity::default();
            let rejected_history = history
                .for_candidate(&previous, &next, Some(&rejected_id))
                .unwrap();
            let actual_id = SurfaceDamageIdentity::default();
            let actual_history = history
                .for_candidate(&previous, &next, Some(&actual_id))
                .unwrap();
            for state in &next {
                let mut rejected = snapshot(state);
                rejected.damage_history = rejected_history.clone();
                let mut actual = snapshot(state);
                actual.damage_history = actual_history.clone();
                assert!(
                    !output_frame_damage(Some(&rejected), &actual)
                        .unwrap()
                        .rects
                        .is_empty(),
                    "memory pressure lost pending identity for {:?}",
                    state.surface
                );
            }
        }
        previous = next;
    }
}

#[test]
fn rejected_pixels_cannot_alias_a_commit_with_the_same_generation_and_buffer() {
    let a = state(1, vec![rect(1)]);
    let b = state(2, vec![rect(4)]);
    let c = state(3, vec![rect(12)]);
    let mut history = primed_history(&a);
    let mut rejected_slot = snapshot(&b);
    rejected_slot.damage_history = history
        .for_candidate(
            std::slice::from_ref(&a),
            std::slice::from_ref(&b),
            Some(&SurfaceDamageIdentity::default()),
        )
        .unwrap();
    let actual_identity = SurfaceDamageIdentity::default();
    history.record_committed(
        std::slice::from_ref(&a),
        std::slice::from_ref(&b),
        &actual_identity,
    );
    let actual_slot = tracked_snapshot(&history, &b);
    // Identical public state, but the rejected preparation's pixels have no
    // proven relationship to this commit, even when its damage is identical.
    assert_eq!(rejected_slot.surfaces, actual_slot.surfaces);
    assert_eq!(
        output_frame_damage(Some(&rejected_slot), &actual_slot)
            .unwrap()
            .rects,
        vec![b.geometry, b.geometry]
    );
    let mut next = snapshot(&c);
    next.damage_history = history
        .for_candidate(&[b], &[c], Some(&SurfaceDamageIdentity::default()))
        .unwrap();
    assert_eq!(
        output_frame_damage(Some(&rejected_slot), &next)
            .unwrap()
            .rects,
        vec![a.geometry, a.geometry]
    );
    assert_eq!(
        output_frame_damage(Some(&actual_slot), &next)
            .unwrap()
            .rects,
        vec![Rect {
            x: 22,
            y: 21,
            width: 2,
            height: 3
        }]
    );
}

#[test]
fn prepared_commit_identity_survives_commit_but_not_repreparation_or_restore() {
    use sophia_engine::{HeadlessEngine, ProductionSessionCoordinator};
    use sophia_protocol::{
        AuthorityKind, SurfaceTransaction, SurfaceTransactionReadiness, TransactionId,
        TransactionOutcome,
    };
    let a = state(1, vec![rect(1)]);
    let b = state(2, vec![rect(4)]);
    let mut coordinator = ProductionSessionCoordinator::new(HeadlessEngine::default())
        .with_committed_surfaces(vec![a.clone()]);
    let transaction = SurfaceTransaction {
        input_region: None,
        transaction: TransactionId::from_raw(50),
        authority: AuthorityKind::SophiaX,
        namespace: None,
        surface: a.surface,
        target_geometry: b.geometry,
        content: b.content.clone(),
        presentation_extent: b.content.logical_extent(),
        damage: Region::empty(),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: 1,
    };
    let prepared = coordinator.prepare_present_transaction(&transaction);
    assert!(prepared.is_ready());
    let mut candidate_slot = snapshot(&b);
    candidate_slot.damage_history = coordinator
        .damage_history_for_candidate(prepared.candidate(), Some(&prepared))
        .unwrap();
    let other = coordinator.prepare_present_transaction(&transaction);
    assert_ne!(prepared.damage_identity(), other.damage_identity());
    assert_eq!(
        coordinator.apply_prepared_surface_commit(prepared).outcome,
        TransactionOutcome::Committed
    );
    let mut actual = snapshot(&b);
    actual.damage_history = coordinator
        .damage_history_for_candidate(coordinator.committed_surfaces(), None)
        .unwrap();
    assert!(
        output_frame_damage(Some(&candidate_slot), &actual)
            .unwrap()
            .rects
            .is_empty()
    );
    let stable_history = actual.damage_history.clone();
    assert_eq!(
        coordinator.apply_prepared_surface_commit(other).outcome,
        TransactionOutcome::RejectedStaleSurface
    );
    assert_eq!(
        coordinator
            .damage_history_for_candidate(coordinator.committed_surfaces(), None)
            .unwrap(),
        stable_history
    );
    let rebased = coordinator.prepare_present_transaction(&transaction);
    assert_eq!(
        rebased.candidate()[0].content.canonical_variant().damage,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 64,
            height: 64
        })
    );
    coordinator.replace_committed_surfaces(vec![state(8, vec![rect(1)])]);
    assert!(
        coordinator
            .damage_history_for_candidate(coordinator.committed_surfaces(), None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn first_surface_candidates_and_scaled_previews_keep_distinct_pixel_identities() {
    use sophia_engine::CompositorSurfaceInstance;
    let a = state(1, vec![rect(1)]);
    let identity = SurfaceDamageIdentity::default();
    let mut history = SurfaceDamageHistory::default();
    let mut rejected = snapshot(&a);
    rejected.damage_history = history
        .for_candidate(
            &[],
            std::slice::from_ref(&a),
            Some(&SurfaceDamageIdentity::default()),
        )
        .unwrap();
    history.record_committed(&[], std::slice::from_ref(&a), &identity);
    let mut actual = tracked_snapshot(&history, &a);
    let preview = Rect {
        x: 80,
        y: 20,
        width: 16,
        height: 16,
    };
    let instance = CompositorDisplayCommand::SurfaceInstance(CompositorSurfaceInstance {
        owner_epoch: 1,
        id: 1,
        generation: 1,
        source: a.surface,
        source_generation: 1,
        destination: preview,
        clip: preview,
        opacity_millis: 1000,
    });
    rejected
        .compositor_display_list
        .commands
        .push(instance.clone());
    actual.compositor_display_list.commands.push(instance);
    let damage = output_frame_damage(Some(&rejected), &actual).unwrap();
    assert!(damage.rects.contains(&a.geometry));
    assert!(
        damage.rects.contains(&preview),
        "same generation must not hide rejected preview pixels"
    );
    // The normal source-generation resolution also invalidates a scaled
    // instance without changing the WM interaction generation.
    let mut advanced = a.clone();
    advanced.committed_generation = 2;
    let mut list = CompositorDisplayList {
        output: actual.output.id,
        commands: actual.compositor_display_list.commands.clone(),
    };
    sophia_engine::resolve_surface_instance_sources(&mut list, &[advanced]).unwrap();
    let next =
        sophia_engine::compositor_display_list_damage(&actual.compositor_display_list, &list);
    assert!(next.rects.contains(&preview));
}

#[test]
fn noncanonical_variant_keeps_the_identity_of_its_prepared_content_set() {
    let state = state(2, vec![rect(1)]);
    let mut base = state.clone();
    base.committed_generation = 1;
    let mut history = primed_history(&base);
    let mut rejected = snapshot(&state);
    rejected.damage_history = history
        .for_candidate(
            std::slice::from_ref(&base),
            std::slice::from_ref(&state),
            Some(&SurfaceDamageIdentity::default()),
        )
        .unwrap();
    history.record_committed(
        &[base],
        std::slice::from_ref(&state),
        &SurfaceDamageIdentity::default(),
    );
    let mut committed = tracked_snapshot(&history, &state);
    // Head lowering writes the selected variant's handle into the slot, while
    // the journal's rectangle chain describes only the canonical variant.
    for slot in [&mut rejected, &mut committed] {
        slot.surfaces[0].buffer = BufferSource::DmaBuf { handle: 900 };
        slot.damage_history =
            sophia_engine::restrict_surface_damage_precision(slot.damage_history.clone(), &[]);
    }
    assert_eq!(rejected.surfaces, committed.surfaces);
    assert!(
        !output_frame_damage(Some(&rejected), &committed)
            .unwrap()
            .rects
            .is_empty(),
        "an alternate variant must not turn two preparation identities into None == None"
    );
    assert!(
        output_frame_damage(Some(&committed), &committed)
            .unwrap()
            .rects
            .is_empty(),
        "the same accepted variant may still reuse pixels"
    );
}
