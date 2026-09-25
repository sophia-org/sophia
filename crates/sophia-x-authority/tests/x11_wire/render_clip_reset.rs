#[test]
fn render_picture_clip_mask_none_restores_drawing_outside_a_temporary_clip() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    let whole = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };
    let clip = render_set_picture_clip_rectangles_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        1,
        1,
        &[Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&clip)), None);
    let fill = render_fill_rectangles_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        [0xffff, 0xffff, 0xffff, 0xffff],
        &[whole],
    );
    // A different attribute must leave the temporary clip in force.
    let repeat =
        render_change_picture_request(RenderFixture::ORDER, RenderFixture::PICTURE, &[(0, 1)]);
    assert_eq!(RenderFixture::error_of(&fixture.send(&repeat)), None);
    assert_eq!(RenderFixture::error_of(&fixture.send(&fill)), None);
    assert_eq!(fixture.pixel(1, 1), [255; 4]);
    assert_eq!(fixture.pixel(3, 3), [0; 4]);

    let unsupported = render_change_picture_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        &[(6, RenderFixture::PIXMAP)],
    );
    assert_eq!(
        RenderFixture::error_of(&fixture.send(&unsupported)),
        Some(XErrorCode::BadImplementation)
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&fill)), None);
    assert_eq!(fixture.pixel(3, 3), [0; 4], "refusal preserves the clip");

    for invalid in [vec![(0, 255), (6, 0)], vec![(6, 0), (12, 2)]] {
        let change =
            render_change_picture_request(RenderFixture::ORDER, RenderFixture::PICTURE, &invalid);
        assert_eq!(
            RenderFixture::error_of(&fixture.send(&change)),
            Some(XErrorCode::BadValue)
        );
        assert_eq!(RenderFixture::error_of(&fixture.send(&fill)), None);
        assert_eq!(
            fixture.pixel(3, 3),
            [0; 4],
            "invalid attribute must not clear a clip"
        );
    }

    // GTK/Cairo removes temporary clips with CPClipMask=None before painting
    // the next widget. Accepting that write without clearing the rectangles
    // silently cuts later text and backgrounds down to the old clip.
    let reset =
        render_change_picture_request(RenderFixture::ORDER, RenderFixture::PICTURE, &[(6, 0)]);
    assert_eq!(RenderFixture::error_of(&fixture.send(&reset)), None);
    assert_eq!(RenderFixture::error_of(&fixture.send(&fill)), None);
    for y in 0..4 {
        for x in 0..4 {
            assert_eq!(fixture.pixel(x, y), [255; 4], "unclipped pixel ({x}, {y})");
        }
    }
}

#[test]
fn render_picture_clip_reset_decodes_both_byte_orders_without_confusing_omission() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        for (attributes, clears) in [
            (vec![(6, 0)], true),
            (vec![(4, 3), (5, 7), (6, 0)], true),
            (vec![(0, 1)], false),
            (vec![(1, 0)], false),
        ] {
            let bytes = render_change_picture_request(order, RenderFixture::PICTURE, &attributes);
            let request = decode_x11_core_request(context(RenderFixture::NS, 1, order), &bytes)
                .expect("valid picture attributes");
            match request {
                XWireRequest::Render(sophia_x_authority::XRenderRequest::RenderChangePicture { values, .. }) => {
                    assert_eq!(values.clear_clip_mask, clears);
                    assert!(!values.refused_attribute);
                }
                other => panic!("unexpected request: {other:?}"),
            }
        }
    }
}

fn clip_send(fixture: &mut RenderFixture, order: XByteOrder, bytes: &[u8]) -> XDispatchResult {
    fixture.sequence += 1;
    let request = decode_x11_core_request(context(RenderFixture::NS, 950, order), bytes).unwrap();
    dispatch_x11_wire_request(
        dispatch_context(RenderFixture::NS, fixture.sequence, order, bytes[0]),
        request,
        &mut fixture.runtime,
        &mut fixture.atoms,
        &mut fixture.properties,
    )
}

#[test]
fn core_clip_origins_empty_regions_and_reset_control_pixels_in_both_orders() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        let mut fixture = RenderFixture::new();
        let window = create_window_request(order, RenderFixture::PIXMAP, 0, 0, 4, 4);
        assert_eq!(
            RenderFixture::error_of(&clip_send(&mut fixture, order, &window)),
            None
        );
        let gc = 0x0020_0300;
        let create = create_gc_request(order, gc, RenderFixture::PIXMAP);
        assert_eq!(
            RenderFixture::error_of(&clip_send(&mut fixture, order, &create)),
            None
        );
        let mut clip = set_clip_rectangles_request(order, gc, &[(0, 0, 1, 2)]);
        let mut origins = Vec::new();
        push_i16(&mut origins, order, 2);
        push_i16(&mut origins, order, 1);
        clip[8..12].copy_from_slice(&origins);
        let set = change_gc_request(order, gc, 1 << 2, &[0x00ff_0000]);
        let fill = poly_fill_rectangle_request(order, RenderFixture::PIXMAP, gc, &[(0, 0, 4, 4)]);
        for bytes in [&clip, &set, &fill] {
            assert_eq!(
                RenderFixture::error_of(&clip_send(&mut fixture, order, bytes)),
                None
            );
        }
        assert_eq!(fixture.pixel(2, 1), [0, 0, 255, 0]);
        assert_eq!(fixture.pixel(2, 2), [0, 0, 255, 0]);
        assert_eq!(fixture.pixel(0, 0), [0; 4]);
        assert_eq!(fixture.pixel(3, 3), [0; 4]);
        let empty = set_clip_rectangles_request(order, gc, &[]);
        let green = change_gc_request(order, gc, 1 << 2, &[0x0000_ff00]);
        for bytes in [&empty, &green, &fill] {
            assert_eq!(
                RenderFixture::error_of(&clip_send(&mut fixture, order, bytes)),
                None
            );
        }
        assert_eq!(
            fixture.pixel(2, 1),
            [0, 0, 255, 0],
            "empty clips suppress drawing"
        );
        let extract = xfixes_create_region_from_request(
            order,
            X_XFIXES_CREATE_REGION_FROM_GC_MINOR_OPCODE,
            0x0020_0400,
            gc,
            0,
        );
        assert_eq!(
            RenderFixture::error_of(&clip_send(&mut fixture, order, &extract)),
            None
        );
        let foreign = NamespaceId::from_raw(89);
        let reset_foreign = change_gc_request(order, gc, 1 << 19, &[0]);
        let denied = dispatch_x11_wire_request(
            dispatch_context(foreign, 40, order, 56),
            decode_x11_core_request(context(foreign, 40, order), &reset_foreign).unwrap(),
            &mut fixture.runtime,
            &mut fixture.atoms,
            &mut fixture.properties,
        );
        assert!(RenderFixture::error_of(&denied).is_some());
        assert_eq!(
            RenderFixture::error_of(&clip_send(&mut fixture, order, &fill)),
            None
        );
        assert_eq!(fixture.pixel(2, 1), [0, 0, 255, 0]);
        // A clip mask is served now, so naming one that is not a pixmap is
        // the client's error. It used to answer BadImplementation, which told
        // a client its valid request was beyond this server.
        let unknown_mask = change_gc_request(
            order,
            gc,
            (1 << 19) | (1 << 2),
            &[0xff, RenderFixture::PIXMAP],
        );
        assert_eq!(
            RenderFixture::error_of(&clip_send(&mut fixture, order, &unknown_mask)),
            Some(XErrorCode::BadPixmap)
        );
        let reset = change_gc_request(order, gc, 1 << 19, &[0]);
        for bytes in [&reset, &fill] {
            assert_eq!(
                RenderFixture::error_of(&clip_send(&mut fixture, order, bytes)),
                None
            );
        }
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(
                    fixture.pixel(x, y),
                    [0, 255, 0, 0],
                    "reset and refusal preserve the foreground"
                );
            }
        }
    }
}

#[test]
fn render_empty_clip_suppresses_drawing_and_remains_an_extractable_region() {
    let mut fixture = RenderFixture::with_argb_pixmap(4, 4);
    let clip = render_set_picture_clip_rectangles_request(
        RenderFixture::ORDER,
        RenderFixture::PICTURE,
        -2,
        3,
        &[],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&clip)), None);
    let fill = render_fill_rectangles_request(
        RenderFixture::ORDER,
        1,
        RenderFixture::PICTURE,
        [65535; 4],
        &[Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        }],
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&fill)), None);
    assert_eq!(fixture.pixel(1, 1), [0; 4]);
    let region = 0x0020_0400;
    let extract = xfixes_create_region_from_request(
        RenderFixture::ORDER,
        X_XFIXES_CREATE_REGION_FROM_PICTURE_MINOR_OPCODE,
        region,
        RenderFixture::PICTURE,
        0,
    );
    assert_eq!(RenderFixture::error_of(&fixture.send(&extract)), None);
    let reset =
        render_change_picture_request(RenderFixture::ORDER, RenderFixture::PICTURE, &[(6, 0)]);
    assert_eq!(RenderFixture::error_of(&fixture.send(&reset)), None);
    assert_eq!(RenderFixture::error_of(&fixture.send(&fill)), None);
    assert_eq!(fixture.pixel(1, 1), [255; 4]);
}

#[cfg(unix)]
#[test]
fn gtk_clip_copy_stream_is_independent_of_socket_write_boundaries() {
    use std::io::{Read, Write};
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let order = XByteOrder::LittleEndian;
    let window = 0x0020_0100;
    let pixmap = 0x0020_0101;
    let picture = 0x0020_0102;
    let gc = 0x0020_0103;
    let whole = Rect {
        x: 0,
        y: 0,
        width: 4,
        height: 4,
    };
    let requests = vec![
        create_window_request(order, window, 0, 0, 4, 4),
        resource_request(order, 8, window),
        create_pixmap_request(order, 24, pixmap, window, 4, 4),
        render_create_picture_request(order, picture, pixmap, X_RENDER_FORMAT_RGB24, &[]),
        create_gc_request(order, gc, window),
        change_gc_request(order, gc, 1 << 16, &[0]),
        render_set_picture_clip_rectangles_request(
            order,
            picture,
            1,
            1,
            &[Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            }],
        ),
        render_fill_rectangles_request(order, 1, picture, [65535, 0, 0, 65535], &[whole]),
        render_set_picture_clip_rectangles_request(order, picture, 0, 0, &[]),
        render_fill_rectangles_request(order, 1, picture, [0, 65535, 0, 65535], &[whole]),
        render_change_picture_request(order, picture, &[(6, 0)]),
        render_fill_rectangles_request(order, 1, picture, [0, 0, 65535, 65535], &[whole]),
        set_clip_rectangles_request(order, gc, &[]),
        copy_area_request(order, pixmap, window, gc, 0, 0, 0, 0, 4, 4),
        change_gc_request(order, gc, 1 << 19, &[0]),
        copy_area_request(order, pixmap, window, gc, 0, 0, 0, 0, 4, 4),
    ];
    let mut reference_updates = None;
    for mode in 0..3 {
        let path = std::env::temp_dir().join(format!(
            "sophia-gtk-clip-{}-{}.sock",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let server_path = path.clone();
        let (sender, receiver) =
            std::sync::mpsc::sync_channel(X_AUTHORITY_OBSERVED_TRANSACTION_CHANNEL_CAPACITY);
        let server = thread::spawn(move || {
            run_x11_core_socket_server_once_channel(&server_path, RenderFixture::NS, sender)
                .unwrap()
        });
        wait_for_socket(&path);
        let mut stream = connect_x_socket(&path);
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .write_all(&setup_request(order, 11, 0, b"", b""))
            .unwrap();
        read_setup_success(&mut stream, order);
        match mode {
            0 => stream.write_all(&requests.concat()).unwrap(),
            1 => {
                for chunk in requests.concat().chunks(3) {
                    stream.write_all(chunk).unwrap();
                    thread::yield_now();
                }
            }
            _ => {
                for request in &requests {
                    stream.write_all(request).unwrap();
                    // Test-only pacing; the production path has no readback or delay.
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }
        stream
            .write_all(&get_image_request(order, 2, window, 0, 0, 4, 4, u32::MAX))
            .unwrap();
        let header = read_x_record(&mut stream);
        assert_eq!(
            header[0], 1,
            "mode {mode}: unexpected event or error: {header:?}"
        );
        assert_eq!(read_u32(order, &header[4..8]), 16);
        let mut pixels = [0; 64];
        stream.read_exact(&mut pixels).unwrap();
        assert_eq!(pixels.as_slice(), [255, 0, 0, 0].repeat(16), "mode {mode}");
        drop(stream);
        server.join().unwrap();
        let _ = std::fs::remove_file(&path);
        let batches = receiver.try_iter().collect::<Vec<_>>();
        assert!(batches.iter().all(|batch| batch.protocol_errors.is_empty()));
        let updates = batches
            .into_iter()
            .flat_map(|batch| batch.cpu_buffer_updates)
            .collect::<Vec<_>>();
        assert!(!updates.is_empty());
        if let Some(reference) = &reference_updates {
            assert_eq!(
                &updates, reference,
                "published pixels differ for mode {mode}"
            );
        } else {
            reference_updates = Some(updates);
        }
    }
}
