#![cfg(all(feature = "gbm-platform", target_os = "linux"))]

use std::{
    fs::{File, OpenOptions},
    os::{
        fd::{AsFd, OwnedFd},
        unix::fs::MetadataExt,
    },
    path::{Path, PathBuf},
    time::Instant,
};

use sophia_renderer_native_egl::{
    NATIVE_IMAGE_IMPORT_DEVICE_CAPACITY, NativeCompositionFrame, NativeCompositionLayer,
    NativeCompositionRect, NativeCompositionSampling, NativeDmaBufPlane,
    NativeGbmOwnedScanoutBuffer, NativeGbmRenderedScanoutContext,
    NativeGbmScanoutBufferExportDetail as Error, NativeMultiPlaneDmaBufFrame,
    NativePixmapImportProbe, NativeRendererImageCompositionLayer, NativeRendererImageId,
    native_dmabuf_cpu_write_access, query_native_dmabuf_import_formats,
};

const WIDTH: u32 = 1920;
const HEIGHT: u32 = 1080;
fn warm_captures() -> u32 {
    let count = std::env::var("SOPHIA_TRANSFER_BENCHMARK_FRAMES").map_or(20, |value| {
        value.parse().expect("numeric benchmark frame count")
    });
    assert!(
        (1..=600).contains(&count),
        "benchmark frame count must be 1..=600"
    );
    count
}
const MAX_MODIFIER_ATTEMPTS: usize = 64;

fn open(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap_or_else(|error| panic!("open {}: {error}", path.display()))
}

fn nodes() -> [PathBuf; 2] {
    let paths = ["SOPHIA_TEST_RENDER_NODE", "SOPHIA_TEST_OTHER_RENDER_NODE"]
        .map(|name| PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name}"))));
    assert_ne!(
        open(&paths[0]).metadata().unwrap().rdev(),
        open(&paths[1]).metadata().unwrap().rdev(),
        "select distinct render nodes",
    );
    paths
}

fn context(path: &Path) -> NativeGbmRenderedScanoutContext<File> {
    let report = NativeGbmRenderedScanoutContext::from_backend_device_result(Ok(open(path)));
    report
        .context
        .unwrap_or_else(|| panic!("initialize {}: {:?}", path.display(), report.status))
}

fn expected(sequence: u32) -> Vec<u8> {
    (0..HEIGHT)
        .flat_map(move |y| {
            (0..WIDTH).flat_map(move |x| {
                [
                    (x * 17 + 11 + sequence * 7) as u8,
                    (y * 29 + 13 + sequence * 11) as u8,
                    ((x + y) * 31 + 7 + sequence * 13) as u8,
                    255,
                ]
            })
        })
        .collect()
}

fn assert_pixels(actual: &[u8], expected: &[u8], stage: &str) {
    assert_eq!(actual.len(), expected.len(), "{stage}: RGBA byte count");
    let mismatch = actual
        .iter()
        .zip(expected)
        .enumerate()
        .find(|(_, (actual, expected))| actual != expected)
        .map(|(index, (actual, expected))| (index, *actual, *expected));
    assert_eq!(mismatch, None, "{stage}: first mismatching RGBA byte");
}

struct Source {
    buffer: gbm::BufferObject<()>,
    fds: Vec<OwnedFd>,
}

impl Source {
    fn from_buffer(buffer: gbm::BufferObject<()>) -> Self {
        let count = buffer.plane_count();
        assert!((1..=4).contains(&count));
        let fds = (0..count)
            .map(|index| {
                buffer
                    .fd_for_plane(index as i32)
                    .expect("export source plane")
            })
            .collect::<Vec<_>>();
        let mut source = Self { buffer, fds };
        source.write_pixels(&expected(0));
        source
    }

    fn write_pixels(&mut self, pixels: &[u8]) {
        assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);
        native_dmabuf_cpu_write_access(&self.fds[0], false).expect("begin source write");
        let written = self.buffer.map_mut(0, 0, WIDTH, HEIGHT, |mapping| {
            let stride = mapping.stride() as usize;
            let bytes = mapping.buffer_mut();
            if stride < WIDTH as usize * 4 || bytes.len() < stride * HEIGHT as usize {
                return Err("invalid source mapping extent");
            }
            bytes.fill(0);
            for y in 0..HEIGHT as usize {
                for x in 0..WIDTH as usize {
                    let input = (y * WIDTH as usize + x) * 4;
                    let output = y * stride + x * 4;
                    bytes[output..output + 4].copy_from_slice(&[
                        pixels[input + 2],
                        pixels[input + 1],
                        pixels[input],
                        pixels[input + 3],
                    ]);
                }
            }
            Ok(())
        });
        let ended = native_dmabuf_cpu_write_access(&self.fds[0], true);
        written.expect("map source").expect("write source");
        ended.expect("end source write");
    }

    fn frame(&self) -> NativeMultiPlaneDmaBufFrame<'_> {
        NativeMultiPlaneDmaBufFrame {
            width: WIDTH,
            height: HEIGHT,
            format: self.buffer.format() as u32,
            modifier: u64::from(self.buffer.modifier()),
            plane_count: self.fds.len() as u8,
            planes: std::array::from_fn(|index| {
                self.fds.get(index).map(|fd| NativeDmaBufPlane {
                    fd: fd.as_fd(),
                    offset: self.buffer.offset(index as i32),
                    stride: self.buffer.stride_for_plane(index as i32),
                })
            }),
        }
    }
}

fn import_failure(error: Error) -> bool {
    matches!(
        error,
        Error::DmaBufImageCreateFailed | Error::DmaBufImageBindFailed | Error::DmaBufImportFailed
    )
}

fn discriminating_source(
    source: &Path,
    destination: &Path,
    format: gbm::Format,
    id: NativeRendererImageId,
) -> (Source, NativeGbmRenderedScanoutContext<File>) {
    let inventory =
        query_native_dmabuf_import_formats(open(source)).expect("query source import capabilities");
    let modifiers = inventory
        .iter()
        .find(|entry| entry.format == format as u32)
        .unwrap_or_else(|| {
            panic!(
                "source {} has no {format:?} import capability",
                source.display()
            )
        });
    let allocator = gbm::Device::new(open(source)).expect("create source allocator");
    let mut target = context(destination);
    let mut attempts = Vec::new();
    for modifier in modifiers
        .modifiers
        .iter()
        .copied()
        .filter(|modifier| {
            !matches!(*modifier, 0 | u64::MAX) && *modifier != u64::from(gbm::Modifier::Invalid)
        })
        .take(MAX_MODIFIER_ATTEMPTS)
    {
        let buffer = match allocator.create_buffer_object_with_modifiers2::<()>(
            WIDTH,
            HEIGHT,
            format,
            std::iter::once(gbm::Modifier::from(modifier)),
            gbm::BufferObjectFlags::RENDERING,
        ) {
            Ok(buffer) => buffer,
            Err(error) => {
                attempts.push(format!("{modifier:#x}: allocation {error}"));
                continue;
            }
        };
        assert_eq!(u64::from(buffer.modifier()), modifier);
        let candidate = Source::from_buffer(buffer);
        match NativePixmapImportProbe::new(open(source), candidate.frame()) {
            Ok(probe) => assert_pixels(
                &probe.read_rgba().expect("source readback"),
                &expected(0),
                "source image before migration",
            ),
            Err(error) => {
                attempts.push(format!("{modifier:#x}: source import {error:?}"));
                continue;
            }
        }
        match target.capture_renderer_image(id, candidate.frame()) {
            Err(error) if import_failure(error) => {
                assert_eq!(target.persistent_render_stats().snapshot_live_entries, 0);
                assert_eq!(target.image_transfer_stats().captures, 0);
                eprintln!(
                    "transfer_discriminator source={} destination={} format={:#x} modifier={modifier:#x} direct={error:?}",
                    source.display(),
                    destination.display(),
                    format as u32
                );
                return (candidate, target);
            }
            Ok(true) => {
                assert!(
                    target
                        .rollback_renderer_image(id)
                        .expect("discard directly imported candidate")
                );
                attempts.push(format!("{modifier:#x}: direct import succeeded"));
            }
            Ok(false) => panic!("candidate identity was unexpectedly retained"),
            Err(error) => panic!("direct capture failed outside import: {error:?}"),
        }
    }
    panic!(
        "no actual cross-device refusal for {format:?}, {} -> {} within {MAX_MODIFIER_ATTEMPTS} candidates: {attempts:?}",
        source.display(),
        destination.display()
    );
}

fn composed_pixels(
    context: &mut NativeGbmRenderedScanoutContext<File>,
    path: &Path,
    id: NativeRendererImageId,
) -> Vec<u8> {
    composed_pixels_with_background(context, path, id, None)
}

fn composed_pixels_with_background(
    context: &mut NativeGbmRenderedScanoutContext<File>,
    path: &Path,
    id: NativeRendererImageId,
    background: Option<[u8; 3]>,
) -> Vec<u8> {
    let mut layers = Vec::new();
    if let Some(color) = background {
        layers.push(NativeCompositionLayer::Solid(
            sophia_renderer_native_egl::NativeSolidCompositionLayer {
                target: NativeCompositionRect {
                    x: 0,
                    y: 0,
                    width: WIDTH as i32,
                    height: HEIGHT as i32,
                },
                color,
            },
        ));
    }
    layers.push(NativeCompositionLayer::RendererImage(
        NativeRendererImageCompositionLayer {
            image_id: id,
            target: NativeCompositionRect {
                x: 0,
                y: 0,
                width: WIDTH as i32,
                height: HEIGHT as i32,
            },
            clip: None,
            alpha: 1.0,
            sampling: NativeCompositionSampling::ExactNearest,
        },
    ));
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
    let buffer = report
        .buffer
        .unwrap_or_else(|| panic!("compose retained image: {:?}", report.detail));
    read_buffer(path, &buffer)
}

fn read_buffer(path: &Path, buffer: &NativeGbmOwnedScanoutBuffer) -> Vec<u8> {
    let fds = buffer
        .export_plane_fds()
        .expect("export composed image")
        .into_plane_fds();
    let offsets = buffer.plane_offsets();
    let strides = buffer.plane_pitches();
    let probe = NativePixmapImportProbe::new(
        open(path),
        NativeMultiPlaneDmaBufFrame {
            width: buffer.width(),
            height: buffer.height(),
            format: buffer.format(),
            modifier: buffer.modifier().expect("explicit composed modifier"),
            plane_count: buffer.plane_count(),
            planes: std::array::from_fn(|index| {
                fds[index].as_ref().map(|fd| NativeDmaBufPlane {
                    fd: fd.as_fd(),
                    offset: offsets[index],
                    stride: strides[index],
                })
            }),
        },
    )
    .expect("import composed image for readback");
    probe.read_rgba().expect("read composed image")
}

fn measure_warm_captures(
    source: &mut Source,
    target: &mut NativeGbmRenderedScanoutContext<File>,
    source_path: &Path,
    target_path: &Path,
    format: gbm::Format,
) {
    let baseline = target.image_transfer_stats();
    let target_allocations = target.persistent_render_stats().dmabuf_target_creations;
    let captures = warm_captures();
    let live_images = target.persistent_render_stats().snapshot_live_entries;
    let retained_allocations = target.snapshot_reuse_stats().live_count;
    let retained_bytes = target.snapshot_reuse_stats().live_bytes;
    let mut capture_us = Vec::new();
    let mut through_readback_us = Vec::new();
    for sequence in 1..=captures {
        let pixels = expected(sequence);
        // The preceding destination readback completed its capture before this
        // source write. CPU preparation is outside both timing measurements.
        source.write_pixels(&pixels);
        let id = NativeRendererImageId::from_raw(100 + u64::from(sequence));
        let started = Instant::now();
        assert!(
            target
                .capture_renderer_image(id, source.frame())
                .unwrap_or_else(|error| panic!(
                    "warm capture {sequence}: {error:?}; {:?}",
                    target.image_transfer_stats()
                ))
        );
        capture_us.push(started.elapsed().as_micros());
        let actual = composed_pixels(target, target_path, id);
        through_readback_us.push(started.elapsed().as_micros());
        assert_pixels(&actual, &pixels, &format!("warm capture {sequence}"));
        assert!(target.evict_renderer_image(id).unwrap());
        assert_eq!(
            target.persistent_render_stats().snapshot_live_entries,
            live_images
        );
        assert_eq!(
            target.image_transfer_stats().captures,
            baseline.captures + u64::from(sequence)
        );
        let reuse = target.snapshot_reuse_stats();
        assert!(
            reuse.live_count <= retained_allocations && reuse.live_bytes <= retained_bytes,
            "refused direct imports must not accumulate unused destination storage"
        );
    }
    assert_eq!(
        target.image_transfer_stats().device_initializations,
        baseline.device_initializations
    );
    let after = target.image_transfer_stats();
    assert_eq!(
        after.bridge_allocations, baseline.bridge_allocations,
        "completed scratch is reused"
    );
    assert_eq!(
        after.bridge_reuses,
        baseline.bridge_reuses + u64::from(captures)
    );
    assert_eq!(after.bridge_live_slots, 1);
    assert_eq!(after.bridge_live_bytes, baseline.bridge_live_bytes);
    assert_eq!(
        target.persistent_render_stats().dmabuf_target_creations,
        target_allocations + captures as usize,
        "direct refusal allocates no throwaway target"
    );
    let percentile = |values: &[u128], percent: usize| {
        let mut ordered = values.to_vec();
        ordered.sort_unstable();
        ordered[(ordered.len() * percent).div_ceil(100) - 1]
    };
    // Host-call time can include driver waits. The second measurement includes
    // composition, probe context creation and GPU readback, not display latency.
    eprintln!(
        "transfer_warm source={} destination={} format={:#x} size={}x{} samples={} capture_host_us={capture_us:?} capture_host_p50_us={} capture_host_p95_us={} through_readback_us={through_readback_us:?} through_readback_p50_us={} through_readback_p95_us={} stats={:?}",
        source_path.display(),
        target_path.display(),
        format as u32,
        WIDTH,
        HEIGHT,
        captures,
        percentile(&capture_us, 50),
        percentile(&capture_us, 95),
        percentile(&through_readback_us, 50),
        percentile(&through_readback_us, 95),
        target.image_transfer_stats()
    );
}

#[test]
#[ignore = "requires two render nodes with actually incompatible explicit AR24/XR24 layouts"]
fn foreign_images_transfer_once_and_retain_exact_pixels_after_context_replacement() {
    let paths = nodes();
    for (source_index, target_index) in [(0, 1), (1, 0)] {
        for format in [gbm::Format::Argb8888, gbm::Format::Xrgb8888] {
            let source_path = &paths[source_index];
            let target_path = &paths[target_index];
            let id = NativeRendererImageId::from_raw(71);
            let (mut source, mut target) =
                discriminating_source(source_path, target_path, format, id);
            target
                .set_image_import_devices(vec![open(source_path).into()])
                .expect("admit source device after direct refusal");
            assert!(
                target
                    .capture_renderer_image(id, source.frame())
                    .unwrap_or_else(|error| panic!(
                        "capture through GPU bridge: {error:?}; {:?}",
                        target.image_transfer_stats()
                    ))
            );
            let captured = target.image_transfer_stats();
            assert_eq!(captured.captures, 1);
            assert_eq!(captured.device_initializations, 1);
            assert!(captured.attempts > 0);
            assert!(captured.bridge_bytes >= u64::from(WIDTH * HEIGHT * 4));
            assert!(
                !target
                    .capture_renderer_image(id, source.frame())
                    .expect("reuse immutable capture")
            );
            assert_eq!(
                target.image_transfer_stats(),
                captured,
                "same identity must not copy again"
            );
            assert!(
                target.export_promoted_renderer_image(id).unwrap().is_none(),
                "staged image cannot be exported"
            );
            assert_pixels(
                &composed_pixels(&mut target, target_path, id),
                &expected(0),
                "staged image",
            );
            measure_warm_captures(&mut source, &mut target, source_path, target_path, format);
            assert_pixels(
                &composed_pixels(&mut target, target_path, id),
                &expected(0),
                "earlier immutable image after source changes",
            );

            let rollback_id = NativeRendererImageId::from_raw(72);
            assert!(
                target
                    .capture_renderer_image(rollback_id, source.frame())
                    .expect("capture rollback candidate")
            );
            assert!(target.rollback_renderer_image(rollback_id).unwrap());
            assert!(!target.promote_renderer_image(rollback_id).unwrap());
            assert!(
                target
                    .export_promoted_renderer_image(rollback_id)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(target.persistent_render_stats().snapshot_live_entries, 1);
            assert!(target.promote_renderer_image(id).unwrap());
            assert!(
                !target.rollback_renderer_image(id).unwrap(),
                "rollback must preserve a promoted image"
            );
            let snapshot = target
                .export_promoted_renderer_image(id)
                .unwrap()
                .expect("export promoted snapshot");
            assert_eq!(snapshot.image_id(), id);
            eprintln!(
                "transfer_proof source={} destination={} format={:#x} stats={captured:?}",
                source_path.display(),
                target_path.display(),
                format as u32
            );
            drop(target);
            drop(source);
            let mut replacement = context(target_path);
            assert!(
                replacement
                    .restore_promoted_renderer_image(snapshot)
                    .expect("restore retained snapshot")
            );
            assert_pixels(
                &composed_pixels(&mut replacement, target_path, id),
                &expected(0),
                "restored image",
            );
            assert_eq!(
                replacement.image_transfer_stats().captures,
                0,
                "destination-owned snapshot needs no foreign-device transfer"
            );
            assert!(replacement.evict_renderer_image(id).unwrap());
            assert_eq!(
                replacement.persistent_render_stats().snapshot_live_entries,
                0
            );
        }
    }
}

#[test]
#[ignore = "requires two explicitly selected render nodes; opens no windows"]
fn device_inventory_rejects_overflow_without_consuming_its_single_assignment() {
    let paths = nodes();
    let mut target = context(&paths[0]);
    let too_many = (0..=NATIVE_IMAGE_IMPORT_DEVICE_CAPACITY)
        .map(|_| open(&paths[1]).into())
        .collect();
    assert_eq!(
        target.set_image_import_devices(too_many),
        Err(Error::InvalidTarget)
    );
    assert!(
        target
            .set_image_import_devices(vec![open(&paths[1]).into()])
            .is_ok()
    );
    assert_eq!(
        target.set_image_import_devices(Vec::new()),
        Err(Error::InvalidTarget)
    );
    assert_eq!(
        target.image_transfer_stats().device_initializations,
        0,
        "inventory registration must remain lazy"
    );
}

#[test]
#[ignore = "requires two selected GPUs with incompatible explicit images; opens no windows"]
fn a_local_image_still_imports_directly_and_source_refresh_preserves_retained_pixels() {
    let paths = nodes();
    let foreign_id = NativeRendererImageId::from_raw(901);
    let local_id = NativeRendererImageId::from_raw(902);
    let (mut source, mut target) =
        discriminating_source(&paths[1], &paths[0], gbm::Format::Argb8888, foreign_id);
    let foreign = source.frame();
    let error = target
        .capture_renderer_image(foreign_id, foreign)
        .expect_err("fixture needs incompatible foreign image");
    assert!(
        import_failure(error),
        "actual foreign FD refusal: {error:?}"
    );
    target
        .set_image_import_devices(vec![open(&paths[1]).into()])
        .unwrap();
    target.capture_renderer_image(foreign_id, foreign).unwrap();
    assert_pixels(
        &composed_pixels(&mut target, &paths[0], foreign_id),
        &expected(0),
        "foreign image",
    );
    let before = target.image_transfer_stats();
    let device = gbm::Device::new(open(&paths[0])).unwrap();
    let local = Source::from_buffer(
        device
            .create_buffer_object::<()>(
                WIDTH,
                HEIGHT,
                gbm::Format::Argb8888,
                gbm::BufferObjectFlags::RENDERING,
            )
            .unwrap(),
    );
    let local_frame = local.frame();
    target
        .capture_renderer_image(local_id, local_frame)
        .unwrap();
    assert_eq!(
        target.image_transfer_stats().captures,
        before.captures,
        "a layout hint cannot force a foreign copy"
    );
    assert_eq!(target.image_transfer_stats().attempts, before.attempts);
    assert_pixels(
        &composed_pixels(&mut target, &paths[0], local_id),
        &expected(0),
        "local image",
    );
    let alpha_id = NativeRendererImageId::from_raw(903);
    let mut alpha_pixels = expected(3);
    for pixel in alpha_pixels.chunks_exact_mut(4) {
        pixel[0] /= 2;
        pixel[1] /= 2;
        pixel[2] /= 2;
        pixel[3] = 128;
    }
    source.write_pixels(&alpha_pixels);
    target
        .capture_renderer_image(alpha_id, source.frame())
        .unwrap();
    let mut over_white = alpha_pixels.clone();
    for pixel in over_white.chunks_exact_mut(4) {
        pixel[0] += 127;
        pixel[1] += 127;
        pixel[2] += 127;
        pixel[3] = 255;
    }
    assert_pixels(
        &composed_pixels_with_background(&mut target, &paths[0], alpha_id, Some([255; 3])),
        &over_white,
        "bridge retains premultiplied alpha",
    );
    assert!(
        target.replace_image_import_devices(Vec::new()).unwrap(),
        "readback completed scratch use"
    );
    assert_eq!(target.image_transfer_stats().bridge_live_slots, 0);
    assert_eq!(target.image_transfer_stats().bridge_live_bytes, 0);
    assert_pixels(
        &composed_pixels(&mut target, &paths[0], foreign_id),
        &expected(0),
        "source inventory removal preserves captured image",
    );
}
