//! Supplied metadata/owner controls; no EGL context or DMA-BUF import.
use super::*;
use std::os::unix::net::UnixStream;

fn request() -> NativeShellCaptureRequest {
    let (source, _peer) = UnixStream::pair().unwrap();
    let budget = NativeRendererSnapshotBudget::new(2, 4096);
    NativeShellCaptureRequest {
        source: NativeShellCaptureSource {
            fd: source.into(),
            width: 8,
            height: 8,
            modifier: 0,
            offset: 0,
            stride: 32,
        },
        image_id: NativeRendererImageId::from_raw(1),
        reservation: budget.reserve_capture(512).unwrap(),
        budget,
        epoch: NativeRendererSnapshotEpoch::new(1, 1, 1),
    }
}

#[test]
fn source_metadata_epoch_and_reservation_must_match_before_submission() {
    let mut r = request();
    assert!(r.preflight());
    r.image_id = NativeRendererImageId::INVALID;
    assert!(!r.preflight());
    r.image_id = NativeRendererImageId::from_raw(1);
    r.source.width = 0;
    assert!(!r.preflight());
    r.source.width = 8;
    r.source.stride = 1;
    assert!(!r.preflight());
    r.source.stride = 32;
    r.source.height = 100;
    assert!(!r.preflight());
    r.source.height = 8;
    r.budget = NativeRendererSnapshotBudget::new(2, 4096);
    assert!(!r.preflight());
    let r = request();
    r.epoch.invalidate();
    assert!(!r.preflight());
}

#[test]
fn allocated_padding_is_checked_before_the_draw_not_after_copy() {
    let called = Cell::new(false);
    let result = draw_within_destination_bound(4096, 256, || {
        called.set(true);
        Ok(())
    });
    assert!(matches!(
        result,
        Err(NativeGbmScanoutBufferExportDetail::RendererImageStoreFull)
    ));
    assert!(!called.get());
    assert_eq!(
        draw_within_destination_bound(4096, 4096, || Ok(7)).unwrap(),
        7
    );
}

#[test]
fn context_replacement_or_retirement_never_enters_fence_polling() {
    let owner = Rc::new(Owner::default());
    let replacement = Rc::new(Owner::default());
    assert!(Owner::matches(&owner, &owner));
    assert!(!Owner::matches(&owner, &replacement));
    owner.retire();
    assert!(!Owner::matches(&owner, &owner));
}

#[test]
fn capture_facts_withhold_every_non_xrgb_format() {
    let formats = vec![
        NativeDmaBufImportFormat {
            format: 0x3432_5258,
            modifiers: vec![0, 7],
        },
        NativeDmaBufImportFormat {
            format: 0x3432_5241,
            modifiers: vec![0],
        },
        NativeDmaBufImportFormat {
            format: 0x3231_324e,
            modifiers: vec![3],
        },
    ];
    let kept = xrgb_capture_formats(formats);
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].format, 0x3432_5258);
    assert_eq!(kept[0].modifiers, vec![0, 7]);
    assert!(xrgb_capture_formats(Vec::new()).is_empty());
}
