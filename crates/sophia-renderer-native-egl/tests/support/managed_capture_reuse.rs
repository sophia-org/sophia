use super::*;
use sophia_renderer_native_egl::{
    NativeCapturedImage, NativeManagedCaptureLease, NativeRendererSnapshotBudget,
    NativeRendererSnapshotEpoch, NativeShellCapturePoll, NativeShellCaptureRequest,
    NativeShellCaptureSource, NativeShellCaptureStart,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn capture(
    context: &mut NativeGbmRenderedScanoutContext<File>,
    source: &Descriptors,
    budget: &Arc<NativeRendererSnapshotBudget>,
    epoch: &Arc<NativeRendererSnapshotEpoch>,
    wake: &sophia_wake::Wake,
    id: u64,
) -> Arc<NativeManagedCaptureLease> {
    let prepared = context
        .prepare_managed_shell_capture_destination(
            WIDTH,
            HEIGHT,
            budget.clone(),
            epoch.clone(),
            wake.notifier(),
        )
        .unwrap()
        .unwrap();
    let allocation = prepared.info().allocation_id;
    let (fd, offset, stride) = source.planes[0].as_ref().unwrap();
    let request = NativeShellCaptureRequest {
        source: NativeShellCaptureSource {
            fd: fd.try_clone().unwrap(),
            width: WIDTH,
            height: HEIGHT,
            offset: *offset,
            stride: *stride,
            modifier: source.modifier,
        },
        image_id: NativeRendererImageId::from_raw(id),
        budget: budget.clone(),
        epoch: epoch.clone(),
        reservation: prepared.reserve(budget).unwrap(),
    };
    let mut pending = match context.begin_shell_capture(request) {
        NativeShellCaptureStart::Pending(pending) => pending,
        NativeShellCaptureStart::Refused { detail, .. } => {
            panic!("managed capture refused: {detail:?}")
        }
        NativeShellCaptureStart::Uncertain(detail) => {
            panic!("managed capture uncertain: {detail:?}")
        }
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    let output = loop {
        match context.poll_shell_capture(pending) {
            NativeShellCapturePoll::Pending(next) => {
                assert!(Instant::now() < deadline, "capture fence timed out");
                pending = next;
                std::thread::sleep(Duration::from_millis(1));
            }
            NativeShellCapturePoll::Completed(completion) => {
                break completion.into_parts().1.unwrap();
            }
            NativeShellCapturePoll::Uncertain(detail) => {
                panic!("managed capture uncertain: {detail:?}")
            }
        }
    };
    let NativeCapturedImage::Managed(lease) = output.as_ref() else {
        panic!("managed request escaped as an exported snapshot");
    };
    assert_eq!(lease.info().allocation_id, allocation);
    assert!(
        context
            .export_promoted_renderer_image(lease.info().image_id)
            .is_err()
    );
    lease.clone()
}

fn drain(context: &mut NativeGbmRenderedScanoutContext<File>) {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let progress = context.maintain_managed_shell_captures().unwrap();
        if progress.pending_fences == 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "composition fences did not drain"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; uses render node only"]
fn managed_destinations_preserve_queued_pixels_reuse_after_readers_and_release_on_close() {
    let path = device_path();
    let mut renderer = context(&path, true);
    let allocator = gbm::Device::new(open(&path)).unwrap();
    let (mut buffer, source) = source(&allocator);
    let budget = NativeRendererSnapshotBudget::new(2, 1 << 20);
    let epoch = NativeRendererSnapshotEpoch::new(1, 1, 1);
    let wake = sophia_wake::Wake::new().unwrap();
    write_source(&mut buffer, &source, 1);
    let first = capture(&mut renderer, &source, &budget, &epoch, &wake, 901);
    let allocation = first.info().allocation_id;
    let generation = first.info().generation;
    let image = renderer.managed_shell_capture_image(&first).unwrap();
    let forged =
        NativeManagedCaptureLease::new(*first.info(), epoch.clone(), wake.notifier()).unwrap();
    assert!(
        renderer.managed_shell_capture_image(&forged).is_none(),
        "matching metadata is not owner custody"
    );
    drop(forged);
    let other_renderer = context(&path, true);
    assert!(other_renderer.managed_shell_capture_image(&first).is_none());
    drop(other_renderer);
    let output = compose(&mut renderer, image);
    assert_eq!(Descriptors::from_buffer(&output).read(&path), expected(1));
    first.retire();
    assert!(
        !renderer.evict_renderer_image(image).unwrap(),
        "queued lease forbids eviction"
    );
    write_source(&mut buffer, &source, 2);
    let second = capture(&mut renderer, &source, &budget, &epoch, &wake, 902);
    assert_ne!(allocation, second.info().allocation_id);
    let still_first = compose(&mut renderer, image);
    assert_eq!(
        Descriptors::from_buffer(&still_first).read(&path),
        expected(1)
    );
    let second_output = compose(&mut renderer, second.info().image_id);
    assert_eq!(
        Descriptors::from_buffer(&second_output).read(&path),
        expected(2)
    );
    drop(first);
    drain(&mut renderer);
    assert_eq!(
        budget.usage().0,
        2,
        "idle and import owners keep physical credit"
    );
    write_source(&mut buffer, &source, 3);
    let third = capture(&mut renderer, &source, &budget, &epoch, &wake, 903);
    assert_eq!(third.info().allocation_id, allocation);
    assert!(third.info().generation > generation);
    assert_eq!(budget.usage().0, 2, "reuse must not reserve a third charge");
    let third_output = compose(&mut renderer, third.info().image_id);
    assert_eq!(
        Descriptors::from_buffer(&third_output).read(&path),
        expected(3)
    );
    let still_second = compose(&mut renderer, second.info().image_id);
    assert_eq!(
        Descriptors::from_buffer(&still_second).read(&path),
        expected(2)
    );
    epoch.invalidate();
    drain(&mut renderer);
    assert!(renderer.managed_shell_capture_image(&second).is_none());
    assert!(renderer.managed_shell_capture_image(&third).is_none());
    assert_eq!(
        budget.usage(),
        (0, 0),
        "closed endpoint releases even with stale queued metadata"
    );
}
