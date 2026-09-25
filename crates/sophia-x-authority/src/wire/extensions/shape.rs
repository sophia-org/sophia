fn decode_shape(
    context: XWireClientContext,
    bytes: &[u8],
) -> Result<XWireRequest, XWireParseError> {
    match bytes[1] {
        X_SHAPE_QUERY_VERSION_MINOR_OPCODE => {
            require_exact_len(
                X_SHAPE_MAJOR_OPCODE,
                X_SHAPE_QUERY_VERSION_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Shape(crate::XShapeRequest::ShapeQueryVersion))
        }
        X_SHAPE_RECTANGLES_MINOR_OPCODE => {
            require_len(X_SHAPE_MAJOR_OPCODE, X_SHAPE_RECTANGLES_REQ_LEN, bytes.len())?;
            if !(bytes.len() - X_SHAPE_RECTANGLES_REQ_LEN).is_multiple_of(8) {
                return Err(XWireParseError::InvalidLength {
                    opcode: X_SHAPE_MAJOR_OPCODE,
                    expected_at_least: X_SHAPE_RECTANGLES_REQ_LEN,
                    actual: bytes.len(),
                });
            }
            Ok(XWireRequest::Shape(crate::XShapeRequest::ShapeRectangles {
                op: bytes[4],
                kind: bytes[5],
                ordering: bytes[6],
                destination: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
                x_offset: context.byte_order.i16(&bytes[12..14]),
                y_offset: context.byte_order.i16(&bytes[14..16]),
                rectangles: decode_shape_rectangles(context.byte_order, &bytes[16..]),
            }))
        }
        X_SHAPE_MASK_MINOR_OPCODE => {
            require_exact_len(X_SHAPE_MAJOR_OPCODE, X_SHAPE_MASK_REQ_LEN, bytes.len())?;
            let source = context.byte_order.u32(&bytes[16..20]);
            Ok(XWireRequest::Shape(crate::XShapeRequest::ShapeMask {
                op: bytes[4],
                kind: bytes[5],
                destination: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
                x_offset: context.byte_order.i16(&bytes[12..14]),
                y_offset: context.byte_order.i16(&bytes[14..16]),
                // None means "no mask", which combined with Set is how a
                // client returns a kind to its default rather than emptying
                // it.
                source: (source != 0).then(|| XResourceId::new(u64::from(source), 1)),
            }))
        }
        X_SHAPE_COMBINE_MINOR_OPCODE => {
            require_exact_len(X_SHAPE_MAJOR_OPCODE, X_SHAPE_COMBINE_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::Shape(crate::XShapeRequest::ShapeCombine {
                op: bytes[4],
                kind: bytes[5],
                source_kind: bytes[6],
                destination: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
                x_offset: context.byte_order.i16(&bytes[12..14]),
                y_offset: context.byte_order.i16(&bytes[14..16]),
                source: XResourceId::new(u64::from(context.byte_order.u32(&bytes[16..20])), 1),
            }))
        }
        X_SHAPE_OFFSET_MINOR_OPCODE => {
            require_exact_len(X_SHAPE_MAJOR_OPCODE, X_SHAPE_OFFSET_REQ_LEN, bytes.len())?;
            Ok(XWireRequest::Shape(crate::XShapeRequest::ShapeOffset {
                kind: bytes[4],
                destination: XResourceId::new(u64::from(context.byte_order.u32(&bytes[8..12])), 1),
                x_offset: context.byte_order.i16(&bytes[12..14]),
                y_offset: context.byte_order.i16(&bytes[14..16]),
            }))
        }
        X_SHAPE_QUERY_EXTENTS_MINOR_OPCODE => {
            require_exact_len(
                X_SHAPE_MAJOR_OPCODE,
                X_SHAPE_QUERY_EXTENTS_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Shape(crate::XShapeRequest::ShapeQueryExtents {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        X_SHAPE_SELECT_INPUT_MINOR_OPCODE => {
            require_exact_len(
                X_SHAPE_MAJOR_OPCODE,
                X_SHAPE_SELECT_INPUT_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Shape(crate::XShapeRequest::ShapeSelectInput {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                enable: bytes[8] != 0,
            }))
        }
        X_SHAPE_INPUT_SELECTED_MINOR_OPCODE => {
            require_exact_len(
                X_SHAPE_MAJOR_OPCODE,
                X_SHAPE_INPUT_SELECTED_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Shape(crate::XShapeRequest::ShapeInputSelected {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
            }))
        }
        X_SHAPE_GET_RECTANGLES_MINOR_OPCODE => {
            require_exact_len(
                X_SHAPE_MAJOR_OPCODE,
                X_SHAPE_GET_RECTANGLES_REQ_LEN,
                bytes.len(),
            )?;
            Ok(XWireRequest::Shape(crate::XShapeRequest::ShapeGetRectangles {
                window: XResourceId::new(u64::from(context.byte_order.u32(&bytes[4..8])), 1),
                kind: bytes[8],
            }))
        }
        // No version of SHAPE defines a minor above eight, so anything else
        // is refused as a request this extension does not have.
        minor_opcode => Ok(XWireRequest::Shape(crate::XShapeRequest::ShapeUnimplemented { minor_opcode })),
    }
}

fn decode_shape_rectangles(byte_order: XByteOrder, bytes: &[u8]) -> Vec<Rect> {
    bytes
        .chunks_exact(8)
        .map(|rectangle| Rect {
            x: i32::from(byte_order.i16(&rectangle[0..2])),
            y: i32::from(byte_order.i16(&rectangle[2..4])),
            width: i32::from(byte_order.u16(&rectangle[4..6])),
            height: i32::from(byte_order.u16(&rectangle[6..8])),
        })
        .collect()
}
