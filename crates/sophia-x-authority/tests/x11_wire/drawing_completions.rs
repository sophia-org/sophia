// The drawing family's completions: the refusals the conformance gate's
// drawing cases require, and the exposure events a copy or a clear owes.
// Each test drives the decoder and the dispatcher the way the socket does,
// in both byte orders.

const DRAWING_COMPLETIONS_NAMESPACE: u64 = 61;
const BOTH_BYTE_ORDERS: [XByteOrder; 2] = [XByteOrder::LittleEndian, XByteOrder::BigEndian];

struct DrawingHost {
    namespace: NamespaceId,
    order: XByteOrder,
    runtime: XAuthorityRuntime,
    atoms: XAtomTable,
    properties: XPropertyTable,
    sequence: u16,
}

impl DrawingHost {
    fn new(order: XByteOrder) -> Self {
        Self {
            namespace: NamespaceId::from_raw(DRAWING_COMPLETIONS_NAMESPACE),
            order,
            runtime: XAuthorityRuntime::new(),
            atoms: XAtomTable::new(),
            properties: XPropertyTable::new(),
            sequence: 0,
        }
    }

    /// Decode and dispatch one request the way the socket does. The request
    /// must be one the decoder accepts; a refusal is read with `refusal`.
    fn send(&mut self, bytes: &[u8]) -> XDispatchResult {
        self.sequence += 1;
        let request = decode_x11_core_request(
            context(self.namespace, 3000 + u64::from(self.sequence), self.order),
            bytes,
        )
        .unwrap_or_else(|error| panic!("opcode {} decodes: {error:?}", bytes[0]));
        dispatch_x11_wire_request(
            dispatch_context(self.namespace, self.sequence, self.order, bytes[0]),
            request,
            &mut self.runtime,
            &mut self.atoms,
            &mut self.properties,
        )
    }

    /// Dispatch a request that must complete without any output.
    fn accept(&mut self, bytes: &[u8]) {
        let result = self.send(bytes);
        assert!(
            result.outputs.is_empty(),
            "opcode {} completes silently: {:?}",
            bytes[0],
            result.outputs
        );
    }

    /// Dispatch a request that must complete with exactly one error.
    fn error(&mut self, bytes: &[u8]) -> XClientError {
        let result = self.send(bytes);
        match result.outputs.as_slice() {
            [XClientOutput::Error(error)] => *error,
            other => panic!("opcode {} completes with one error: {other:?}", bytes[0]),
        }
    }

    /// The decoder's refusal of a request.
    fn refusal(&self, bytes: &[u8]) -> XWireParseError {
        decode_x11_core_request(context(self.namespace, 1, self.order), bytes)
            .err()
            .unwrap_or_else(|| panic!("opcode {} is refused by the decoder", bytes[0]))
    }

    fn pixmap(&mut self, pixmap: u32, depth: u8, width: u16, height: u16) {
        self.accept(&create_pixmap_request(
            self.order,
            depth,
            pixmap,
            X_SETUP_DEFAULT_ROOT,
            width,
            height,
        ));
    }

    fn gc(&mut self, gc: u32, drawable: u32) {
        self.accept(&create_gc_request(self.order, gc, drawable));
    }

    /// A window as a client creates one: the creation's own events are not
    /// the subject here.
    fn window(&mut self, bytes: &[u8]) {
        let created = self.send(bytes);
        assert!(
            !created
                .outputs
                .iter()
                .any(|output| matches!(output, XClientOutput::Error(_))),
            "{:?}",
            created.outputs
        );
    }

    /// A window created InputOnly, as a client asks for one: class 2, depth
    /// zero, visual CopyFromParent.
    fn input_only_window(&mut self, window: u32) {
        let mut bytes = create_window_request(self.order, window, 0, 0, 10, 10);
        bytes[1] = 0;
        put_wire_u16(&mut bytes[22..24], self.order, 2);
        self.window(&bytes);
    }
}

fn put_wire_u16(out: &mut [u8], order: XByteOrder, value: u16) {
    out.copy_from_slice(&match order {
        XByteOrder::LittleEndian => value.to_le_bytes(),
        XByteOrder::BigEndian => value.to_be_bytes(),
    });
}

fn query_best_size_request(
    order: XByteOrder,
    class: u8,
    drawable: u32,
    width: u16,
    height: u16,
) -> Vec<u8> {
    let mut out = vec![97, class];
    push_u16(&mut out, order, 3);
    push_u32(&mut out, order, drawable);
    push_u16(&mut out, order, width);
    push_u16(&mut out, order, height);
    out
}

fn create_gc_function_request(order: XByteOrder, gc: u32, drawable: u32, function: u32) -> Vec<u8> {
    let mut out = vec![55, 0];
    push_u16(&mut out, order, 5);
    push_u32(&mut out, order, gc);
    push_u32(&mut out, order, drawable);
    push_u32(&mut out, order, 1);
    push_u32(&mut out, order, function);
    out
}

fn resource(id: u32) -> XResourceId {
    XResourceId::new(u64::from(id), 1)
}

#[test]
fn drawing_family_values_outside_the_protocol_are_refused_by_the_decoder() {
    for order in BOTH_BYTE_ORDERS {
        let host = DrawingHost::new(order);
        let (pixmap, gc) = (0x22_0300, 0x22_0301);
        let decodes = |bytes: &[u8]| {
            decode_x11_core_request(context(host.namespace, 1, order), bytes).is_ok()
        };

        // SetClipRectangles: ordering is {UnSorted, YSorted, YXSorted, YXBanded}.
        let mut clip = set_clip_rectangles_request(order, gc, &[]);
        clip[1] = 3;
        assert!(decodes(&clip));
        clip[1] = 4;
        assert_eq!(host.refusal(&clip), XWireParseError::InvalidValue(4));

        // PolyPoint and PolyLine: coordinate-mode is {Origin, Previous}.
        assert_eq!(
            host.refusal(&poly_point_request(order, pixmap, gc, 2, &[(0, 0)])),
            XWireParseError::InvalidValue(2)
        );
        let mut line = poly_line_request(order, pixmap, gc, &[(0, 0), (1, 1)]);
        line[1] = 1;
        assert!(decodes(&line));
        line[1] = 2;
        assert_eq!(host.refusal(&line), XWireParseError::InvalidValue(2));

        // FillPoly: shape is {Complex, Nonconvex, Convex}, judged before the mode.
        let mut poly = fill_poly_request(order, pixmap, gc, &[(0, 0), (1, 0), (1, 1)]);
        poly[12] = 3;
        poly[13] = 2;
        assert_eq!(host.refusal(&poly), XWireParseError::InvalidValue(3));
        poly[12] = 2;
        assert_eq!(host.refusal(&poly), XWireParseError::InvalidValue(2));
        poly[13] = 1;
        assert!(decodes(&poly));

        // QueryBestSize: class is {Cursor, Tile, Stipple}.
        assert_eq!(
            host.refusal(&query_best_size_request(order, 3, X_SETUP_DEFAULT_ROOT, 16, 16)),
            XWireParseError::InvalidValue(3)
        );

        // ClearArea: exposures is a BOOL.
        let mut clear = clear_area_request(order, true, pixmap, 0, 0, 1, 1);
        clear[1] = 2;
        assert_eq!(host.refusal(&clear), XWireParseError::InvalidValue(2));

        // CreateWindow: class is {CopyFromParent, InputOutput, InputOnly}.
        let mut window = create_window_request(order, 0x22_0302, 0, 0, 8, 8);
        put_wire_u16(&mut window[22..24], order, 3);
        assert_eq!(host.refusal(&window), XWireParseError::InvalidValue(3));

        // The refused value travels in the error the client reads, where a
        // resource id would otherwise go.
        let refused = dispatch_x11_parse_error(
            dispatch_context(host.namespace, 9, order, 59),
            0,
            XWireParseError::InvalidValue(4),
        );
        assert!(matches!(
            refused.outputs.as_slice(),
            [XClientOutput::Error(XClientError {
                code: XErrorCode::BadValue,
                sequence: 9,
                resource_id: 4,
                minor_code: 0,
                major_code: 59,
            })]
        ));
    }
}

#[test]
fn drawing_family_gc_values_outside_their_ranges_are_value_errors() {
    for order in BOTH_BYTE_ORDERS {
        let mut host = DrawingHost::new(order);
        let (pixmap, bitmap, gc) = (0x22_0310, 0x22_0311, 0x22_0312);
        host.pixmap(pixmap, 24, 4, 2);
        host.pixmap(bitmap, 1, 4, 2);
        host.gc(gc, pixmap);

        // function 16 on CreateGC; line-style 3, a zero dash, an oversized
        // line width and a non-boolean graphics-exposures on ChangeGC.
        assert_eq!(
            host.refusal(&create_gc_function_request(order, 0x22_0313, pixmap, 16)),
            XWireParseError::InvalidValue(16)
        );
        for (mask, value) in [(1 << 5, 3), (1 << 21, 0), (1 << 4, 70_000), (1 << 16, 2)] {
            assert_eq!(
                host.refusal(&change_gc_request(order, gc, mask, &[value])),
                XWireParseError::InvalidValue(value)
            );
        }
        host.accept(&change_gc_request(order, gc, 1, &[15]));

        // A tile must be a pixmap of the context's depth and a stipple one of
        // depth one; an unknown pixmap is named in a Pixmap error.
        let unknown = 0x22_03ff;
        let refused = host.error(&change_gc_request(order, gc, 1 << 10, &[unknown]));
        assert_eq!(
            (refused.code, refused.resource_id),
            (XErrorCode::BadPixmap, unknown)
        );
        let refused = host.error(&change_gc_request(order, gc, 1 << 10, &[bitmap]));
        assert_eq!(refused.code, XErrorCode::BadMatch);
        let refused = host.error(&change_gc_request(order, gc, 1 << 11, &[pixmap]));
        assert_eq!(refused.code, XErrorCode::BadMatch);
        host.accept(&change_gc_request(order, gc, 1 << 10, &[pixmap]));
        host.accept(&change_gc_request(order, gc, 1 << 11, &[bitmap]));
    }
}

#[test]
fn drawing_family_copy_gc_across_depths_is_a_match_error() {
    for order in BOTH_BYTE_ORDERS {
        let mut host = DrawingHost::new(order);
        let (pixmap, bitmap, deep, deeper, shallow) =
            (0x22_0320, 0x22_0321, 0x22_0322, 0x22_0323, 0x22_0324);
        host.pixmap(pixmap, 24, 4, 2);
        host.pixmap(bitmap, 1, 4, 2);
        host.gc(deep, pixmap);
        host.gc(deeper, pixmap);
        host.gc(shallow, bitmap);

        host.accept(&copy_gc_request(order, deep, deeper, 1 << 2));
        host.accept(&copy_gc_request(order, deep, deeper, 0));
        let refused = host.error(&copy_gc_request(order, shallow, deep, 1 << 2));
        assert_eq!(
            (refused.code, refused.major_code),
            (XErrorCode::BadMatch, 57)
        );
        // An unknown context is reported before depths are compared.
        let refused = host.error(&copy_gc_request(order, 0x22_03fe, deep, 1 << 2));
        assert_eq!(
            (refused.code, refused.resource_id),
            (XErrorCode::BadGraphicsContext, 0x22_03fe)
        );
    }
}

#[test]
fn drawing_family_put_image_left_pad_on_z_pixmap_is_a_match_error() {
    for order in BOTH_BYTE_ORDERS {
        let mut host = DrawingHost::new(order);
        let (pixmap, gc) = (0x22_0330, 0x22_0331);
        host.pixmap(pixmap, 24, 1, 1);
        host.gc(gc, pixmap);
        let geometry = || PutImageGeometry {
            width: 1,
            height: 1,
            dst_x: 0,
            dst_y: 0,
        };

        let mut padded = put_image_request(order, pixmap, gc, geometry(), &[0; 4]);
        padded[20] = 4;
        let refused = host.error(&padded);
        assert_eq!(
            (refused.code, refused.major_code),
            (XErrorCode::BadMatch, 72)
        );

        // Format 3 is a bad value, and the value is named in the error.
        let mut bad_format = put_image_request(order, pixmap, gc, geometry(), &[0; 4]);
        bad_format[1] = 3;
        let refused = dispatch_x11_parse_error(
            dispatch_context(host.namespace, 5, order, 72),
            0,
            host.refusal(&bad_format),
        );
        assert!(matches!(
            refused.outputs.as_slice(),
            [XClientOutput::Error(XClientError {
                code: XErrorCode::BadValue,
                resource_id: 3,
                major_code: 72,
                ..
            })]
        ));
    }
}

#[test]
fn drawing_family_query_best_size_needs_a_drawable_with_pixels_for_tiles() {
    for order in BOTH_BYTE_ORDERS {
        let mut host = DrawingHost::new(order);
        let (window, input_only, pixmap) = (0x22_0340, 0x22_0341, 0x22_0342);
        host.window(&create_window_request(order, window, 0, 0, 8, 8));
        host.input_only_window(input_only);
        host.pixmap(pixmap, 24, 4, 4);

        for (class, drawable) in [(0, window), (1, window), (2, pixmap), (0, input_only)] {
            let reply = host.send(&query_best_size_request(order, class, drawable, 16, 16));
            assert!(
                matches!(
                    reply.outputs.as_slice(),
                    [XClientOutput::Reply(XClientReply::QueryBestSize {
                        width: 16,
                        height: 16,
                        ..
                    })]
                ),
                "class {class}: {:?}",
                reply.outputs
            );
        }
        // A tile or stipple size is meaningless for a window without pixels.
        for class in [1, 2] {
            let refused = host.error(&query_best_size_request(order, class, input_only, 16, 16));
            assert_eq!(
                (refused.code, refused.major_code),
                (XErrorCode::BadMatch, 97)
            );
        }
        let refused = host.error(&query_best_size_request(order, 1, 0x22_03fd, 16, 16));
        assert_eq!(
            (refused.code, refused.resource_id),
            (XErrorCode::BadDrawable, 0x22_03fd)
        );
    }
}

#[test]
fn drawing_family_input_only_windows_refuse_drawing_and_clearing() {
    for order in BOTH_BYTE_ORDERS {
        let mut host = DrawingHost::new(order);
        let (input_only, pixmap, gc) = (0x22_0350, 0x22_0351, 0x22_0352);
        host.input_only_window(input_only);
        host.pixmap(pixmap, 24, 4, 4);
        host.gc(gc, pixmap);

        let refused = host.error(&poly_point_request(order, input_only, gc, 0, &[(0, 0)]));
        assert_eq!(
            (refused.code, refused.major_code),
            (XErrorCode::BadMatch, 64)
        );
        let refused = host.error(&clear_area_request(order, false, input_only, 0, 0, 1, 1));
        assert_eq!(
            (refused.code, refused.major_code),
            (XErrorCode::BadMatch, 61)
        );
        // ClearArea on a pixmap names the pixmap in a Window error.
        let refused = host.error(&clear_area_request(order, false, pixmap, 0, 0, 1, 1));
        assert_eq!(
            (refused.code, refused.resource_id),
            (XErrorCode::BadWindow, pixmap)
        );
    }
}

#[test]
fn drawing_family_clear_area_reports_the_cleared_rectangle_of_a_viewable_window() {
    for order in BOTH_BYTE_ORDERS {
        let mut host = DrawingHost::new(order);
        let (window, unmapped) = (0x22_0360, 0x22_0361);
        host.window(&create_window_background_request(
            order, window, 10, 20, 8, 4, 0x0f0f0f,
        ));
        let mapped = host.send(&resource_request(order, 8, window));
        assert!(
            mapped
                .outputs
                .iter()
                .any(|output| matches!(output, XClientOutput::Event(XClientEvent::Expose { .. }))),
            "mapping a window exposes it: {:?}",
            mapped.outputs
        );

        // exposures False clears and reports nothing.
        host.accept(&clear_area_request(order, false, window, 2, 1, 3, 2));
        // Zero width and height reach the window's edges, and the event
        // covers the rectangle within the window.
        let cleared = host.send(&clear_area_request(order, true, window, 6, 3, 0, 0));
        assert!(
            matches!(
                cleared.outputs.as_slice(),
                [XClientOutput::Event(XClientEvent::Expose {
                    window: exposed,
                    x: 6,
                    y: 3,
                    width: 2,
                    height: 1,
                    count: 0,
                    ..
                })] if *exposed == resource(window)
            ),
            "{:?}",
            cleared.outputs
        );
        // An unmapped window retains nothing, so nothing is reported for it.
        host.window(&create_window_background_request(
            order, unmapped, 0, 0, 8, 4, 0,
        ));
        host.accept(&clear_area_request(order, true, unmapped, 0, 0, 0, 0));
    }
}

#[test]
fn drawing_family_copy_area_reports_the_source_it_could_not_copy() {
    for order in BOTH_BYTE_ORDERS {
        let mut host = DrawingHost::new(order);
        let (source, destination, gc) = (0x22_0370, 0x22_0371, 0x22_0372);
        host.pixmap(source, 24, 4, 2);
        host.pixmap(destination, 24, 6, 3);
        host.gc(gc, source);

        // A source wholly within its pixmap earns one NoExpose.
        let whole = host.send(&copy_area_request(
            order, source, destination, gc, 0, 0, 1, 1, 4, 2,
        ));
        assert!(
            matches!(
                whole.outputs.as_slice(),
                [XClientOutput::Event(XClientEvent::NoExpose {
                    drawable,
                    minor_opcode: 0,
                    major_opcode: 62,
                    ..
                })] if *drawable == resource(destination)
            ),
            "{:?}",
            whole.outputs
        );

        // Source columns 4 and 5 do not exist, so destination columns 2 and
        // 3 are reported exposed and left as they were.
        let partial = host.send(&copy_area_request(
            order, source, destination, gc, 2, 0, 0, 0, 4, 2,
        ));
        assert!(
            matches!(
                partial.outputs.as_slice(),
                [XClientOutput::Event(XClientEvent::GraphicsExpose {
                    drawable,
                    x: 2,
                    y: 0,
                    width: 2,
                    height: 2,
                    minor_opcode: 0,
                    count: 0,
                    major_opcode: 62,
                    ..
                })] if *drawable == resource(destination)
            ),
            "{:?}",
            partial.outputs
        );

        // A source overhanging every side exposes the bands around what was
        // copied, counted down to zero and cut to the destination.
        let bands = host.send(&copy_area_request(
            order, source, destination, gc, -1, -1, 0, 0, 6, 4,
        ));
        let exposed: Vec<(u16, u16, u16, u16, u16)> = bands
            .outputs
            .iter()
            .map(|output| match output {
                XClientOutput::Event(XClientEvent::GraphicsExpose {
                    x,
                    y,
                    width,
                    height,
                    count,
                    major_opcode: 62,
                    ..
                }) => (*x, *y, *width, *height, *count),
                other => panic!("only graphics exposures: {other:?}"),
            })
            .collect();
        assert_eq!(
            exposed,
            vec![(0, 0, 6, 1, 2), (0, 1, 1, 2, 1), (5, 1, 1, 2, 0)]
        );

        // The clip list confines the report as it confines the copy.
        host.accept(&set_clip_rectangles_request(order, gc, &[(0, 0, 3, 3)]));
        let clipped = host.send(&copy_area_request(
            order, source, destination, gc, 2, 0, 0, 0, 4, 2,
        ));
        assert!(
            matches!(
                clipped.outputs.as_slice(),
                [XClientOutput::Event(XClientEvent::GraphicsExpose {
                    x: 2,
                    y: 0,
                    width: 1,
                    height: 2,
                    count: 0,
                    ..
                })]
            ),
            "{:?}",
            clipped.outputs
        );
        host.accept(&change_gc_clip_mask_request(order, gc, 0));

        // graphics-exposures False silences both events.
        host.accept(&change_gc_request(order, gc, 1 << 16, &[0]));
        host.accept(&copy_area_request(
            order, source, destination, gc, 2, 0, 0, 0, 4, 2,
        ));
    }
}

#[test]
fn drawing_family_copy_plane_has_copy_area_exposures_and_a_bounded_plane() {
    for order in BOTH_BYTE_ORDERS {
        let mut host = DrawingHost::new(order);
        let (source, bitmap, destination, gc) = (0x22_0380, 0x22_0381, 0x22_0382, 0x22_0383);
        host.pixmap(source, 24, 4, 1);
        host.pixmap(bitmap, 1, 4, 1);
        host.pixmap(destination, 24, 4, 1);
        host.gc(gc, destination);

        let whole = host.send(&copy_plane_request(
            order,
            source,
            destination,
            gc,
            (0, 0),
            (0, 0),
            (4, 1),
            1 << 16,
        ));
        assert!(
            matches!(
                whole.outputs.as_slice(),
                [XClientOutput::Event(XClientEvent::NoExpose {
                    drawable,
                    major_opcode: 63,
                    ..
                })] if *drawable == resource(destination)
            ),
            "{:?}",
            whole.outputs
        );
        // The source need not share the destination's depth.
        let from_bitmap = host.send(&copy_plane_request(
            order,
            bitmap,
            destination,
            gc,
            (0, 0),
            (0, 0),
            (4, 1),
            1,
        ));
        assert!(
            matches!(
                from_bitmap.outputs.as_slice(),
                [XClientOutput::Event(XClientEvent::NoExpose {
                    major_opcode: 63,
                    ..
                })]
            ),
            "{:?}",
            from_bitmap.outputs
        );
        let partial = host.send(&copy_plane_request(
            order,
            source,
            destination,
            gc,
            (2, 0),
            (0, 0),
            (4, 1),
            1 << 16,
        ));
        assert!(
            matches!(
                partial.outputs.as_slice(),
                [XClientOutput::Event(XClientEvent::GraphicsExpose {
                    x: 2,
                    y: 0,
                    width: 2,
                    height: 1,
                    count: 0,
                    major_opcode: 63,
                    ..
                })]
            ),
            "{:?}",
            partial.outputs
        );

        // The plane must lie below the source depth; the refused plane is
        // named, whether the dispatcher or the decoder refuses it.
        for (plane, drawable) in [(1u32 << 24, source), (2, bitmap)] {
            let refused = host.error(&copy_plane_request(
                order,
                drawable,
                destination,
                gc,
                (0, 0),
                (0, 0),
                (1, 1),
                plane,
            ));
            assert_eq!(
                (refused.code, refused.resource_id, refused.major_code),
                (XErrorCode::BadValue, plane, 63)
            );
        }
        for plane in [0u32, 3] {
            assert_eq!(
                host.refusal(&copy_plane_request(
                    order,
                    source,
                    destination,
                    gc,
                    (0, 0),
                    (0, 0),
                    (1, 1),
                    plane,
                )),
                XWireParseError::InvalidValue(plane)
            );
        }
    }
}
