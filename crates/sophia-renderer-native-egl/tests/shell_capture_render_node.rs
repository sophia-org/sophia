//! Real render-node control for the native shell capture path.
//!
//! One focused qualification case, ignored by default: it opens only the
//! granted render node (no KMS, no scanout, no frame submission), prepares
//! a destination through the real GBM pool, copies a same-device LINEAR
//! XRGB source through `begin_shell_capture`, and checks the completed
//! snapshot pixel-exactly. It also proves a dropped pending capture keeps
//! its reservation charged (process-lifetime quarantine). This is native capture
//! correctness only; it says nothing about Session presentation.
#![cfg(all(
    feature = "gbm-platform",
    feature = "experimental-shell-capture",
    target_os = "linux"
))]

use sophia_renderer_native_egl::{
    NativeDmaBufPlane, NativeGbmRenderedScanoutContext, NativeMultiPlaneDmaBufFrame,
    NativePixmapImportProbe, NativeRendererImageId, NativeRendererSnapshotBudget,
    NativeRendererSnapshotEpoch, NativeShellCapturePoll, NativeShellCaptureRequest,
    NativeShellCaptureSource, NativeShellCaptureStart, native_dmabuf_cpu_write_access,
};
use std::{
    fs::{File, OpenOptions},
    os::fd::AsFd,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const WIDTH: u32 = 13;
const HEIGHT: u32 = 7;
const BY_PATH_RENDER: &str = "/dev/dri/by-path/pci-0000:03:00.0-render";

fn open(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}

fn pixel(sequence: u8, x: u32, y: u32) -> [u8; 4] {
    [
        sequence.wrapping_mul(5).wrapping_add(x as u8 * 3),
        20 + y as u8 * 11,
        sequence.wrapping_mul(3).wrapping_add((x + y) as u8),
        // The source fourth byte is deliberately not opaque: XRGB
        // readback must still report alpha one.
        0,
    ]
}

fn expected(sequence: u8) -> Vec<u8> {
    (0..HEIGHT)
        .flat_map(|y| {
            (0..WIDTH).flat_map(move |x| {
                let [r, g, b, _] = pixel(sequence, x, y);
                [r, g, b, 255]
            })
        })
        .collect()
}

fn write_source(buffer: &mut gbm::BufferObject<()>, fd: &std::os::fd::OwnedFd, sequence: u8) {
    native_dmabuf_cpu_write_access(fd, false).unwrap();
    buffer
        .map_mut(0, 0, WIDTH, HEIGHT, |mapped| {
            let stride = mapped.stride() as usize;
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    let [r, g, b, a] = pixel(sequence, x, y);
                    let start = y as usize * stride + x as usize * 4;
                    mapped.buffer_mut()[start..start + 4].copy_from_slice(&[b, g, r, a]);
                }
            }
        })
        .unwrap();
    native_dmabuf_cpu_write_access(fd, true).unwrap();
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; uses render node only"]
fn prepared_capture_copies_same_device_xrgb_and_abort_retains_reservation() {
    // The granted node is the display GPU at pci 0000:03:00.0, selected
    // through by-path so a renumbered renderD node cannot redirect it.
    let path = PathBuf::from(
        std::env::var_os("SOPHIA_TEST_RENDER_NODE")
            .expect("SOPHIA_TEST_RENDER_NODE must name the granted render node"),
    );
    assert_eq!(
        std::fs::canonicalize(&path).unwrap(),
        std::fs::canonicalize(BY_PATH_RENDER).unwrap(),
        "render node must resolve to the pci-0000:03:00.0 display GPU"
    );
    let device_rdev = rustix::fs::fstat(open(&path).as_fd()).unwrap().st_rdev;

    let mut context = NativeGbmRenderedScanoutContext::from_backend_device_result(Ok(open(&path)))
        .context
        .expect("render context");
    context.set_snapshot_reuse_enabled(true);

    // Facts come from this context's own device and offer LINEAR XRGB.
    let facts = context.shell_capture_facts().expect("capture facts");
    assert_eq!(facts.device, device_rdev);
    assert!(
        facts
            .formats
            .iter()
            .any(|f| f.format == 0x3432_5258 && f.modifiers.contains(&0))
    );

    // Same-device GPU producer: a LINEAR XRGB allocation on this node.
    let allocator = gbm::Device::new(open(&path)).unwrap();
    let mut source = allocator
        .create_buffer_object_with_modifiers2::<()>(
            WIDTH,
            HEIGHT,
            gbm::Format::Xrgb8888,
            std::iter::once(gbm::Modifier::Linear),
            gbm::BufferObjectFlags::RENDERING,
        )
        .unwrap();
    assert_eq!(u64::from(source.modifier()), 0);
    let source_fd = source.fd_for_plane(0).unwrap();
    write_source(&mut source, &source_fd, 1);

    // Prepared bound first: the proven destination bytes for this size.
    let bound = context
        .prepare_shell_capture_destination(WIDTH, HEIGHT)
        .unwrap()
        .expect("prepared destination bound");
    assert!(bound >= u64::from(WIDTH * HEIGHT * 4));

    let budget = NativeRendererSnapshotBudget::new(4, 1 << 20);
    let epoch = NativeRendererSnapshotEpoch::new(device_rdev, 1, 1);
    macro_rules! request {
        ($id:expr) => {
            NativeShellCaptureRequest {
                source: NativeShellCaptureSource {
                    fd: source_fd.try_clone().unwrap(),
                    width: WIDTH,
                    height: HEIGHT,
                    modifier: 0,
                    offset: source.offset(0),
                    stride: source.stride_for_plane(0),
                },
                image_id: NativeRendererImageId::from_raw($id),
                reservation: budget.reserve_capture(bound).unwrap(),
                budget: budget.clone(),
                epoch: epoch.clone(),
            }
        };
    }

    // Prepared bound -> ticket -> actual copy -> exact completion, with
    // no frame ever submitted.
    let start = context.begin_shell_capture(request!(0x8000_0000_0001));
    let mut pending = match start {
        NativeShellCaptureStart::Pending(pending) => pending,
        NativeShellCaptureStart::Refused { detail, .. } => {
            panic!("capture refused: {detail:?}")
        }
        NativeShellCaptureStart::Uncertain(detail) => panic!("capture uncertain: {detail:?}"),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let completion = loop {
        match context.poll_shell_capture(pending) {
            NativeShellCapturePoll::Pending(returned) => {
                assert!(Instant::now() < deadline, "capture did not complete");
                pending = returned;
                std::thread::sleep(Duration::from_millis(1));
            }
            NativeShellCapturePoll::Completed(completion) => break completion,
            NativeShellCapturePoll::Uncertain(detail) => {
                panic!("capture uncertain: {detail:?}")
            }
        }
    };
    let (_source, result) = completion.into_parts();
    let snapshot = result.expect("snapshot retained against the reservation");
    assert_eq!(snapshot.image_id().raw(), 0x8000_0000_0001);
    assert!(snapshot.is_retained());

    let read_snapshot = |context_path: &Path| {
        let frame = snapshot.as_frame();
        NativePixmapImportProbe::new(
            open(context_path),
            NativeMultiPlaneDmaBufFrame {
                width: frame.width,
                height: frame.height,
                format: frame.format,
                modifier: frame.modifier,
                plane_count: frame.plane_count,
                planes: std::array::from_fn(|i| {
                    frame.planes[i].as_ref().map(|plane| NativeDmaBufPlane {
                        fd: plane.fd.as_fd(),
                        offset: plane.offset,
                        stride: plane.stride,
                    })
                }),
            },
        )
        .unwrap()
        .read_rgba()
        .unwrap()
    };
    assert_eq!(read_snapshot(&path), expected(1));

    // Rewriting the source cannot reach the completed snapshot.
    write_source(&mut source, &source_fd, 2);
    assert_eq!(read_snapshot(&path), expected(1));

    // Abort: dropping a pending capture retains its pending storage;
    // the reservation stays charged until context teardown releases it.
    let start = context.begin_shell_capture(request!(0x8000_0000_0002));
    let pending = match start {
        NativeShellCaptureStart::Pending(pending) => pending,
        NativeShellCaptureStart::Refused { detail, .. } => {
            panic!("abort capture refused: {detail:?}")
        }
        NativeShellCaptureStart::Uncertain(detail) => {
            panic!("abort capture uncertain: {detail:?}")
        }
    };
    let charged_before = budget.usage();
    drop(pending);
    assert_eq!(
        budget.usage(),
        charged_before,
        "dropped pending capture must retain its reservation"
    );
    // Quarantine is deliberately process-lifetime (bounded by the
    // process copy cap): even context teardown must not release it.
    drop(context);
    assert_eq!(
        budget.usage(),
        charged_before,
        "quarantined reservation must survive context teardown"
    );
}
