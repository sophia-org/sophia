use super::*;

#[test]
fn an_output_import_keeps_the_charge_after_the_capture_owner_is_gone() {
    let accounting = std::rc::Rc::new(NativeCaptureAllocationAccounting::default());
    let capture = NativeCaptureAllocationCharge::new(&accounting, 4096);
    let first_output = capture.clone();
    let second_output = capture.clone();
    assert_eq!((accounting.count.get(), accounting.bytes.get()), (1, 4096));
    drop(capture);
    drop(first_output);
    assert_eq!(
        (accounting.count.get(), accounting.bytes.get()),
        (1, 4096),
        "a hidden output EGLImage still owns this allocation charge"
    );
    drop(second_output);
    assert_eq!((accounting.count.get(), accounting.bytes.get()), (0, 0));
    assert_eq!(accounting.peak_bytes.get(), 4096);
}

#[test]
fn a_graveyard_charge_cannot_keep_its_accounting_owner_alive() {
    let accounting = std::rc::Rc::new(NativeCaptureAllocationAccounting::default());
    let weak = std::rc::Rc::downgrade(&accounting);
    let charge = NativeCaptureAllocationCharge::new(&accounting, 4096);
    drop(accounting);
    assert!(
        weak.upgrade().is_none(),
        "charge ownership must not form a cycle"
    );
    drop(charge);
}

#[test]
fn output_cleanup_poison_survives_an_ordinary_image_store_clear() {
    let mut reuse = NativeCaptureReuse::default();
    let allocation = NativeCaptureAllocationCharge::new(&reuse.accounting, 8192);
    reuse.accounting.cleanup_failed.set(true);
    assert!(reuse.source_cleanup_failed());
    reuse.clear();
    assert!(reuse.source_cleanup_failed());
    assert_eq!((reuse.retained_count(), reuse.retained_bytes()), (1, 8192));
    assert_eq!(
        reuse.trim_to(8192),
        Err(NativeGbmScanoutBufferExportDetail::EglImageDestroyFailed)
    );
    drop(allocation);
    assert_eq!((reuse.retained_count(), reuse.retained_bytes()), (0, 0));
}

#[test]
fn repeated_pre_draw_import_refusals_release_storage_before_transfer() {
    let accounting = std::rc::Rc::new(NativeCaptureAllocationAccounting::default());
    let mut uncertain = Vec::new();
    let mut transfers = 0;
    for _ in 0..=DEFAULT_NATIVE_RENDERER_IMAGE_CAPACITY {
        let result = image_transfer_policy::capture_direct_or_transfer(
            &mut uncertain,
            |uncertain| {
                if accounting.count.get() >= DEFAULT_NATIVE_RENDERER_IMAGE_CAPACITY {
                    return Err(NativeGbmScanoutBufferExportDetail::RendererImageStoreFull);
                }
                let allocation = NativeCaptureAllocationCharge::new(&accounting, 4096);
                finish_native_capture_attempt(
                    allocation,
                    Err::<(), _>(NativeCaptureDrawFailure::BeforeDestinationWork(
                        NativeGbmScanoutBufferExportDetail::DmaBufImageCreateFailed,
                    )),
                    uncertain,
                )
            },
            image_import_failure,
            |_, error| {
                assert_eq!(
                    error,
                    NativeGbmScanoutBufferExportDetail::DmaBufImageCreateFailed
                );
                assert_eq!(
                    (accounting.count.get(), accounting.bytes.get()),
                    (0, 0),
                    "a pre-draw refusal must release storage before transfer"
                );
                transfers += 1;
                Ok(())
            },
        );
        assert_eq!(result, Ok(()), "transfer must remain reachable");
        assert!(uncertain.is_empty());
    }
    assert_eq!(transfers, DEFAULT_NATIVE_RENDERER_IMAGE_CAPACITY + 1);
    assert_eq!(accounting.peak_bytes.get(), 4096);
}

#[test]
fn a_failed_submitted_draw_keeps_storage_charged_until_teardown() {
    let accounting = std::rc::Rc::new(NativeCaptureAllocationAccounting::default());
    let allocation = NativeCaptureAllocationCharge::new(&accounting, 4096);
    let mut uncertain = Vec::new();
    let error = NativeGbmScanoutBufferExportDetail::CompositionFinishFailed;
    let result = finish_native_capture_attempt(
        allocation,
        Err::<(), _>(NativeCaptureDrawFailure::DestinationMayHaveWork(error)),
        &mut uncertain,
    );
    assert_eq!(result, Err(error));
    assert_eq!(uncertain.len(), 1);
    assert_eq!(
        (accounting.count.get(), accounting.bytes.get()),
        (1, 4096),
        "an unproven GPU completion must retain its allocation charge"
    );
    uncertain.clear();
    assert_eq!((accounting.count.get(), accounting.bytes.get()), (0, 0));
}

#[cfg(feature = "experimental-shell-capture")]
#[test]
fn destination_bound_uses_only_recorded_sizes_for_the_layout() {
    let mut reuse = NativeCaptureReuse::default();
    assert_eq!(reuse.destination_bound(64, 32), None);
    let candidates = rendered_scanout_candidates(&[], Some(0x3432_5258));
    reuse.known_sizes.push((
        NativeCaptureAllocationKey::new(64, 32, &candidates[0]),
        9000,
    ));
    reuse.known_sizes.push((
        NativeCaptureAllocationKey::new(64, 32, &candidates[1]),
        12000,
    ));
    reuse.known_sizes.push((
        NativeCaptureAllocationKey::new(32, 32, &candidates[0]),
        99999,
    ));
    // The bound covers whichever candidate the capture may pick, and
    // never leaks a different layout's size into the proof.
    assert_eq!(reuse.destination_bound(64, 32), Some(12000));
    assert_eq!(reuse.destination_bound(32, 32), Some(99999));
    assert_eq!(reuse.destination_bound(16, 16), None);
}

#[test]
fn import_time_stays_inside_copy_while_cleanup_is_removed() {
    let mut reuse = NativeCaptureReuse::default();
    assert!(
        NativeCaptureTimingSpan::start(&reuse.accounting, NativeCaptureTimingKind::Import)
            .is_none(),
        "timing starts disabled"
    );
    reuse.set_render_timing_enabled(true);
    let pause = std::time::Duration::from_millis(5);
    let span = |kind| NativeCaptureTimingSpan::start(&reuse.accounting, kind);
    {
        let _capture = span(NativeCaptureTimingKind::Capture);
        let _copy = span(NativeCaptureTimingKind::Copy);
        {
            let _import = span(NativeCaptureTimingKind::Import);
            std::thread::sleep(pause);
        }
        {
            let _cleanup = span(NativeCaptureTimingKind::Cleanup);
            std::thread::sleep(pause);
        }
    }
    let stats = reuse.stats();
    assert!(
        stats.import_elapsed >= pause && stats.cleanup_elapsed >= pause,
        "{stats:?}"
    );
    // Import is a nested subset of copy; only the measured cleanup leaves it,
    // so copy and cleanup together still fit inside the enclosing capture.
    assert!(stats.copy_elapsed >= stats.import_elapsed, "{stats:?}");
    assert!(
        stats.copy_elapsed + stats.cleanup_elapsed <= stats.capture_elapsed,
        "{stats:?}"
    );
    assert_eq!(stats.reclaim_elapsed, std::time::Duration::ZERO);
}

#[test]
fn destination_allocation_time_is_its_own_nested_bracket() {
    let mut reuse = NativeCaptureReuse::default();
    assert!(
        NativeCaptureTimingSpan::start(&reuse.accounting, NativeCaptureTimingKind::Allocation)
            .is_none(),
        "timing starts disabled"
    );
    reuse.set_render_timing_enabled(true);
    let pause = std::time::Duration::from_millis(5);
    let span = |kind| NativeCaptureTimingSpan::start(&reuse.accounting, kind);
    {
        // Capture path: allocation runs inside setup.
        let _setup = span(NativeCaptureTimingKind::Setup);
        let _allocation = span(NativeCaptureTimingKind::Allocation);
        std::thread::sleep(pause);
    }
    let nested = reuse.stats();
    assert!(nested.destination_allocation_elapsed >= pause, "{nested:?}");
    assert!(nested.setup_elapsed >= nested.destination_allocation_elapsed, "{nested:?}");
    {
        // Admission preparation: allocation outside every capture span.
        let _allocation = span(NativeCaptureTimingKind::Allocation);
        std::thread::sleep(pause);
    }
    let stats = reuse.stats();
    assert!(stats.destination_allocation_elapsed >= nested.destination_allocation_elapsed + pause);
    assert_eq!(stats.setup_elapsed, nested.setup_elapsed, "{stats:?}");
    assert_eq!(stats.capture_elapsed, std::time::Duration::ZERO, "{stats:?}");
    assert_eq!(stats.copy_elapsed, std::time::Duration::ZERO, "{stats:?}");
}

#[test]
fn composition_texture_time_survives_import_cache_accumulation() {
    let second = std::time::Duration::from_secs(1);
    let cache = NativeDmaBufImportCacheStats {
        imports: 1,
        texture_calls: 2,
        texture_cpu: second,
        texture_elapsed: 2 * second,
        ..Default::default()
    };
    // A destroyed target's cache, then a live one, as persistent_render_stats merges them.
    let mut total = NativeDmaBufImportCacheStats::default();
    accumulate_import_cache_stats(&mut total, cache);
    accumulate_import_cache_stats(&mut total, cache);
    assert_eq!(
        (total.imports, total.texture_calls, total.texture_cpu, total.texture_elapsed),
        (2, 4, 2 * second, 4 * second)
    );
    let full = NativeDmaBufImportCacheStats {
        texture_calls: usize::MAX,
        texture_cpu: std::time::Duration::MAX,
        texture_elapsed: std::time::Duration::MAX,
        ..Default::default()
    };
    accumulate_import_cache_stats(&mut total, full);
    assert_eq!(
        (total.texture_calls, total.texture_cpu, total.texture_elapsed),
        (usize::MAX, std::time::Duration::MAX, std::time::Duration::MAX)
    );
}
