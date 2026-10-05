#![cfg(all(feature = "gbm-platform", target_os = "linux"))]

use sophia_renderer_native_egl::{
    NativeCompositionFrame, NativeCompositionLayer, NativeCompositionRect,
    NativeCompositionSampling, NativeDmaBufPlane, NativeGbmOwnedScanoutBuffer,
    NativeGbmRenderedScanoutContext, NativeMultiPlaneDmaBufFrame, NativePixmapImportProbe,
    NativeRendererImageCompositionLayer, NativeRendererImageId, NativeSolidCompositionLayer,
    native_dmabuf_cpu_write_access,
};
use std::{
    collections::BTreeSet,
    fs::{File, OpenOptions},
    os::fd::{AsFd, OwnedFd},
    path::{Path, PathBuf},
};

const WIDTH: u32 = 13;
const HEIGHT: u32 = 7;

fn device_path() -> PathBuf {
    std::env::var_os("SOPHIA_TEST_RENDER_NODE")
        .expect("SOPHIA_TEST_RENDER_NODE must name the granted render node")
        .into()
}

fn open(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}

fn context(path: &Path, reuse: bool) -> NativeGbmRenderedScanoutContext<File> {
    let mut context = NativeGbmRenderedScanoutContext::from_backend_device_result(Ok(open(path)))
        .context
        .expect("render context");
    context.set_snapshot_reuse_enabled(reuse);
    context
}

struct Descriptors {
    width: u32,
    height: u32,
    format: u32,
    modifier: u64,
    plane_count: u8,
    planes: [Option<(OwnedFd, u32, u32)>; 4],
}

impl Descriptors {
    fn duplicate(frame: NativeMultiPlaneDmaBufFrame<'_>) -> Self {
        Self {
            width: frame.width,
            height: frame.height,
            format: frame.format,
            modifier: frame.modifier,
            plane_count: frame.plane_count,
            planes: std::array::from_fn(|index| {
                frame.planes[index].map(|plane| {
                    (
                        plane.fd.try_clone_to_owned().unwrap(),
                        plane.offset,
                        plane.stride,
                    )
                })
            }),
        }
    }

    fn from_buffer(buffer: &NativeGbmOwnedScanoutBuffer) -> Self {
        let mut fds = buffer.export_plane_fds().unwrap().into_plane_fds();
        Self {
            width: buffer.width(),
            height: buffer.height(),
            format: buffer.format(),
            modifier: buffer
                .modifier()
                .unwrap_or(u64::from(gbm::Modifier::Invalid)),
            plane_count: buffer.plane_count(),
            planes: std::array::from_fn(|index| {
                fds[index].take().map(|fd| {
                    (
                        fd,
                        buffer.plane_offsets()[index],
                        buffer.plane_pitches()[index],
                    )
                })
            }),
        }
    }

    fn frame(&self) -> NativeMultiPlaneDmaBufFrame<'_> {
        NativeMultiPlaneDmaBufFrame {
            width: self.width,
            height: self.height,
            format: self.format,
            modifier: self.modifier,
            plane_count: self.plane_count,
            planes: std::array::from_fn(|index| {
                self.planes[index]
                    .as_ref()
                    .map(|(fd, offset, stride)| NativeDmaBufPlane {
                        fd: fd.as_fd(),
                        offset: *offset,
                        stride: *stride,
                    })
            }),
        }
    }

    fn read(&self, path: &Path) -> Vec<u8> {
        NativePixmapImportProbe::new(open(path), self.frame())
            .unwrap()
            .read_rgba()
            .unwrap()
    }

    fn allocation(&self) -> (u64, u64) {
        let stat = rustix::fs::fstat(&self.planes[0].as_ref().unwrap().0).unwrap();
        (stat.st_dev, stat.st_ino)
    }
}

fn source(allocator: &gbm::Device<File>) -> (gbm::BufferObject<()>, Descriptors) {
    let buffer = allocator
        .create_buffer_object_with_modifiers2::<()>(
            WIDTH,
            HEIGHT,
            gbm::Format::Xrgb8888,
            std::iter::once(gbm::Modifier::Linear),
            gbm::BufferObjectFlags::RENDERING,
        )
        .unwrap();
    let descriptor = Descriptors {
        width: WIDTH,
        height: HEIGHT,
        format: gbm::Format::Xrgb8888 as u32,
        modifier: u64::from(buffer.modifier()),
        plane_count: 1,
        planes: [
            Some((
                buffer.fd_for_plane(0).unwrap(),
                buffer.offset(0),
                buffer.stride_for_plane(0),
            )),
            None,
            None,
            None,
        ],
    };
    (buffer, descriptor)
}

fn pixel(sequence: u8, x: u32, y: u32) -> [u8; 4] {
    [
        sequence.wrapping_mul(5).wrapping_add(x as u8 * 3),
        20 + y as u8 * 11,
        sequence.wrapping_mul(3).wrapping_add((x + y) as u8),
        255,
    ]
}

fn expected(sequence: u8) -> Vec<u8> {
    (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).flat_map(move |x| pixel(sequence, x, y)))
        .collect()
}

fn write_source(buffer: &mut gbm::BufferObject<()>, descriptor: &Descriptors, sequence: u8) {
    let fd = &descriptor.planes[0].as_ref().unwrap().0;
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

fn rect(x: i32, y: i32, width: i32, height: i32) -> NativeCompositionRect {
    NativeCompositionRect {
        x,
        y,
        width,
        height,
    }
}

fn compose(
    context: &mut NativeGbmRenderedScanoutContext<File>,
    image: NativeRendererImageId,
) -> NativeGbmOwnedScanoutBuffer {
    let layers = [NativeCompositionLayer::RendererImage(
        NativeRendererImageCompositionLayer {
            image_id: image,
            target: rect(0, 0, WIDTH as i32, HEIGHT as i32),
            clip: None,
            alpha: 1.0,
            sampling: NativeCompositionSampling::ExactNearest,
        },
    )];
    let report = context.export_composed_owned_scanout_buffer_with_modifiers(
        NativeCompositionFrame {
            width: WIDTH,
            height: HEIGHT,
            layers: &layers,
            trace: None,
            repaint: None,
        },
        &[0],
    );
    report
        .buffer
        .unwrap_or_else(|| panic!("composing snapshot: {:?}", report.detail))
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; uses render node only"]
fn local_snapshots_reuse_storage_without_reading_rewritten_client_pixels() {
    let path = device_path();
    let allocator = gbm::Device::new(open(&path)).unwrap();
    let (mut client, descriptor) = source(&allocator);
    let mut capture = context(&path, true);
    let mut warm = None;
    for sequence in 0..32_u8 {
        write_source(&mut client, &descriptor, sequence);
        let image = NativeRendererImageId::from_raw(u64::from(sequence) + 1);
        assert!(
            capture
                .capture_renderer_image(image, descriptor.frame())
                .unwrap()
        );
        assert!(capture.promote_renderer_image(image).unwrap());
        // Reuse the producer's pixels before a later composition of the image.
        write_source(&mut client, &descriptor, 200);
        let output = compose(&mut capture, image);
        assert_eq!(
            Descriptors::from_buffer(&output).read(&path),
            expected(sequence),
            "immutable content generation {sequence}"
        );
        drop(output);
        assert!(capture.evict_renderer_image(image).unwrap());
        let stats = capture.snapshot_reuse_stats();
        if sequence == 15 {
            warm = Some((
                stats.allocations,
                stats.reuses,
                stats.source_imports,
                capture.persistent_render_stats().import_cache.imports,
            ));
        }
    }
    let stats = capture.snapshot_reuse_stats();
    let (allocations, reuses, source_imports, output_imports) = warm.unwrap();
    assert_eq!(
        stats.source_imports, source_imports,
        "warm source imports stop creating EGLImages"
    );
    assert_eq!(
        capture.persistent_render_stats().import_cache.imports,
        output_imports,
        "warm output imports stop creating EGLImages"
    );
    assert!(
        allocations > 0,
        "the pooled capture path must actually allocate"
    );
    assert_eq!(
        stats.allocations, allocations,
        "warm storage stops allocating"
    );
    assert!(
        stats.reuses > reuses,
        "later captures must reuse retired storage"
    );
    assert!(
        stats.source_hits > 0,
        "the repeated client allocation hits its import"
    );
    assert_eq!(
        stats.source_rebinds, stats.source_hits,
        "every reused source import must be rebound"
    );
    assert_eq!(capture.persistent_render_stats().snapshot_live_entries, 0);
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; uses render node only"]
fn independently_duplicated_snapshot_fds_survive_pool_churn_and_donor_destruction() {
    let path = device_path();
    let allocator = gbm::Device::new(open(&path)).unwrap();
    let (mut client, descriptor) = source(&allocator);
    let mut capture = context(&path, true);
    write_source(&mut client, &descriptor, 1);
    let image = NativeRendererImageId::from_raw(1);
    capture
        .capture_renderer_image(image, descriptor.frame())
        .unwrap();
    capture.promote_renderer_image(image).unwrap();
    let snapshot = capture
        .export_promoted_renderer_image(image)
        .unwrap()
        .unwrap();
    let wrapper = snapshot.try_clone().unwrap();
    let independent_fds = Descriptors::duplicate(wrapper.as_frame());
    drop(wrapper);
    drop(snapshot);
    capture.evict_renderer_image(image).unwrap();

    for sequence in 2..34_u8 {
        write_source(&mut client, &descriptor, sequence);
        let image = NativeRendererImageId::from_raw(u64::from(sequence));
        capture
            .capture_renderer_image(image, descriptor.frame())
            .unwrap();
        capture.promote_renderer_image(image).unwrap();
        let output = compose(&mut capture, image);
        assert_eq!(
            Descriptors::from_buffer(&output).read(&path),
            expected(sequence)
        );
        drop(output);
        capture.evict_renderer_image(image).unwrap();
    }
    let stats = capture.snapshot_reuse_stats();
    assert!(
        stats.reuses > 0,
        "the witness must run beside real pool reuse"
    );
    drop(capture);
    drop(client);
    drop(descriptor);
    drop(allocator);
    assert_eq!(independent_fds.read(&path), expected(1));
    assert!(
        stats.exported_exclusions > 0,
        "raw export disqualifies its allocation"
    );
}

fn gpu_frame(
    producer: &mut NativeGbmRenderedScanoutContext<File>,
    sequence: u8,
) -> NativeGbmOwnedScanoutBuffer {
    let patches = [
        (
            rect(0, 0, WIDTH as i32, HEIGHT as i32),
            [sequence * 3, 29, 47],
        ),
        (rect(0, 0, 3, 2), [213, sequence * 5, 17]),
        (rect(11, 4, 2, 3), [31, 173, sequence * 7]),
        (rect(4, 3, 2, 2), [sequence * 2, 89, 229]),
    ];
    let layers = patches.map(|(target, color)| {
        NativeCompositionLayer::Solid(NativeSolidCompositionLayer { target, color })
    });
    let report = producer.export_composed_owned_scanout_buffer_with_modifiers(
        NativeCompositionFrame {
            width: WIDTH,
            height: HEIGHT,
            layers: &layers,
            trace: None,
            repaint: None,
        },
        &[0],
    );
    report
        .buffer
        .unwrap_or_else(|| panic!("GPU producer: {:?}", report.detail))
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; uses render node only"]
fn gpu_producer_rewrites_cached_source_allocations_with_the_same_pixels_as_fresh_capture() {
    let path = device_path();
    let mut arms = Vec::new();
    for reuse in [false, true] {
        let mut producer = context(&path, false);
        let mut capture = context(&path, reuse);
        let mut allocations = BTreeSet::new();
        let mut reused_source = false;
        let mut pixels = Vec::new();
        for sequence in 1..33_u8 {
            let source = gpu_frame(&mut producer, sequence);
            let descriptors = Descriptors::from_buffer(&source);
            reused_source |= !allocations.insert(descriptors.allocation());
            let image = NativeRendererImageId::from_raw(u64::from(sequence));
            capture
                .capture_renderer_image(image, descriptors.frame())
                .unwrap();
            capture.promote_renderer_image(image).unwrap();
            let output = compose(&mut capture, image);
            // Both GPU operations precede every CPU readback. No CPU map or
            // DMA_BUF_SYNC can hide a missing source refresh before capture.
            let captured_pixels = Descriptors::from_buffer(&output).read(&path);
            assert_eq!(
                captured_pixels,
                descriptors.read(&path),
                "GPU frame {sequence}"
            );
            if let Some(previous) = pixels.last() {
                assert_ne!(
                    &captured_pixels, previous,
                    "the producer really changed pixels"
                );
            }
            pixels.push(captured_pixels);
            drop(output);
            capture.evict_renderer_image(image).unwrap();
            drop(descriptors);
            drop(source);
        }
        assert!(
            reused_source,
            "the producer must rewrite a previously imported allocation"
        );
        if reuse {
            let stats = capture.snapshot_reuse_stats();
            assert!(stats.reuses > 0);
            assert!(
                stats.source_hits > 0,
                "the GPU rewrite must reach an import-cache hit"
            );
            assert_eq!(
                stats.source_rebinds, stats.source_hits,
                "every GPU-updated cache hit must refresh its binding"
            );
        }
        arms.push(pixels);
    }
    assert_eq!(
        arms[0], arms[1],
        "fresh and reused capture preserve the same GPU frames"
    );
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; uses render node only"]
fn clearing_images_does_not_accumulate_completed_gpu_allocations() {
    let path = device_path();
    let allocator = gbm::Device::new(open(&path)).unwrap();
    let (mut client, descriptor) = source(&allocator);
    let mut capture = context(&path, true);
    for sequence in 1..17_u8 {
        write_source(&mut client, &descriptor, sequence);
        let image = NativeRendererImageId::from_raw(u64::from(sequence));
        capture
            .capture_renderer_image(image, descriptor.frame())
            .unwrap();
        capture.promote_renderer_image(image).unwrap();
        let output = compose(&mut capture, image);
        assert_eq!(
            Descriptors::from_buffer(&output).read(&path),
            expected(sequence)
        );
        drop(output);
        capture.clear_renderer_images().unwrap();
        assert_eq!(capture.persistent_render_stats().snapshot_live_entries, 0);
        assert!(
            capture.snapshot_reuse_stats().live_count <= 2,
            "completed clear must release storage, not quarantine it indefinitely"
        );
    }
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; uses render node only"]
fn explicit_import_eviction_reimports_the_same_snapshot_pixels() {
    let path = device_path();
    let allocator = gbm::Device::new(open(&path)).unwrap();
    let (mut client, descriptor) = source(&allocator);
    let mut capture = context(&path, true);
    write_source(&mut client, &descriptor, 3);
    let image = NativeRendererImageId::from_raw(1);
    capture
        .capture_renderer_image(image, descriptor.frame())
        .unwrap();
    capture.promote_renderer_image(image).unwrap();
    let output = compose(&mut capture, image);
    assert_eq!(Descriptors::from_buffer(&output).read(&path), expected(3));
    drop(output);
    let before = capture.persistent_render_stats().import_cache.imports;
    assert!(capture.evict_renderer_image_imports(image).unwrap());
    let output = compose(&mut capture, image);
    assert_eq!(Descriptors::from_buffer(&output).read(&path), expected(3));
    assert_eq!(
        capture.persistent_render_stats().import_cache.imports,
        before + 1
    );
}
