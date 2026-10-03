use sophia_protocol::{
    BufferSource, NamespaceId, Rect, Region, Size, SurfaceConstraints, SurfaceId, TransactionId,
};
use sophia_x_authority::*;

const NS: NamespaceId = NamespaceId::from_raw(46);
const PARENT: XResourceId = XResourceId::new(0x400003, 1);
const CHILD: XResourceId = XResourceId::new(0x40000b, 1);
const PIXMAP: XResourceId = XResourceId::new(0x400010, 1);
const SURFACE: SurfaceId = SurfaceId::new(46, 1);

fn fixture(width: i32, height: i32, offset: i32) -> XAuthorityRuntime {
    let mut runtime = XAuthorityRuntime::new();
    for (window, surface, geometry) in [
        (
            PARENT,
            SURFACE,
            Rect {
                x: 100,
                y: 200,
                width: width + offset,
                height: height + offset,
            },
        ),
        (
            CHILD,
            SurfaceId::new(47, 1),
            Rect {
                x: offset,
                y: offset,
                width,
                height,
            },
        ),
    ] {
        let response = runtime.apply(XAuthorityRequestPacket {
            transaction: TransactionId::from_raw(u64::from(surface.index())),
            namespace: NS,
            kind: XAuthorityRequestKind::CreateWindow {
                window,
                surface,
                geometry,
                constraints: SurfaceConstraints {
                    min_size: None,
                    max_size: None,
                },
                generation: 1,
            },
        });
        assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    }
    runtime.set_window_parent(NS, CHILD, PARENT).unwrap();
    runtime
}

fn upload(runtime: &mut XAuthorityRuntime, drawable: XResourceId, rect: Rect, data: &[u8]) {
    runtime.begin_dispatch();
    let response = runtime.apply_put_image(
        TransactionId::from_raw(100),
        NS,
        drawable,
        Region::single(rect),
        Some(data),
        Some(&XPutImageSemantics {
            format: 2,
            depth: 24,
            left_pad: 0,
            byte_order: XByteOrder::LittleEndian,
            gc: XGraphicsContextValues::default(),
        }),
    );
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
}

#[test]
fn dri3_present_replaces_a_previous_cpu_publication() {
    // Firefox paints its startup background through core X, then switches to
    // DRI3 on a child. Retaining that CPU background must not select it again.
    let mut runtime = fixture(40, 20, 0);
    upload(
        &mut runtime,
        PARENT,
        Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        },
        &[0; 40 * 20 * 4],
    );
    let old = runtime.take_cpu_buffer_update().unwrap().handle();
    let descriptor = runtime
        .create_dri3_pixmap(NS, PIXMAP, 1, 40 * 20 * 4, 40, 20, 160, 24, 32)
        .unwrap();
    runtime.begin_dispatch();
    let response = runtime.present_standard_pixmap(
        TransactionId::from_raw(101),
        NS,
        CHILD,
        PIXMAP,
        0,
        0,
        None,
        None,
    );
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions[0].surface, SURFACE);
    assert_eq!(
        response.transactions[0].target_buffer(),
        BufferSource::DmaBuf {
            handle: descriptor.handle.raw()
        },
        "the retained CPU background {old} must not replace the requested DRI3 source"
    );
    assert!(runtime.take_cpu_buffer_updates().is_empty());
}

#[test]
fn firefox_strip_uploads_present_on_the_parent_and_preserve_later_patches() {
    let (width, height, offset) = (1266, 1408, 3);
    let mut runtime = fixture(width, height, offset);
    runtime
        .create_pixmap(NS, PIXMAP, Size { width, height }, 24, 1)
        .unwrap();
    let mut expected = Vec::new();
    for y in 0..height {
        for x in 0..width {
            expected.extend_from_slice(&[x as u8, y as u8, 0x7f, 0]);
        }
    }
    for y in (0..height).step_by(51) {
        let rows = (height - y).min(51);
        let start = (y * width * 4) as usize;
        let end = ((y + rows) * width * 4) as usize;
        upload(
            &mut runtime,
            PIXMAP,
            Rect {
                x: 0,
                y,
                width,
                height: rows,
            },
            &expected[start..end],
        );
    }
    runtime.begin_dispatch();
    let response = runtime.present_standard_pixmap(
        TransactionId::from_raw(102),
        NS,
        CHILD,
        PIXMAP,
        0,
        0,
        None,
        None,
    );
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions[0].surface, SURFACE);
    let XAuthorityCpuBufferUpdate::Replace(snapshot) = runtime.take_cpu_buffer_update().unwrap()
    else {
        panic!("first publication must replace")
    };
    assert_eq!(snapshot.drawable, PARENT);
    assert_eq!(response.transactions[0].raster_extent(), snapshot.size);
    for y in 0..height {
        let start = ((y + offset) * snapshot.size.width * 4 + offset * 4) as usize;
        assert_eq!(
            &snapshot.bytes[start..start + width as usize * 4],
            &expected[(y * width * 4) as usize..((y + 1) * width * 4) as usize]
        );
    }
    let rect = Rect {
        x: 12,
        y: 52,
        width: 1,
        height: 1,
    };
    upload(&mut runtime, PIXMAP, rect, &[0x44, 0x55, 0x66, 0]);
    runtime.begin_dispatch();
    let response = runtime.present_standard_pixmap(
        TransactionId::from_raw(103),
        NS,
        CHILD,
        PIXMAP,
        0,
        0,
        None,
        Some(Region::single(rect)),
    );
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions[0].surface, SURFACE);
    let XAuthorityCpuBufferUpdate::PatchBatch(batch) = runtime.take_cpu_buffer_update().unwrap()
    else {
        panic!("later publication must patch")
    };
    assert_eq!(batch.handle, snapshot.handle);
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: rect.x + offset,
            y: rect.y + offset,
            ..rect
        })
    );
}

#[test]
fn only_untransformed_top_level_present_supplies_source_pixel_damage() {
    let small = Rect {
        x: 2,
        y: 3,
        width: 4,
        height: 5,
    };
    let full = Region::single(Rect {
        x: 0,
        y: 0,
        width: 40,
        height: 20,
    });
    // Coordinate ambiguity must not turn into undersized output repaint.
    for (label, window, child_offset, x_offset, valid, damage, precise) in [
        ("top-level", PARENT, 0, 0, None, Region::single(small), true),
        ("child", CHILD, 0, 0, None, Region::single(small), false),
        ("offset", PARENT, 0, 1, None, Region::single(small), false),
        (
            "size mismatch",
            PARENT,
            1,
            0,
            None,
            Region::single(small),
            false,
        ),
        (
            "valid region",
            PARENT,
            0,
            0,
            Some(Region::single(small)),
            Region::single(small),
            false,
        ),
        (
            "complex",
            PARENT,
            0,
            0,
            None,
            Region {
                rects: vec![small; 33],
            },
            false,
        ),
    ] {
        let mut runtime = fixture(40, 20, child_offset);
        runtime
            .create_dri3_pixmap(NS, PIXMAP, 1, 40 * 20 * 4, 40, 20, 160, 24, 32)
            .unwrap();
        runtime.begin_dispatch();
        let response = runtime.present_standard_pixmap(
            TransactionId::from_raw(201),
            NS,
            window,
            PIXMAP,
            x_offset,
            0,
            valid,
            Some(damage.clone()),
        );
        assert_eq!(
            response.outcome,
            XAuthorityResponseOutcome::Accepted,
            "{label}"
        );
        assert_eq!(
            response.transactions[0].content.canonical_variant().damage,
            if precise { damage } else { full.clone() },
            "{label}"
        );
    }
}

#[test]
fn ust_requests_are_refused_before_either_source_is_published() {
    for dma_buf in [false, true] {
        for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
            let mut runtime = fixture(40, 20, 0);
            if dma_buf {
                runtime
                    .create_dri3_pixmap(NS, PIXMAP, 1, 3200, 40, 20, 160, 24, 32)
                    .unwrap();
            } else {
                runtime
                    .create_pixmap(
                        NS,
                        PIXMAP,
                        Size {
                            width: 40,
                            height: 20,
                        },
                        24,
                        1,
                    )
                    .unwrap();
                upload(
                    &mut runtime,
                    PIXMAP,
                    Rect {
                        x: 0,
                        y: 0,
                        width: 40,
                        height: 20,
                    },
                    &[0x5a; 3200],
                );
            }
            let context = XDispatchContext {
                byte_order,
                namespace: NS,
                transaction: TransactionId::from_raw(301),
                sequence: 93,
                major_opcode: X_PRESENT_MAJOR_OPCODE,
                client_id: 1,
                injection: XTestAdmission::Absent,
                server_time: 1,
            };
            let mut atoms = XAtomTable::new();
            let mut properties = XPropertyTable::new();
            let capabilities = dispatch_x11_wire_request(
                context,
                XWireRequest::Present(XPresentRequest::PresentQueryCapabilities { target: PARENT }),
                &mut runtime,
                &mut atoms,
                &mut properties,
            );
            assert!(
                matches!(capabilities.outputs.as_slice(), [XClientOutput::Reply(
                XClientReply::PresentQueryCapabilities { capabilities, .. })] if capabilities & 4 == 0)
            );
            for options in [4, 5, 6, 12, 15, 0] {
                runtime.begin_dispatch();
                let result = dispatch_x11_wire_request(
                    context,
                    XWireRequest::Present(XPresentRequest::PresentPixmap {
                        transaction: context.transaction,
                        window: PARENT,
                        pixmap: PIXMAP,
                        serial: 1,
                        valid_region: 0,
                        update_region: 0,
                        x_offset: 0,
                        y_offset: 0,
                        target_crtc: 0,
                        wait_fence: None,
                        idle_fence: None,
                        options,
                        target_msc: 1_000_000_000_000,
                        divisor: 1_000_000,
                        remainder: 0,
                        notifies: Vec::new(),
                    }),
                    &mut runtime,
                    &mut atoms,
                    &mut properties,
                );
                if options == 0 {
                    // Control: an ordinary MSC request still reaches the
                    // existing admission path on the same connection state.
                    assert_eq!(result.response.unwrap().transactions.len(), 1);
                    assert_eq!(runtime.take_cpu_buffer_updates().is_empty(), dma_buf);
                } else {
                    assert!(result.response.is_none());
                    assert!(runtime.take_cpu_buffer_updates().is_empty());
                    let [XClientOutput::Error(error)] = result.outputs.as_slice() else {
                        panic!("UST request produced a publication or success: {result:?}");
                    };
                    assert_eq!(error.code, XErrorCode::BadValue);
                    assert_eq!(error.resource_id, options);
                    assert_eq!(error.sequence, context.sequence);
                    assert_eq!(error.minor_code, u16::from(X_PRESENT_PIXMAP_MINOR_OPCODE));
                }
            }
        }
    }
}

fn prepare(
    runtime: &mut XAuthorityRuntime,
    client: u64,
    transaction: u64,
    window: XResourceId,
) -> Result<(), XPresentPreparationError> {
    runtime.prepare_standard_pixmap(
        client,
        TransactionId::from_raw(transaction),
        NS,
        window,
        PIXMAP,
        (0, 0),
        None,
        None,
        XPresentFenceResources::default(),
    )
}

#[test]
fn preparation_does_not_change_window_pixels_or_the_next_content_generation() {
    let rect = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };
    let mut runtimes = [fixture(4, 4, 0), fixture(4, 4, 0)];
    for runtime in &mut runtimes {
        runtime
            .create_pixmap(
                NS,
                PIXMAP,
                Size {
                    width: 4,
                    height: 4,
                },
                24,
                1,
            )
            .unwrap();
        upload(runtime, PARENT, rect, &[0x11; 64]);
        upload(runtime, PIXMAP, rect, &[0x55; 64]);
        runtime.begin_dispatch();
    }
    prepare(&mut runtimes[0], 1, 700, PARENT).unwrap();
    assert!(runtimes[0].take_cpu_buffer_updates().is_empty());
    assert_eq!(
        runtimes[0].drawable_image_region(NS, PARENT, rect).unwrap(),
        vec![0x11; 64]
    );
    // A later ordinary draw must behave exactly as on an authority that has
    // never received the future Present. This also detects an early generation
    // change, not just an early raster copy.
    let responses = runtimes.each_mut().map(|runtime| {
        runtime.begin_dispatch();
        let response = runtime.apply_put_image(
            TransactionId::from_raw(701),
            NS,
            PARENT,
            Region::single(rect),
            Some(&[0x33; 64]),
            None,
        );
        (response, runtime.take_cpu_buffer_updates())
    });
    assert_eq!(responses[0], responses[1]);
    runtimes[0].begin_dispatch();
    let held = runtimes[0]
        .execute_prepared_standard_pixmap(
            TransactionId::from_raw(700),
            TransactionId::from_raw(10700),
        )
        .unwrap()
        .map(|execution| execution.response)
        .unwrap();
    runtimes[1].begin_dispatch();
    let direct = runtimes[1].present_standard_pixmap(
        TransactionId::from_raw(10700),
        NS,
        PARENT,
        PIXMAP,
        0,
        0,
        None,
        None,
    );
    assert_eq!(held, direct);
    assert_eq!(
        runtimes[0].take_cpu_buffer_updates(),
        runtimes[1].take_cpu_buffer_updates()
    );
    assert_eq!(runtimes[0].prepared_present_count(), 0);
}

#[test]
fn a_prepared_cpu_present_keeps_the_backing_through_free_and_xid_reuse() {
    let mut runtime = fixture(4, 4, 0);
    let rect = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };
    runtime
        .create_pixmap(
            NS,
            PIXMAP,
            Size {
                width: 4,
                height: 4,
            },
            24,
            1,
        )
        .unwrap();
    upload(&mut runtime, PIXMAP, rect, &[0x55; 64]);
    prepare(&mut runtime, 1, 710, PARENT).unwrap();
    prepare(&mut runtime, 1, 711, CHILD).unwrap();
    // Source pixels are sampled at execution. Preparing the request must not
    // read SHM or CPU bytes ahead of the acquire fence.
    upload(&mut runtime, PIXMAP, rect, &[0x66; 64]);
    assert_eq!(runtime.free_pixmap(NS, PIXMAP).unwrap(), None);
    assert_eq!(runtime.retained_pixmap_count(), 1);
    runtime
        .create_pixmap(
            NS,
            PIXMAP,
            Size {
                width: 4,
                height: 4,
            },
            24,
            1,
        )
        .unwrap();
    upload(&mut runtime, PIXMAP, rect, &[0x77; 64]);
    runtime.begin_dispatch();
    let response = runtime
        .execute_prepared_standard_pixmap(
            TransactionId::from_raw(710),
            TransactionId::from_raw(10710),
        )
        .unwrap()
        .map(|execution| execution.response)
        .unwrap();
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    let XAuthorityCpuBufferUpdate::Replace(snapshot) = runtime.take_cpu_buffer_update().unwrap()
    else {
        panic!("first presentation replaces");
    };
    assert_eq!(snapshot.bytes.as_slice(), &[0x66; 64]);
    assert_eq!(runtime.retained_pixmap_count(), 1);
    assert!(runtime.cancel_prepared_standard_pixmap(TransactionId::from_raw(711)));
    assert_eq!(runtime.retained_pixmap_count(), 0);
    assert_eq!(runtime.prepared_present_count(), 0);
    assert_eq!(
        runtime.drawable_image_region(NS, PIXMAP, rect).unwrap(),
        vec![0x77; 64]
    );
}

#[test]
fn destroying_a_window_cancels_preparation_and_releases_the_last_dmabuf_reference() {
    let mut runtime = fixture(4, 4, 0);
    let descriptor = runtime
        .create_dri3_pixmap(NS, PIXMAP, 1, 64, 4, 4, 16, 24, 32)
        .unwrap();
    prepare(&mut runtime, 1, 720, CHILD).unwrap();
    assert_eq!(runtime.free_pixmap(NS, PIXMAP).unwrap(), None);
    assert!(runtime.take_retired_pixmap_registrations(NS).is_empty());
    runtime.destroy_window_subtree(NS, PARENT).unwrap();
    assert_eq!(runtime.prepared_present_count(), 0);
    assert_eq!(runtime.retained_pixmap_count(), 0);
    assert_eq!(
        runtime.take_retired_pixmap_registrations(NS),
        vec![descriptor.handle]
    );
    assert!(
        runtime
            .execute_prepared_standard_pixmap(
                TransactionId::from_raw(720),
                TransactionId::from_raw(10720)
            )
            .unwrap()
            .map(|execution| execution.response)
            .is_none()
    );
    assert!(!runtime.cancel_prepared_standard_pixmap(TransactionId::from_raw(720)));
    assert!(runtime.take_retired_pixmap_registrations(NS).is_empty());
}

#[test]
fn freed_dmabuf_executes_from_its_original_descriptor_and_releases_once() {
    let mut runtime = fixture(4, 4, 0);
    let descriptor = runtime
        .create_dri3_pixmap(NS, PIXMAP, 1, 64, 4, 4, 16, 24, 32)
        .unwrap();
    prepare(&mut runtime, 1, 730, CHILD).unwrap();
    runtime.free_pixmap(NS, PIXMAP).unwrap();
    runtime
        .create_pixmap(
            NS,
            PIXMAP,
            Size {
                width: 4,
                height: 4,
            },
            24,
            1,
        )
        .unwrap();
    runtime.begin_dispatch();
    let response = runtime
        .execute_prepared_standard_pixmap(
            TransactionId::from_raw(730),
            TransactionId::from_raw(10730),
        )
        .unwrap()
        .map(|execution| execution.response)
        .unwrap();
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(
        response.transactions[0].target_buffer(),
        BufferSource::DmaBuf {
            handle: descriptor.handle.raw()
        }
    );
    assert!(runtime.take_cpu_buffer_updates().is_empty());
    assert_eq!(
        runtime.take_retired_pixmap_registrations(NS),
        vec![descriptor.handle]
    );
    assert_eq!(runtime.retained_pixmap_count(), 0);
}

#[test]
fn prepared_requests_are_bounded_and_client_cancellation_does_not_touch_other_clients() {
    let mut runtime = fixture(4, 4, 0);
    runtime
        .create_dri3_pixmap(NS, PIXMAP, 1, 64, 4, 4, 16, 24, 32)
        .unwrap();
    for client in 1..=4 {
        for sequence in 0..64 {
            prepare(&mut runtime, client, client * 100 + sequence, CHILD).unwrap();
        }
        assert_eq!(
            prepare(&mut runtime, client, client * 100 + 64, CHILD),
            Err(XPresentPreparationError::Capacity)
        );
    }
    assert_eq!(runtime.prepared_present_count(), 256);
    assert_eq!(
        prepare(&mut runtime, 5, 500, CHILD),
        Err(XPresentPreparationError::Capacity)
    );
    runtime.free_pixmap(NS, PIXMAP).unwrap();
    runtime.cancel_client_prepared_presents(1);
    assert_eq!(runtime.prepared_present_count(), 192);
    assert!(
        runtime
            .execute_prepared_standard_pixmap(
                TransactionId::from_raw(100),
                TransactionId::from_raw(10100)
            )
            .unwrap()
            .map(|execution| execution.response)
            .is_none()
    );
    assert!(
        runtime
            .execute_prepared_standard_pixmap(
                TransactionId::from_raw(200),
                TransactionId::from_raw(10200)
            )
            .unwrap()
            .map(|execution| execution.response)
            .is_some()
    );
    for client in 2..=4 {
        runtime.cancel_client_prepared_presents(client);
    }
    assert_eq!(runtime.prepared_present_count(), 0);
    assert_eq!(runtime.retained_pixmap_count(), 0);
}

#[test]
fn prepared_fences_are_private_and_destruction_does_not_bind_a_reused_xid() {
    let mut runtime = fixture(4, 4, 0);
    runtime
        .create_dri3_pixmap(NS, PIXMAP, 1, 64, 4, 4, 16, 24, 32)
        .unwrap();
    let wait = XResourceId::new(0x600001, 1);
    let idle = XResourceId::new(0x600002, 1);
    let original_wait = runtime.create_dri3_fence(NS, wait, 1).unwrap();
    let original_idle = runtime.create_dri3_fence(NS, idle, 1).unwrap();
    let transaction = TransactionId::from_raw(700);
    runtime
        .prepare_standard_pixmap(
            1,
            transaction,
            NS,
            CHILD,
            PIXMAP,
            (0, 0),
            None,
            None,
            XPresentFenceResources {
                wait: Some(wait),
                idle: Some(idle),
            },
        )
        .unwrap();
    assert_eq!(
        runtime.prepared_present_fences(transaction),
        Some(XPreparedPresentFences {
            wait: Some(original_wait),
            idle: Some(original_idle),
        })
    );
    assert_eq!(runtime.destroy_dri3_fence(NS, wait).unwrap(), original_wait);
    let replacement_wait = runtime.create_dri3_fence(NS, wait, 1).unwrap();
    assert_ne!(replacement_wait, original_wait);
    assert_eq!(
        runtime.prepared_present_fences(transaction),
        Some(XPreparedPresentFences {
            wait: None,
            idle: Some(original_idle),
        })
    );
    // A different client's range can own the fences. Its cleanup must detach
    // only those fences from a surviving present, just like DestroyFence.
    let release = runtime
        .release_client_resource_range(
            NS,
            XWireClientResourceRange {
                base: 0x600000,
                mask: 0xff,
            },
        )
        .unwrap();
    assert!(release.released_fences.contains(&replacement_wait));
    assert!(release.released_fences.contains(&original_idle));
    let replacement_idle = runtime.create_dri3_fence(NS, idle, 1).unwrap();
    assert_ne!(replacement_idle, original_idle);
    assert_eq!(
        runtime.prepared_present_fences(transaction),
        Some(XPreparedPresentFences::default())
    );
    assert_eq!(
        runtime
            .execute_prepared_standard_pixmap(transaction, TransactionId::from_raw(10700))
            .unwrap()
            .map(|execution| execution.response)
            .unwrap()
            .outcome,
        XAuthorityResponseOutcome::Accepted
    );
    assert_eq!(runtime.prepared_present_fences(transaction), None);
}

#[test]
fn save_set_reparent_preserves_the_surviving_clients_prepared_present() {
    let mut runtime = fixture(4, 4, 0);
    runtime
        .create_dri3_pixmap(NS, PIXMAP, 1, 64, 4, 4, 16, 24, 32)
        .unwrap();
    prepare(&mut runtime, 1, 701, PARENT).unwrap();
    prepare(&mut runtime, 2, 702, CHILD).unwrap();
    // This range owns the frame, but not the peer's child or pixmap.
    let manager = XWireClientResourceRange {
        base: 0x400000,
        mask: 7,
    };
    runtime.cancel_client_prepared_presents(1);
    let release = runtime
        .release_client_resource_range_with_save_set(NS, manager, &[CHILD])
        .unwrap();
    assert_eq!(release.destroyed_windows, vec![PARENT]);
    assert_eq!(release.save_set_reparents.len(), 1);
    assert_eq!(release.save_set_reparents[0].window, CHILD);
    assert_eq!(
        release.save_set_reparents[0].new_parent.local.raw(),
        u64::from(X_SETUP_DEFAULT_ROOT)
    );
    assert_eq!(runtime.prepared_present_count(), 1);
    assert!(
        runtime
            .execute_prepared_standard_pixmap(
                TransactionId::from_raw(701),
                TransactionId::from_raw(10701)
            )
            .unwrap()
            .map(|execution| execution.response)
            .is_none()
    );
    let response = runtime
        .execute_prepared_standard_pixmap(
            TransactionId::from_raw(702),
            TransactionId::from_raw(10702),
        )
        .unwrap()
        .map(|execution| execution.response)
        .unwrap();
    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions[0].surface, SurfaceId::new(47, 1));
    assert_eq!(runtime.prepared_present_count(), 0);
}

#[test]
fn present_validates_resources_before_scalar_options_in_reference_order() {
    let missing_window = XResourceId::new(0x700001, 1);
    let missing_pixmap = XResourceId::new(0x700002, 1);
    let missing_region = 0x700003;
    let missing_fence = XResourceId::new(0x700004, 1);
    for byte_order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut runtime = fixture(4, 4, 0);
        runtime
            .create_pixmap(
                NS,
                PIXMAP,
                Size {
                    width: 4,
                    height: 4,
                },
                24,
                1,
            )
            .unwrap();
        let context = XDispatchContext {
            byte_order,
            namespace: NS,
            transaction: TransactionId::from_raw(800),
            sequence: 14,
            major_opcode: X_PRESENT_MAJOR_OPCODE,
            client_id: 1,
            injection: XTestAdmission::Absent,
            server_time: 1,
        };
        let mut atoms = XAtomTable::new();
        let mut properties = XPropertyTable::new();
        for (window, pixmap, region, crtc, fence, options, code, resource) in [
            (
                missing_window,
                missing_pixmap,
                missing_region,
                9,
                Some(missing_fence),
                16,
                XErrorCode::BadWindow,
                missing_window.local.raw() as u32,
            ),
            (
                PARENT,
                missing_pixmap,
                missing_region,
                9,
                Some(missing_fence),
                16,
                XErrorCode::BadPixmap,
                missing_pixmap.local.raw() as u32,
            ),
            (
                PARENT,
                PIXMAP,
                missing_region,
                9,
                Some(missing_fence),
                16,
                XErrorCode::BadValue,
                missing_region,
            ),
            (
                PARENT,
                PIXMAP,
                0,
                9,
                Some(missing_fence),
                16,
                XErrorCode::BadValue,
                9,
            ),
            (
                PARENT,
                PIXMAP,
                0,
                0,
                Some(missing_fence),
                16,
                XErrorCode::BadValue,
                missing_fence.local.raw() as u32,
            ),
            (PARENT, PIXMAP, 0, 0, None, 16, XErrorCode::BadValue, 16),
            (PARENT, PIXMAP, 0, 0, None, 4, XErrorCode::BadValue, 4),
            (PARENT, PIXMAP, 0, 0, None, 0, XErrorCode::BadValue, 2),
        ] {
            runtime.begin_dispatch();
            let result = dispatch_x11_wire_request(
                context,
                XWireRequest::Present(XPresentRequest::PresentPixmap {
                    transaction: context.transaction,
                    window,
                    pixmap,
                    serial: 1,
                    valid_region: region,
                    update_region: 0,
                    x_offset: 0,
                    y_offset: 0,
                    target_crtc: crtc,
                    wait_fence: fence,
                    idle_fence: None,
                    options,
                    target_msc: 100,
                    divisor: 0,
                    remainder: 2,
                    notifies: Vec::new(),
                }),
                &mut runtime,
                &mut atoms,
                &mut properties,
            );
            assert!(
                matches!(result.outputs.as_slice(), [XClientOutput::Error(error)]
                if error.code == code && error.resource_id == resource && error.sequence == 14),
                "{byte_order:?}: expected {code:?}/{resource}, got {:?}",
                result.outputs
            );
            assert!(result.response.is_none());
            assert!(runtime.take_cpu_buffer_updates().is_empty());
        }
    }
}

#[test]
fn execution_uses_a_fresh_ticket_and_carries_exact_private_fences() {
    let mut runtime = fixture(4, 4, 0);
    runtime
        .create_dri3_pixmap(NS, PIXMAP, 1, 64, 4, 4, 16, 24, 32)
        .unwrap();
    let fence = XResourceId::new(0x600001, 1);
    let handle = runtime.create_dri3_fence(NS, fence, 1).unwrap();
    let preparation = TransactionId::from_raw(900);
    runtime
        .prepare_standard_pixmap(
            2,
            preparation,
            NS,
            CHILD,
            PIXMAP,
            (0, 0),
            None,
            None,
            XPresentFenceResources {
                wait: Some(fence),
                idle: Some(fence),
            },
        )
        .unwrap();
    prepare(&mut runtime, 1, 901, PARENT).unwrap();
    for invalid in [0, 899, 900, 901] {
        assert!(matches!(
            runtime.execute_prepared_standard_pixmap(preparation, TransactionId::from_raw(invalid)),
            Err(XPresentExecutionError::InvalidTransaction)
        ));
        assert_eq!(runtime.prepared_present_count(), 2);
    }
    let execution = runtime
        .execute_prepared_standard_pixmap(preparation, TransactionId::from_raw(902))
        .unwrap()
        .unwrap();
    assert_eq!(execution.preparation, preparation);
    assert_eq!(execution.client, 2);
    assert_eq!(execution.namespace, NS);
    assert_eq!(execution.window, CHILD);
    assert_eq!(
        execution.fences,
        XPreparedPresentFences {
            wait: Some(handle),
            idle: Some(handle)
        }
    );
    assert_eq!(execution.response.transaction, TransactionId::from_raw(902));
    assert_eq!(
        execution.response.outcome,
        XAuthorityResponseOutcome::Accepted
    );
    assert_eq!(runtime.prepared_present_count(), 1);
    // Destruction after execution belongs to the backend fence-release path;
    // it cannot rewrite the handles already returned for this exact execution.
    assert_eq!(runtime.destroy_dri3_fence(NS, fence).unwrap(), handle);
    assert_eq!(execution.fences.wait, Some(handle));
    assert_ne!(runtime.create_dri3_fence(NS, fence, 1).unwrap(), handle);
}

#[test]
fn a_rejected_execution_still_names_the_fresh_ticket_for_empty_ordered_publication() {
    let mut runtime = fixture(4, 4, 0);
    // Existing CPU execution refuses a pixmap with no raster backing. This
    // exercises failure after preparation, without pretending it was drawn.
    runtime
        .create_pixmap(
            NS,
            PIXMAP,
            Size {
                width: 4,
                height: 4,
            },
            24,
            1,
        )
        .unwrap();
    prepare(&mut runtime, 1, 910, CHILD).unwrap();
    let execution = runtime
        .execute_prepared_standard_pixmap(
            TransactionId::from_raw(910),
            TransactionId::from_raw(920),
        )
        .unwrap()
        .unwrap();
    assert_eq!(execution.response.transaction, TransactionId::from_raw(920));
    assert!(matches!(
        execution.response.outcome,
        XAuthorityResponseOutcome::Rejected(_)
    ));
    assert!(execution.response.transactions.is_empty());
    assert!(runtime.take_cpu_buffer_updates().is_empty());
    assert_eq!(runtime.prepared_present_count(), 0);
}
