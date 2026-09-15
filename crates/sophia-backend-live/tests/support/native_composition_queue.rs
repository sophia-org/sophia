//! Deferred production queue ownership only. These tests hold actual lowered
//! Mixed-frame bytes and use the real resource store. Primary ownership is a
//! simulated adapter input; this does not exercise KMS or the native group join.

use super::*;
use sophia_protocol::{
    ContentGrant, ContentLimits, ContentResourceBegin, ContentResourceChunk, ContentResourceEnd,
    ContentResourceId, ContentResourceRetire, Rect, ShellContentRecord, Size, Transform,
};
use sophia_renderer_live::{
    LiveCompositionPlacement, LiveOwnedMixedCompositionLayer, LiveSharedCpuBufferSource,
};

fn content_resource() -> (
    sophia_runtime::ContentResourceStore,
    sophia_runtime::ContentResourceLease,
) {
    let grant = ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    };
    let resource = ContentResourceId {
        id: 11,
        generation: 1,
    };
    let mut store =
        sophia_runtime::ContentResourceStore::new(ContentLimits::prototype(grant)).unwrap();
    store
        .begin(
            TransactionId::from_raw(1),
            ContentResourceBegin {
                grant,
                resource,
                width_px: 4,
                height_px: 2,
                rendered_scale_numerator: 1,
                rendered_scale_denominator: 1,
                pixel_format: 1,
                chunk_count: 1,
                total_bytes: 32,
            },
            0,
        )
        .unwrap();
    store
        .chunk(
            TransactionId::from_raw(2),
            &ContentResourceChunk {
                grant,
                resource,
                ordinal: 0,
                offset: 0,
                bytes: vec![0x7f; 32],
            },
            0,
        )
        .unwrap();
    store
        .end(
            TransactionId::from_raw(3),
            &ContentResourceEnd {
                grant,
                resource,
                total_bytes: 32,
                chunk_count: 1,
            },
            0,
        )
        .unwrap();
    let lease = store.lease(grant, resource).unwrap();
    (store, lease)
}

fn generation(
    lease: &sophia_runtime::ContentResourceLease,
    output: OutputId,
    frame: u64,
) -> LiveProductionQueuedMirrorGeneration {
    generation_with_owner(lease, output, frame, crate::NativeFrameOwner::new())
}

fn generation_with_owner(
    lease: &sophia_runtime::ContentResourceLease,
    output: OutputId,
    frame: u64,
    owner: crate::NativeFrameOwner,
) -> LiveProductionQueuedMirrorGeneration {
    let frame = LiveProductionNativeFrameId::from_raw(frame);
    let size = Size {
        width: 4,
        height: 2,
    };
    let description = lease.description();
    LiveProductionQueuedMirrorGeneration {
        output,
        frame,
        logical_content_checksum: Some(99),
        heads: (0..2)
            .map(|head_index| {
                let damage = sophia_engine::OutputFrameDamageSnapshot {
                    output: sophia_engine::HeadlessOutput {
                        id: output,
                        size,
                        scale: 1,
                    },
                    surfaces: Vec::new(),
                    compositor_display_list: sophia_engine::CompositorDisplayList::empty(output),
                    software_cursor: None,
                };
                LiveProductionQueuedMirrorHeadFrame {
                    head_index,
                    identity: owner.frame(
                        output,
                        sophia_engine::RenderHeadId::from_raw(head_index as u64 + 1),
                        1,
                        frame.raw(),
                    ),
                    content: LiveProductionScanoutContent::RetainedMixed {
                        logical_content_checksum: None,
                        requires_retirement: false,
                        frame,
                        nonzero_rgb_pixels: 0,
                    },
                    frame: crate::LiveOwnedMixedCompositionFrame {
                        layers: vec![LiveOwnedMixedCompositionLayer::Cpu {
                            buffer: LiveSharedCpuBufferSource {
                                handle: description.resource.id,
                                size,
                                stride: 16,
                                format: u32::from_le_bytes(*b"AR24"),
                                generation: description.resource.generation,
                                bytes: lease.clone().into(),
                            },
                            placement: LiveCompositionPlacement {
                                target: Rect {
                                    x: 0,
                                    y: 0,
                                    width: 4,
                                    height: 2,
                                },
                                clip: None,
                                transform: Transform::IDENTITY,
                                alpha: 1.0,
                                sampling: sophia_engine::HeadSamplingClass::Exact,
                            },
                        }],
                        output_damage_snapshot: Some(damage.clone()),
                        ..Default::default()
                    },
                    output_damage_snapshot: Some(damage),
                    cpu_nonzero_pixel_bytes: 0,
                }
            })
            .collect(),
    }
}

fn blocked() -> (
    Option<LiveProductionNativeFrameId>,
    Option<LiveProductionScanoutContent>,
) {
    let frame = LiveProductionNativeFrameId::from_raw(1);
    (
        Some(frame),
        Some(LiveProductionScanoutContent::MixedPresent {
            frame,
            transaction: TransactionId::from_raw(8),
            nonzero_rgb_pixels: 0,
        }),
    )
}

fn retire(
    store: &mut sophia_runtime::ContentResourceStore,
    lease: sophia_runtime::ContentResourceLease,
) {
    while store.take_event().is_some() {}
    store
        .retire(
            TransactionId::from_raw(9),
            &ContentResourceRetire {
                grant: lease.description().grant,
                resource: lease.description().resource,
            },
        )
        .unwrap();
    drop(lease);
}

fn assert_released_once(store: &mut sophia_runtime::ContentResourceStore) {
    store.collect();
    assert!(matches!(store.take_event().map(|event| event.record),
        Some(ShellContentRecord::ResourceReleased(value)) if value.resource.id == 11));
    store.collect();
    assert!(store.take_event().is_none());
    assert_eq!(store.usage().retiring, 0);
}

#[test]
fn deferred_frames_stay_owned_until_the_exact_primary_is_ready_and_both_heads_release() {
    let (mut store, lease) = content_resource();
    let output = OutputId::from_raw(1);
    let mut queue = DeferredNativeCompositions::default();
    let (active, content) = blocked();
    assert!(matches!(
        queue.offer(generation(&lease, output, 2), active, None, content),
        DeferredCompositionOffer::Deferred { replaced: None }
    ));
    // Another output can hand off while this output has not reached its primary.
    assert!(matches!(
        queue.offer(
            generation(&lease, OutputId::from_raw(2), 3),
            None,
            None,
            None
        ),
        DeferredCompositionOffer::Install(_)
    ));
    retire(&mut store, lease);
    assert!(
        queue
            .take_ready(
                output,
                active,
                Some(LiveProductionNativeFrameId::from_raw(99)),
                content
            )
            .is_none()
    );
    assert!(queue.owns(output, LiveProductionNativeFrameId::from_raw(2)));
    store.collect();
    assert!(store.take_event().is_none());
    let mut ready = queue.take_ready(output, active, active, content).unwrap();
    assert!(!queue.owns(output, ready.frame));
    assert!(queue.take_ready(output, active, active, content).is_none());
    drop(ready.heads.pop());
    store.collect();
    assert!(
        store.take_event().is_none(),
        "the second head still owns the pixels"
    );
    drop(ready);
    assert_released_once(&mut store);
}

#[test]
fn supersession_and_topology_cleanup_do_not_release_a_retained_byte_consumer() {
    let (mut store, lease) = content_resource();
    let output = OutputId::from_raw(1);
    let mut queue = DeferredNativeCompositions::default();
    let old = generation(&lease, output, 2);
    let bytes = match &old.heads[0].frame.layers[0] {
        LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } => buffer.bytes.clone(),
        _ => unreachable!(),
    };
    let (active, content) = blocked();
    assert!(matches!(
        queue.offer(old, active, None, content),
        DeferredCompositionOffer::Deferred { replaced: None }
    ));
    assert!(
        matches!(queue.offer(generation(&lease, output, 3), active, None, content),
        DeferredCompositionOffer::Deferred { replaced: Some(old) } if old.raw() == 2)
    );
    retire(&mut store, lease);
    queue.revoke_output(output);
    queue.clear();
    store.collect();
    assert!(store.take_event().is_none());
    assert_eq!(store.usage().retiring, 32);
    assert_eq!(&bytes[..], &[0x7f; 32]);
    drop(bytes);
    assert_released_once(&mut store);
}

#[test]
fn a_new_ready_offer_supersedes_an_older_deferred_owner_before_service() {
    let (mut store, lease) = content_resource();
    let output = OutputId::from_raw(1);
    let mut queue = DeferredNativeCompositions::default();
    let (active, content) = blocked();
    assert!(matches!(
        queue.offer(generation(&lease, output, 2), active, None, content),
        DeferredCompositionOffer::Deferred { replaced: None }
    ));
    let DeferredCompositionOffer::Install(newer) =
        queue.offer(generation(&lease, output, 3), active, active, content)
    else {
        panic!("the primary already owns the predecessor");
    };
    retire(&mut store, lease);
    assert!(!queue.owns(output, LiveProductionNativeFrameId::from_raw(2)));
    assert!(queue.take_ready(output, active, active, content).is_none());
    store.collect();
    assert!(
        store.take_event().is_none(),
        "the newer handoff still owns its bytes"
    );
    drop(newer);
    assert_released_once(&mut store);
}

#[test]
fn refused_installation_retains_pixel_owners_until_retry_or_explicit_output_cleanup() {
    let (mut store, lease) = content_resource();
    let output = OutputId::from_raw(1);
    let mut queue = DeferredNativeCompositions::default();
    let refused = generation(&lease, output, 2);
    retire(&mut store, lease);
    queue.retain_refused(refused).ok().unwrap();
    store.collect();
    assert!(store.take_event().is_none());
    let retry = queue.take_ready(output, None, None, None).unwrap();
    assert_eq!(retry.frame.raw(), 2);
    // Another failed adapter attempt must return the same actual owners.
    queue.retain_refused(retry).ok().unwrap();
    store.collect();
    assert!(store.take_event().is_none());
    queue.revoke_output(output);
    assert_released_once(&mut store);
}

#[test]
fn an_old_refused_retry_cannot_replace_a_newer_owned_generation() {
    let (mut store, lease) = content_resource();
    let output = OutputId::from_raw(1);
    let mut queue = DeferredNativeCompositions::default();
    let old = generation(&lease, output, 2);
    queue
        .retain_refused(generation(&lease, output, 3))
        .ok()
        .unwrap();
    queue.retain_refused(old).ok().unwrap();
    assert!(queue.owns(output, LiveProductionNativeFrameId::from_raw(3)));
    retire(&mut store, lease);
    store.collect();
    assert!(store.take_event().is_none());
    queue.clear();
    assert_released_once(&mut store);
}

#[test]
fn whole_batch_refusal_preserves_old_cells_and_returns_every_offered_pixel_owner() {
    let (mut store, lease) = content_resource();
    let first = OutputId::from_raw(1);
    let second = OutputId::from_raw(2);
    let configured = BTreeSet::from([first, second]);
    let mut queue = DeferredNativeCompositions::default();
    queue
        .admit_batch(vec![generation(&lease, first, 1)], &configured)
        .ok()
        .unwrap();
    let mut invalid = generation(&lease, second, 3);
    invalid.heads[1].content = LiveProductionScanoutContent::RetainedMixed {
        logical_content_checksum: None,
        requires_retirement: false,
        frame: LiveProductionNativeFrameId::from_raw(99),
        nonzero_rgb_pixels: 0,
    };
    let (_, returned) = queue
        .admit_batch(vec![generation(&lease, first, 2), invalid], &configured)
        .err()
        .unwrap();
    assert_eq!(returned.len(), 2);
    assert!(queue.owns(first, LiveProductionNativeFrameId::from_raw(1)));
    assert!(!queue.pending(second));
    retire(&mut store, lease);
    queue.clear();
    store.collect();
    assert!(
        store.take_event().is_none(),
        "refusal still owns both offered frames"
    );
    drop(returned);
    assert_released_once(&mut store);
}

#[test]
fn admitted_batch_is_visible_as_a_whole_before_any_independent_output_service() {
    let (mut store, lease) = content_resource();
    let first = OutputId::from_raw(1);
    let second = OutputId::from_raw(2);
    let configured = BTreeSet::from([first, second]);
    let mut queue = DeferredNativeCompositions::default();
    let admitted = queue
        .admit_batch(
            vec![generation(&lease, first, 2), generation(&lease, second, 3)],
            &configured,
        )
        .ok()
        .unwrap();
    assert_eq!(admitted.len(), 2);
    assert!(queue.pending(first) && queue.pending(second));
    retire(&mut store, lease);
    let first_handoff = queue.take_ready(first, None, None, None).unwrap();
    assert!(!queue.pending(first) && queue.pending(second));
    drop(first_handoff);
    store.collect();
    assert!(store.take_event().is_none());
    let second_handoff = queue.take_ready(second, None, None, None).unwrap();
    queue.retain_refused(second_handoff).ok().unwrap();
    store.collect();
    assert!(store.take_event().is_none());
    queue.clear();
    assert_released_once(&mut store);
}

#[test]
fn batch_admission_rejects_duplicate_unknown_regressed_and_over_capacity_outputs() {
    let (_, lease) = content_resource();
    let first = OutputId::from_raw(1);
    let mut queue = DeferredNativeCompositions::default();
    let configured = BTreeSet::from([first]);
    for batch in [
        vec![generation(&lease, first, 1), generation(&lease, first, 2)],
        vec![generation(&lease, OutputId::from_raw(99), 1)],
    ] {
        assert!(queue.admit_batch(batch, &configured).is_err());
        assert!(!queue.pending(first));
    }
    queue
        .admit_batch(vec![generation(&lease, first, 3)], &configured)
        .ok()
        .unwrap();
    assert!(
        queue
            .admit_batch(vec![generation(&lease, first, 2)], &configured)
            .is_err()
    );
    assert!(queue.owns(first, LiveProductionNativeFrameId::from_raw(3)));
    let configured = (1..=crate::LIVE_RENDERED_OUTPUT_CAPACITY as u64 + 1)
        .map(OutputId::from_raw)
        .collect::<BTreeSet<_>>();
    let batch = configured
        .iter()
        .enumerate()
        .map(|(i, output)| generation(&lease, *output, i as u64 + 4))
        .collect();
    assert!(queue.admit_batch(batch, &configured).is_err());
    assert_eq!(queue.generations.len(), 1);
}

#[test]
fn protected_retirement_survives_ordinary_repaint_and_failed_installation() {
    let (mut store, lease) = content_resource();
    let first = OutputId::from_raw(1);
    let second = OutputId::from_raw(2);
    let configured = BTreeSet::from([first, second]);
    let mut queue = DeferredNativeCompositions::default();
    let mut exact = generation(&lease, first, 1);
    for head in &mut exact.heads {
        head.content = LiveProductionScanoutContent::RetainedMixed {
            logical_content_checksum: None,
            frame: exact.frame,
            nonzero_rgb_pixels: 0,
            requires_retirement: true,
        };
    }
    queue.admit_batch(vec![exact], &configured).ok().unwrap();
    // Incoming latest-scene work has no retirement flag of its own. Its
    // numeric freshness does not grant permission to erase the occupied debt.
    let (_, refused) = queue
        .admit_batch(
            vec![generation(&lease, second, 2), generation(&lease, first, 3)],
            &configured,
        )
        .err()
        .unwrap();
    assert_eq!(refused.len(), 2);
    assert!(queue.owns(first, LiveProductionNativeFrameId::from_raw(1)));
    assert!(!queue.pending(second));
    assert!(matches!(
        queue.offer(generation(&lease, first, 4), None, None, None),
        DeferredCompositionOffer::Refused(_)
    ));
    assert!(queue.retain_refused(generation(&lease, first, 5)).is_err());
    queue
        .admit_batch(vec![generation(&lease, second, 6)], &configured)
        .ok()
        .unwrap();
    drop(queue.take_ready(second, None, None, None).unwrap());
    let attempt = queue.take_ready(first, None, None, None).unwrap();
    // Simulate a returned installation failure, not an unwind.
    queue.retain_refused(attempt).ok().unwrap();
    assert!(queue.protected(first));
    drop(refused);
    retire(&mut store, lease);
    store.collect();
    assert!(store.take_event().is_none());
    let retry = queue.take_ready(first, None, None, None).unwrap();
    assert_eq!(retry.frame.raw(), 1);
    assert!(retry.requires_retirement());
    drop(retry);
    assert_released_once(&mut store);
}

#[path = "native_composition_installation.rs"]
mod installation;

#[test]
fn topology_first_frames_are_owned_before_exporter_installation() {
    let (_store, lease) = content_resource();
    let owner = crate::NativeFrameOwner::new();
    let output = OutputId::from_raw(1);
    let mut queue = DeferredNativeCompositions::default();
    let expected = BTreeMap::from([(
        output,
        (0..2)
            .map(|index| {
                (
                    index,
                    owner.frame(
                        output,
                        sophia_engine::RenderHeadId::from_raw(index as u64 + 1),
                        1,
                        3,
                    ),
                )
            })
            .collect::<Vec<_>>(),
    )]);
    assert!(queue.validate_first_frames(&expected).is_err());
    queue
        .admit_batch(
            vec![generation_with_owner(&lease, output, 3, owner)],
            &BTreeSet::from([output]),
        )
        .unwrap_or_else(|(reason, _)| panic!("{reason}"));
    assert!(queue.validate_first_frames(&expected).is_ok());
    for negative in 0..7 {
        let mut wrong = expected.clone();
        match negative {
            0 => {
                wrong.get_mut(&output).unwrap()[1].1 = crate::NativeFrameOwner::new().frame(
                    output,
                    sophia_engine::RenderHeadId::from_raw(2),
                    1,
                    3,
                )
            }
            1 => {
                wrong.get_mut(&output).unwrap()[1].1 =
                    owner.frame(output, sophia_engine::RenderHeadId::from_raw(2), 2, 3)
            }
            2 => {
                wrong.get_mut(&output).unwrap()[1].1 =
                    owner.frame(output, sophia_engine::RenderHeadId::from_raw(2), 1, 4)
            }
            3 => {
                wrong.get_mut(&output).unwrap().pop();
            }
            4 => wrong.get_mut(&output).unwrap()[1].0 = 0,
            5 => {
                wrong.insert(OutputId::from_raw(2), vec![]);
            }
            6 => wrong.get_mut(&output).unwrap()[1].1 = expected[&output][0].1,
            _ => unreachable!(),
        }
        assert!(
            queue.validate_first_frames(&wrong).is_err(),
            "negative {negative}"
        );
        assert_eq!(queue.get(output).unwrap().frame.raw(), 3);
        assert_eq!(queue.get(output).unwrap().heads.len(), 2);
    }
}
