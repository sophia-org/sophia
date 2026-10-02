#![cfg(all(feature = "gbm-platform", target_os = "linux"))]
use sophia_renderer_native_egl::{
    NativeCompositionFrame, NativeCompositionLayer, NativeCompositionRect,
    NativeCompositionSampling, NativeDmaBufPlane, NativeGbmOwnedScanoutBuffer,
    NativeGbmRenderedScanoutContext, NativeMultiPlaneDmaBufFrame, NativePixmapImportProbe,
    NativeRendererImageCompositionLayer, NativeRendererImageId, native_dmabuf_cpu_write_access,
};
use std::{
    fs::{File, OpenOptions},
    os::fd::AsFd,
    path::Path,
};

fn open(path: &Path) -> File {
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}
fn context(path: &Path) -> NativeGbmRenderedScanoutContext<File> {
    NativeGbmRenderedScanoutContext::from_backend_device_result(Ok(open(path)))
        .context
        .expect("render context")
}
fn read(path: &Path, buffer: &NativeGbmOwnedScanoutBuffer) -> Vec<u8> {
    let fds = buffer.export_plane_fds().unwrap().into_plane_fds();
    NativePixmapImportProbe::new(
        open(path),
        NativeMultiPlaneDmaBufFrame {
            width: buffer.width(),
            height: buffer.height(),
            format: buffer.format(),
            modifier: buffer.modifier().unwrap(),
            plane_count: buffer.plane_count(),
            planes: std::array::from_fn(|i| {
                fds[i].as_ref().map(|fd| NativeDmaBufPlane {
                    fd: fd.as_fd(),
                    offset: buffer.plane_offsets()[i],
                    stride: buffer.plane_pitches()[i],
                })
            }),
        },
    )
    .unwrap()
    .read_rgba()
    .unwrap()
}

fn capture_color(
    path: &Path,
    ctx: &mut NativeGbmRenderedScanoutContext<File>,
    id: u64,
    rgba: [u8; 4],
) {
    let allocator = gbm::Device::new(open(path)).unwrap();
    let mut source = allocator
        .create_buffer_object_with_modifiers2::<()>(
            8,
            8,
            gbm::Format::Argb8888,
            std::iter::once(gbm::Modifier::Linear),
            gbm::BufferObjectFlags::RENDERING,
        )
        .unwrap();
    let fd = source.fd_for_plane(0).unwrap();
    native_dmabuf_cpu_write_access(&fd, false).unwrap();
    source
        .map_mut(0, 0, 8, 8, |mapped| {
            let stride = mapped.stride() as usize;
            for y in 0..8 {
                for x in 0..8 {
                    mapped.buffer_mut()[y * stride + x * 4..y * stride + x * 4 + 4]
                        .copy_from_slice(&[rgba[2], rgba[1], rgba[0], rgba[3]]);
                }
            }
        })
        .unwrap();
    native_dmabuf_cpu_write_access(&fd, true).unwrap();
    let id = NativeRendererImageId::from_raw(id);
    assert!(
        ctx.capture_renderer_image(
            id,
            NativeMultiPlaneDmaBufFrame {
                width: 8,
                height: 8,
                format: source.format() as u32,
                modifier: u64::from(source.modifier()),
                plane_count: 1,
                planes: [
                    Some(NativeDmaBufPlane {
                        fd: fd.as_fd(),
                        offset: source.offset(0),
                        stride: source.stride_for_plane(0)
                    }),
                    None,
                    None,
                    None
                ],
            }
        )
        .unwrap()
    );
    assert!(ctx.promote_renderer_image(id).unwrap());
}
fn image(id: u64, x: i32, y: i32, width: i32, height: i32) -> NativeCompositionLayer<'static> {
    NativeCompositionLayer::RendererImage(NativeRendererImageCompositionLayer {
        image_id: NativeRendererImageId::from_raw(id),
        target: NativeCompositionRect {
            x,
            y,
            width,
            height,
        },
        clip: Some(NativeCompositionRect {
            x: 0,
            y: 0,
            width: 32,
            height: 16,
        }),
        alpha: 1.0,
        sampling: NativeCompositionSampling::ExactNearest,
    })
}
#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; render node only, no KMS or windows"]
fn overlay_duplicates_work_but_cross_head_images_need_custody() {
    use sophia_renderer_native_egl::{
        NativeGbmScanoutBufferExportDetail, NativeSolidCompositionLayer,
    };
    let path =
        std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_RENDER_NODE").expect("render node"));
    let mut first = context(&path);
    let mut second = context(&path);
    capture_color(&path, &mut first, 1, [255, 0, 0, 255]);
    capture_color(&path, &mut second, 2, [0, 255, 0, 255]);
    let backdrop = NativeCompositionLayer::Solid(NativeSolidCompositionLayer {
        target: NativeCompositionRect {
            x: 0,
            y: 0,
            width: 32,
            height: 16,
        },
        color: [0, 0, 0],
    });
    let mut layers = vec![image(2, 0, 0, 32, 16), backdrop, image(2, -1, 4, 8, 8)];
    let render = |ctx: &mut NativeGbmRenderedScanoutContext<File>,
                  layers: &[NativeCompositionLayer<'_>]| {
        ctx.export_composed_owned_scanout_buffer_with_modifiers(
            NativeCompositionFrame {
                width: 32,
                height: 16,
                layers,
                trace: None,
                repaint: None,
            },
            &[0],
        )
    };
    let control = render(&mut second, &layers);
    assert!(
        control.buffer.is_some(),
        "same-head normal surface plus clipped instance: {:?}",
        control.detail
    );
    drop(control);
    layers.push(image(1, 16, 4, 8, 8));
    let missing = render(&mut second, &layers);
    eprintln!("cross-head without custody: {:?}", missing.detail);
    assert!(missing.buffer.is_none());
    // Frame admission refuses a renderer-image ID absent from this context.
    assert_eq!(
        missing.detail,
        NativeGbmScanoutBufferExportDetail::InvalidRendererImageId
    );
    // A later source image must be transferred under its own identity too.
    for revision in 1..=4 {
        if revision > 1 {
            capture_color(
                &path,
                &mut first,
                revision + 2,
                [255, revision as u8, 0, 255],
            );
        }
        let id = if revision == 1 { 1 } else { revision + 2 };
        let snapshot = first
            .export_promoted_renderer_image(NativeRendererImageId::from_raw(id))
            .unwrap()
            .unwrap();
        assert!(second.restore_promoted_renderer_image(snapshot).unwrap());
        *layers.last_mut().unwrap() = image(id, 16, 4, 8, 8);
        let output = render(&mut second, &layers);
        let buffer = output
            .buffer
            .unwrap_or_else(|| panic!("transferred image: {:?}", output.detail));
        let pixels = read(&path, &buffer);
        let pixel = |x: usize, y: usize| &pixels[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(pixel(0, 5), &[0, 255, 0, 255], "clipped same-head preview");
        assert_eq!(
            pixel(16, 5),
            &[255, if revision == 1 { 0 } else { revision as u8 }, 0, 255],
            "cross-head revision"
        );
        assert_eq!(pixel(12, 5), &[0, 0, 0, 255], "backdrop");
    }
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; immutable snapshot pixel check, no KMS"]
fn overlay_snapshot_pixels_follow_source_revisions_after_donor_eviction() {
    use sophia_renderer_native_egl::{
        NativeDmaBufCompositionLayer, NativeRendererSnapshotBudget, NativeRendererSnapshotEpoch,
        NativeSolidCompositionLayer,
    };
    let path = std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_RENDER_NODE").unwrap());
    let mut donor = context(&path);
    let mut recipient = context(&path);
    capture_color(&path, &mut recipient, 2, [0, 255, 0, 255]);
    let budget = NativeRendererSnapshotBudget::new(1, 8 * 1024 * 1024);
    let epoch = NativeRendererSnapshotEpoch::new(1, 1, 1);
    for revision in 1..=8_u8 {
        let raw = 100 + u64::from(revision);
        let id = NativeRendererImageId::from_raw(raw);
        let color = [255, revision * 16, 0, 255];
        capture_color(&path, &mut donor, raw, color);
        let snapshot = budget
            .retain(
                donor.export_promoted_renderer_image(id).unwrap().unwrap(),
                epoch.clone(),
            )
            .unwrap();
        donor.evict_renderer_image(id).unwrap();
        let layers = [
            image(2, 0, 0, 32, 16),
            NativeCompositionLayer::Solid(NativeSolidCompositionLayer {
                target: NativeCompositionRect {
                    x: 0,
                    y: 0,
                    width: 32,
                    height: 16,
                },
                color: [0, 0, 0],
            }),
            image(2, -1, 4, 8, 8),
            NativeCompositionLayer::DmaBuf(NativeDmaBufCompositionLayer {
                image_id: id,
                frame: snapshot.as_frame(),
                custody: Some(&snapshot),
                target: NativeCompositionRect {
                    x: 16,
                    y: 4,
                    width: 8,
                    height: 8,
                },
                clip: Some(NativeCompositionRect {
                    x: 0,
                    y: 0,
                    width: 32,
                    height: 16,
                }),
                alpha: 1.0,
                sampling: NativeCompositionSampling::ExactNearest,
            }),
        ];
        let report = recipient.export_composed_owned_scanout_buffer_with_modifiers(
            NativeCompositionFrame {
                width: 32,
                height: 16,
                layers: &layers,
                trace: None,
                repaint: None,
            },
            &[0],
        );
        let buffer = report
            .buffer
            .unwrap_or_else(|| panic!("revision {revision}: {:?}", report.detail));
        let pixels = read(&path, &buffer);
        for y in 0..16 {
            for x in 0..32 {
                let expected = if (4..12).contains(&y) && x < 7 {
                    [0, 255, 0, 255]
                } else if (4..12).contains(&y) && (16..24).contains(&x) {
                    color
                } else {
                    [0, 0, 0, 255]
                };
                assert_eq!(
                    &pixels[(y * 32 + x) * 4..(y * 32 + x + 1) * 4],
                    &expected,
                    "revision {revision}, ({x}, {y})"
                );
            }
        }
        recipient.evict_renderer_image(id).unwrap();
        drop(snapshot);
        assert_eq!(
            budget.usage().0,
            1,
            "the output buffer still owns the sampled source"
        );
        drop(buffer);
        assert_eq!(budget.usage(), (0, 0));
    }
}

#[test]
#[ignore = "requires SOPHIA_TEST_RENDER_NODE; three contexts, no KMS"]
fn a_cached_snapshot_can_change_donor_without_changing_image_identity() {
    use sophia_renderer_native_egl::{
        NativeDmaBufCompositionLayer, NativeRendererSnapshotBudget, NativeRendererSnapshotEpoch,
    };
    let path = std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_RENDER_NODE").unwrap());
    let mut donors = [context(&path), context(&path)];
    let mut recipient = context(&path);
    let budget = NativeRendererSnapshotBudget::new(2, 16 * 1024 * 1024);
    let id = NativeRendererImageId::from_raw(501);
    let color = [17, 113, 205, 255];
    let mut snapshots = Vec::new();
    for (i, donor) in donors.iter_mut().enumerate() {
        capture_color(&path, donor, id.raw(), color);
        snapshots.push(
            budget
                .retain(
                    donor.export_promoted_renderer_image(id).unwrap().unwrap(),
                    NativeRendererSnapshotEpoch::new(1, 1, i as u64 + 1),
                )
                .unwrap(),
        );
    }
    // The same target cache sees alternating immutable copies, without an
    // intervening eviction. A plain ID hit used to reject the new descriptor.
    for turn in 0..8 {
        let snapshot = &snapshots[turn % 2];
        let layers = [NativeCompositionLayer::DmaBuf(
            NativeDmaBufCompositionLayer {
                image_id: id,
                frame: snapshot.as_frame(),
                custody: Some(snapshot),
                target: NativeCompositionRect {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 8,
                },
                clip: None,
                alpha: 1.0,
                sampling: NativeCompositionSampling::ExactNearest,
            },
        )];
        let report = recipient.export_composed_owned_scanout_buffer_with_modifiers(
            NativeCompositionFrame {
                width: 8,
                height: 8,
                layers: &layers,
                trace: None,
                repaint: None,
            },
            &[0],
        );
        let buffer = report
            .buffer
            .unwrap_or_else(|| panic!("turn {turn}: {:?}", report.detail));
        let pixels = read(&path, &buffer);
        assert!(pixels.chunks_exact(4).all(|pixel| pixel == color));
    }
    recipient.evict_renderer_image_imports(id).unwrap();
    drop(snapshots);
    assert_eq!(budget.usage(), (0, 0));
}
