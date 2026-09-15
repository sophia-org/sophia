//! The production install validator borrows real queued mirror source owners.
//! Current head facts are simulated; no exporter, KMS or native group completion.
use super::*;
use crate::{NativeCompositionInstallationHead, validate_composition_installation};

fn current(
    owner: crate::NativeFrameOwner,
    output: OutputId,
) -> Vec<NativeCompositionInstallationHead> {
    (0..2)
        .map(|index| NativeCompositionInstallationHead {
            index,
            identity: owner.frame(
                output,
                sophia_engine::RenderHeadId::from_raw(index as u64 + 1),
                1,
                3,
            ),
            prepared_cleanup_available: true,
            protected_frames: [None; 3],
        })
        .collect()
}

#[test]
fn second_head_installation_refusal_keeps_both_real_sources_until_exact_retry() {
    let (mut store, lease) = content_resource();
    let output = OutputId::from_raw(1);
    let owner = crate::NativeFrameOwner::new();
    let generation = generation_with_owner(&lease, output, 3, owner);
    retire(&mut store, lease);
    let wrong = crate::NativeFrameOwner::new();
    let head = sophia_engine::RenderHeadId::from_raw(2);
    let identities = [
        wrong.frame(output, head, 1, 3),
        owner.frame(OutputId::from_raw(2), head, 1, 3),
        owner.frame(output, sophia_engine::RenderHeadId::from_raw(3), 1, 3),
        owner.frame(output, head, 2, 3),
        owner.frame(output, head, 1, 4),
    ];
    for identity in identities {
        let mut targets = current(owner, output);
        targets[1].identity = identity;
        assert_eq!(
            validate_composition_installation(&generation, &targets),
            Err("mirror generation does not name the current native targets")
        );
        store.collect();
        assert!(store.take_event().is_none());
        assert_eq!(generation.heads.len(), 2);
    }
    let mut targets = current(owner, output);
    targets[1].prepared_cleanup_available = false;
    assert_eq!(
        validate_composition_installation(&generation, &targets),
        Err("mirror generation waits for prepared-owner cleanup capacity")
    );
    targets[1].prepared_cleanup_available = true;
    for slot in 0..3 {
        targets[1].protected_frames[slot] = Some(LiveProductionNativeFrameId::from_raw(2));
        assert_eq!(
            validate_composition_installation(&generation, &targets),
            Err("composition installation waits for an existing distinct retirement")
        );
        targets[1].protected_frames[slot] = None;
    }
    assert!(validate_composition_installation(&generation, &targets).is_ok());
    store.collect();
    assert!(store.take_event().is_none()); // validation is not consumption
    let mut heads = generation.heads.into_iter();
    drop(heads.next());
    store.collect();
    assert!(store.take_event().is_none()); // the second real head still holds bytes
    drop(heads);
    assert_released_once(&mut store);
}

#[test]
fn incomplete_repeated_or_reordered_heads_never_pass_installation() {
    let (_store, lease) = content_resource();
    let output = OutputId::from_raw(1);
    let owner = crate::NativeFrameOwner::new();
    let mut generation = generation_with_owner(&lease, output, 3, owner);
    let targets = current(owner, output);
    assert!(validate_composition_installation(&generation, &targets[..1]).is_err());
    generation.heads.swap(0, 1);
    assert!(validate_composition_installation(&generation, &targets).is_err());
    generation.heads.swap(0, 1);
    generation.heads[1].head_index = 0;
    assert!(validate_composition_installation(&generation, &targets).is_err());
}
