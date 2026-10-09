use super::*;

fn store(index: usize, group: usize, busy: bool) -> ColdImageStore {
    ColdImageStore {
        index,
        group,
        busy,
        output: OutputId::from_raw(index as u64 + 1),
        identity: index as u64 + 11,
    }
}

#[test]
fn idle_move_and_straddle_restore_once_without_charging_snapshot_custody() {
    let image = Image::from_raw(1);
    let stores = [store(0, 0, false), store(1, 0, false)];
    let mut images = PreviewImages::default();
    images.owners.insert(image, BTreeSet::from([11]));
    images
        .cold_misses
        .insert(image, BTreeSet::from([stores[0].output, stores[1].output]));
    let mut transfers = Vec::new();
    prepare_cold_images(
        &mut images.cold_misses,
        &mut images.owners,
        &stores,
        &mut BTreeMap::new(),
        0,
        |a, b, id| {
            transfers.push((a, b, id));
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(transfers, vec![(0, 1, image)]);
    assert_eq!(images.owners[&image], BTreeSet::from([11, 12]));
    assert!(images.cold_misses.is_empty());
    assert!(images.snapshots.is_empty());
    assert_eq!(images.budget.usage(), (0, 0));
    prepare_cold_images(
        &mut images.cold_misses,
        &mut images.owners,
        &stores,
        &mut BTreeMap::new(),
        0,
        |_, _, _| panic!("no repeated work after the one-shot restore"),
    )
    .unwrap();
}

#[test]
fn busy_donors_and_full_worker_queues_defer_only_their_destination() {
    use crate::LiveRendererScanoutBufferExportDetail as D;
    for refusal in [D::WorkerPending, D::WorkerQueueFull] {
        let a = Image::from_raw(1);
        let b = Image::from_raw(2);
        // Cross-device COLD migration is allowed to use the existing transfer
        // fallback. HOT snapshot preparation still refuses that combination.
        let mut stores = [store(0, 0, true), store(1, 0, false), store(2, 1, false)];
        let mut owners = BTreeMap::from([(a, BTreeSet::from([11])), (b, BTreeSet::from([12]))]);
        let mut demand = BTreeMap::from([
            (a, BTreeSet::from([stores[1].output])),
            (b, BTreeSet::from([stores[2].output])),
        ]);
        let mut calls = Vec::new();
        prepare_cold_images(&mut demand, &mut owners, &stores, &mut BTreeMap::new(), 0, |from, to, image| {
            calls.push((from, to, image));
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, vec![(1, 2, b)]);
        assert_eq!(
            demand,
            BTreeMap::from([(a, BTreeSet::from([stores[1].output]))])
        );
        stores[0].busy = false;
        prepare_cold_images(&mut demand, &mut owners, &stores, &mut BTreeMap::new(), 0, |_, _, _| Err(refusal)).unwrap();
        assert!(!owners[&a].contains(&12));
        assert!(!demand.is_empty());
        prepare_cold_images(&mut demand, &mut owners, &stores, &mut BTreeMap::new(), 0, |from, to, image| {
            assert_eq!((from, to, image), (0, 1, a));
            Ok(())
        })
        .unwrap();
        assert!(demand.is_empty());
        assert_eq!(owners[&a], BTreeSet::from([11, 12]));
    }
}

#[test]
fn a_missing_displayed_donor_and_real_restore_faults_remain_typed_errors() {
    use crate::LiveRendererScanoutBufferExportDetail as D;
    let image = Image::from_raw(1);
    let stores = [store(0, 0, false), store(1, 0, false)];
    let mut demand = BTreeMap::from([(image, BTreeSet::from([stores[1].output]))]);
    let mut owners = BTreeMap::new();
    assert_eq!(
        prepare_cold_images(&mut demand, &mut owners, &stores, &mut BTreeMap::new(), 0, |_, _, _| unreachable!()),
        Err(D::InvalidRendererImageId)
    );
    owners.insert(image, BTreeSet::from([11]));
    // A full store is gated, and a refused import reported, not raised;
    // every other fault still is.
    for detail in [
        D::WorkerStalled,
        D::WorkerDisconnected,
        D::EglMakeCurrentFailed,
    ] {
        assert_eq!(
            prepare_cold_images(&mut demand, &mut owners, &stores, &mut BTreeMap::new(), 0, |_, _, _| Err(detail)),
            Err(detail)
        );
        assert!(!owners[&image].contains(&12));
    }
}

#[test]
fn a_staged_cold_copy_defers_until_promotion_or_restore_after_rollback() {
    use crate::LiveRendererScanoutBufferExportDetail as D;
    // These are worker observations injected at the same confirmation seam
    // used by production: an existing staged copy, then either promotion or
    // rollback followed by a successful fresh capture.
    for rolled_back in [false, true] {
        let image = Image::from_raw(1);
        let stores = [store(0, 0, false), store(1, 0, false)];
        let mut images = PreviewImages::default();
        images.owners.insert(image, BTreeSet::from([11]));
        images.cold_misses.insert(image, BTreeSet::from([stores[1].output]));
        prepare_cold_images(&mut images.cold_misses, &mut images.owners, &stores, &mut BTreeMap::new(), 0,
            |_, _, _| confirm_cold_restore(false, || Ok(false))).unwrap();
        assert!(!images.owners[&image].contains(&12));
        assert_eq!(images.cold_misses[&image], BTreeSet::from([stores[1].output]));
        assert!(cold_preparation_ready(&images.cold_misses, &images.owners, &stores, &BTreeMap::new(), 0));

        let mut confirmations = 0;
        prepare_cold_images(&mut images.cold_misses, &mut images.owners, &stores, &mut BTreeMap::new(), 0,
            |_, _, _| confirm_cold_restore(rolled_back, || {
                confirmations += 1;
                Ok(true)
            })).unwrap();
        assert_eq!(confirmations, usize::from(!rolled_back));
        assert!(images.owners[&image].contains(&12));
        assert!(images.cold_misses.is_empty());
        assert!(!cold_preparation_ready(&images.cold_misses, &images.owners, &stores, &BTreeMap::new(), 0));
        prepare_cold_images(&mut images.cold_misses, &mut images.owners, &stores, &mut BTreeMap::new(), 0,
            |_, _, _| panic!("the restored copy is local")).unwrap();
    }
    assert_eq!(confirm_cold_restore(false, || Err(D::WorkerDisconnected)),
        Err(D::WorkerDisconnected));
}

#[test]
fn cold_misses_owe_a_pass_and_copy_at_most_one_image_per_pass() {
    let stores = [store(0, 0, false), store(1, 0, false)];
    let mut owners = BTreeMap::new();
    let mut demand = BTreeMap::new();
    for raw in 1..=3 {
        let image = Image::from_raw(raw);
        owners.insert(image, BTreeSet::from([11]));
        demand.insert(image, BTreeSet::from([stores[1].output]));
    }
    for remaining in (0..3).rev() {
        assert!(cold_preparation_ready(&demand, &owners, &stores, &BTreeMap::new(), 0));
        let mut visits = 0;
        prepare_cold_images(&mut demand, &mut owners, &stores, &mut BTreeMap::new(), 0, |_, _, _| {
            visits += 1;
            Ok(())
        }).unwrap();
        assert_eq!(visits, 1);
        assert_eq!(demand.len(), remaining);
    }
    assert!(!cold_preparation_ready(&demand, &owners, &stores, &BTreeMap::new(), 0));
    demand.insert(Image::from_raw(4), BTreeSet::from([stores[1].output]));
    owners.insert(Image::from_raw(4), BTreeSet::from([11]));
    let mut busy = stores;
    busy[0].busy = true;
    assert!(!cold_preparation_ready(&demand, &owners, &busy, &BTreeMap::new(), 0), "completion wakes, no polling");
    busy[0].busy = false;
    assert!(cold_preparation_ready(&demand, &owners, &busy, &BTreeMap::new(), 0));
}

#[test]
fn hot_previews_choose_a_donor_per_recipient_gpu_not_the_first_owner() {
    let image = Image::from_raw(1);
    let stores = [store(0, 0, false), store(1, 1, false),
        store(2, 1, false), store(3, 0, false), store(4, 2, false)];
    let owners = BTreeSet::from([11, 12]);
    assert_eq!(preview_group_donors(image, &BTreeSet::from([stores[2].output]),
        Some(&owners), &stores).unwrap(), BTreeMap::from([(1, 1)]));
    assert_eq!(preview_group_donors(image, &BTreeSet::from([stores[2].output, stores[3].output]),
        Some(&owners), &stores).unwrap(), BTreeMap::from([(0, 0), (1, 1)]));
    assert!(preview_group_donors(image, &BTreeSet::from([stores[0].output]),
        Some(&owners), &stores).unwrap().is_empty(), "shared local store needs no snapshot");
    assert_eq!(preview_group_donors(image, &BTreeSet::from([stores[4].output]),
        Some(&owners), &stores), Err(LivePreviewImageRefusal::CrossDevice { image }));
}

#[test]
fn native_admission_defers_only_the_nonrequired_recovering_output() {
    use super::super::{composition_admission::defer_recovering_outputs,
        renderer_images::LiveProductionHeadCompositionContent as Content};
    let a = OutputId::from_raw(1);
    let b = OutputId::from_raw(2);
    let mut frames = vec![(a, vec![]), (b, vec![])];
    defer_recovering_outputs(&mut frames, &BTreeSet::from([a]), Content::Retained,
        |output| Ok(output == b)).unwrap();
    assert_eq!(frames.iter().map(|(output, _)| *output).collect::<Vec<_>>(), vec![a]);
    let mut required = vec![(b, vec![])];
    assert_eq!(defer_recovering_outputs(&mut required, &BTreeSet::from([b]),
        Content::Retained, |_| Ok(true)).unwrap_err().to_string(),
        "native preview recovery admission bypassed readiness");
}

#[test]
fn an_image_previewed_elsewhere_can_still_request_an_ordinary_cold_move() {
    let mut images = PreviewImages::default();
    let image = Image::from_raw(1);
    let preview = OutputId::from_raw(1);
    let moved = OutputId::from_raw(2);
    images.demand_sources.insert(image, (sophia_protocol::SurfaceId::new(1, 1),
        BTreeSet::from([preview])));
    assert!(images.preview_on_output(image, preview));
    assert!(!images.preview_on_output(image, moved));
    // The ordinary output uses the cold path even with the same image in an
    // active publication. It can migrate across GPUs without revoking it.
    let stores = [store(0, 0, false), store(1, 1, false)];
    images.owners.insert(image, BTreeSet::from([11]));
    images.cold_misses.insert(image, BTreeSet::from([moved]));
    assert!(cold_preparation_ready(&images.cold_misses, &images.owners, &stores, &BTreeMap::new(), 0));
    prepare_cold_images(&mut images.cold_misses, &mut images.owners, &stores, &mut BTreeMap::new(), 0,
        |from, to, _| { assert_eq!((from, to), (0, 1)); Ok(()) }).unwrap();
    assert!(images.owners[&image].contains(&12));
    assert!(images.preview_on_output(image, preview));
}

#[test]
fn a_full_store_waits_for_progress_without_spinning_or_losing_the_donor() {
    use crate::LiveRendererScanoutBufferExportDetail as D;
    let image = Image::from_raw(1);
    let stores = [store(0, 0, false), store(1, 0, false)];
    let mut owners = BTreeMap::from([(image, BTreeSet::from([11]))]);
    let mut demand = BTreeMap::from([(image, BTreeSet::from([stores[1].output]))]);
    let mut gate = BTreeMap::new();
    let refused = prepare_cold_images(&mut demand, &mut owners, &stores, &mut gate, 7, |_, _, _| {
        Err(D::RendererImageStoreFull)
    })
    .unwrap();
    assert!(refused.is_empty());
    assert_eq!(owners[&image], BTreeSet::from([11]));
    assert_eq!(demand[&image], BTreeSet::from([stores[1].output]));
    // No progress: not ready, and not tried again.
    assert!(!cold_preparation_ready(&demand, &owners, &stores, &gate, 7));
    prepare_cold_images(&mut demand, &mut owners, &stores, &mut gate, 7, |_, _, _| {
        unreachable!("a gated pair is not retried before progress")
    })
    .unwrap();
    // After a retirement the same donor image restores.
    assert!(cold_preparation_ready(&demand, &owners, &stores, &gate, 8));
    prepare_cold_images(&mut demand, &mut owners, &stores, &mut gate, 8, |from, to, id| {
        assert_eq!((from, to, id), (0, 1, image));
        Ok(())
    })
    .unwrap();
    assert!(demand.is_empty());
    assert_eq!(owners[&image], BTreeSet::from([11, 12]));
}

#[test]
fn a_refused_import_is_reported_and_the_donor_keeps_its_copy() {
    use crate::LiveRendererScanoutBufferExportDetail as D;
    let image = Image::from_raw(1);
    let stores = [store(0, 0, false), store(1, 1, false)];
    let mut owners = BTreeMap::from([(image, BTreeSet::from([11]))]);
    let mut demand = BTreeMap::from([(image, BTreeSet::from([stores[1].output]))]);
    let refused = prepare_cold_images(&mut demand, &mut owners, &stores, &mut BTreeMap::new(), 0, |_, _, _| {
        Err(D::DmaBufImportFailed)
    })
    .unwrap();
    assert_eq!(refused, vec![(image, stores[1].output)]);
    assert!(demand.is_empty());
    assert_eq!(owners[&image], BTreeSet::from([11]));
}

#[test]
fn one_output_refusing_leaves_another_outputs_restored_copy() {
    // REVIEW-CODEX-05 R3: store A takes the image, store B refuses it; A's
    // copy and its owner entry stay.
    use crate::LiveRendererScanoutBufferExportDetail as D;
    let image = Image::from_raw(1);
    let stores = [store(0, 0, false), store(1, 0, false), store(2, 1, false)];
    let mut owners = BTreeMap::from([(image, BTreeSet::from([11]))]);
    let mut demand = BTreeMap::from([(image, BTreeSet::from([stores[1].output, stores[2].output]))]);
    let mut refused = Vec::new();
    for _ in 0..2 {
        refused.extend(
            prepare_cold_images(&mut demand, &mut owners, &stores, &mut BTreeMap::new(), 0, |_, to, _| {
                if to == 2 { Err(D::DmaBufImportFailed) } else { Ok(()) }
            })
            .unwrap(),
        );
    }
    assert_eq!(refused, vec![(image, stores[2].output)]);
    assert_eq!(owners[&image], BTreeSet::from([11, 12]));
    assert!(demand.is_empty());
}

#[test]
fn a_busy_bridge_is_retried_on_the_next_pass_and_not_gated() {
    use crate::LiveRendererScanoutBufferExportDetail as D;
    let image = Image::from_raw(1);
    let stores = [store(0, 0, false), store(1, 0, false)];
    let mut owners = BTreeMap::from([(image, BTreeSet::from([11]))]);
    let mut demand = BTreeMap::from([(image, BTreeSet::from([stores[1].output]))]);
    let mut gate = BTreeMap::new();
    prepare_cold_images(&mut demand, &mut owners, &stores, &mut gate, 5, |_, _, _| {
        Err(D::RendererImageTransferBusy)
    })
    .unwrap();
    assert!(gate.is_empty());
    // Still owed and ready at the same storage progress: the owner's short
    // service retries while the GPU work is in flight.
    assert!(cold_preparation_ready(&demand, &owners, &stores, &gate, 5));
    prepare_cold_images(&mut demand, &mut owners, &stores, &mut gate, 5, |_, _, _| Ok(())).unwrap();
    assert!(demand.is_empty());
}
