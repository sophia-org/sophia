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
