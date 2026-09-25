use sophia_protocol::{NamespaceId, Rect, Size, TransactionId};
use sophia_x_authority::*;
use std::sync::Arc;

#[test]
fn shared_zpixmap_upload_decodes_packed_depths_padding_crop_and_byte_order() {
    for order in [XByteOrder::LittleEndian, XByteOrder::BigEndian] {
        for depth in [1u8, 4, 8, 16, 24, 32] {
            let namespace = NamespaceId::from_raw(1);
            let pixmap = XResourceId::new(0x200001, 1);
            let gc = XResourceId::new(0x200002, 1);
            let segment = XResourceId::new(0x200003, 1);
            let mut runtime = XAuthorityRuntime::new();
            runtime
                .create_pixmap(
                    namespace,
                    pixmap,
                    Size {
                        width: 2,
                        height: 2,
                    },
                    depth,
                    1,
                )
                .unwrap();
            runtime
                .create_graphics_context(namespace, gc, pixmap, XGraphicsContextValues::default())
                .unwrap();
            let (mapping, _descriptor) =
                sophia_sysv_shm::DescriptorMapping::create_sealed(4096).unwrap();
            let bpp = if depth == 24 { 32 } else { depth };
            let stride = (3 * usize::from(bpp)).div_ceil(32) * 4;
            let value: u32 = match depth {
                1 => 1,
                4 => 13,
                8 => 0xa5,
                16 => 0x1234,
                _ => 0x123456,
            };
            let mut data = vec![0; stride * 2];
            for (i, set) in [true, false, true, false, true, false]
                .into_iter()
                .enumerate()
            {
                let pixel = if set { value } else { 0 };
                let x = i % 3;
                let row = &mut data[(i / 3) * stride..];
                match bpp {
                    32 => row[x * 4..x * 4 + 4].copy_from_slice(&match order {
                        XByteOrder::LittleEndian => pixel.to_le_bytes(),
                        XByteOrder::BigEndian => pixel.to_be_bytes(),
                    }),
                    16 => row[x * 2..x * 2 + 2].copy_from_slice(&match order {
                        XByteOrder::LittleEndian => (pixel as u16).to_le_bytes(),
                        XByteOrder::BigEndian => (pixel as u16).to_be_bytes(),
                    }),
                    8 => row[x] = pixel as u8,
                    bits => {
                        let per_byte = 8 / usize::from(bits);
                        let bit = match order {
                            XByteOrder::LittleEndian => x % per_byte,
                            XByteOrder::BigEndian => per_byte - 1 - x % per_byte,
                        } * usize::from(bits);
                        row[x / per_byte] |= (pixel as u8) << bit;
                    }
                }
            }
            mapping.write_bytes(32, &data).unwrap();
            runtime
                .attach_shm_descriptor_segment(
                    namespace,
                    segment,
                    Arc::new(sophia_sysv_shm::ClientMapping::Descriptor(mapping)),
                    false,
                    1,
                )
                .unwrap();
            let request = |offset| {
                XWireRequest::Shm(sophia_x_authority::XShmRequest::ShmPutImage {
                    drawable: pixmap,
                    gc,
                    segment,
                    total_width: 3,
                    total_height: 2,
                    src_x: 1,
                    src_y: 0,
                    src_width: 2,
                    src_height: 2,
                    dst_x: 0,
                    dst_y: 0,
                    depth,
                    format: 2,
                    offset,
                    send_event: true,
                })
            };
            let context = XDispatchContext {
                byte_order: order,
                namespace,
                transaction: TransactionId::from_raw(1),
                sequence: 1,
                major_opcode: X_MIT_SHM_MAJOR_OPCODE,
                client_id: 1,
                injection: XTestAdmission::Absent,
                server_time: 4_242,
            };
            let mut atoms = XAtomTable::new();
            let mut properties = XPropertyTable::new();
            let result = dispatch_x11_wire_request(
                context,
                request(32),
                &mut runtime,
                &mut atoms,
                &mut properties,
            );
            assert_eq!(
                result.response.unwrap().outcome,
                XAuthorityResponseOutcome::Accepted,
                "depth={depth} order={order:?}"
            );
            assert!(matches!(
                result.outputs.as_slice(),
                [XClientOutput::Event(XClientEvent::ShmCompletion { .. })]
            ));
            let region = Rect {
                x: 0,
                y: 0,
                width: 2,
                height: 2,
            };
            let pixels = runtime
                .drawable_image_region(namespace, pixmap, region)
                .unwrap();
            assert_eq!(
                pixels
                    .chunks_exact(4)
                    .map(|p| u32::from_le_bytes(p.try_into().unwrap()))
                    .collect::<Vec<_>>(),
                vec![0, value, value, 0]
            );
            // Neither an out-of-bounds read nor another namespace can modify the pixmap.
            let rejected = dispatch_x11_wire_request(
                context,
                request(4095),
                &mut runtime,
                &mut atoms,
                &mut properties,
            );
            assert!(matches!(
                rejected.response.unwrap().outcome,
                XAuthorityResponseOutcome::Rejected(_)
            ));
            let foreign = XDispatchContext {
                namespace: NamespaceId::from_raw(2),
                ..context
            };
            let rejected = dispatch_x11_wire_request(
                foreign,
                request(32),
                &mut runtime,
                &mut atoms,
                &mut properties,
            );
            assert!(matches!(
                rejected.outputs.as_slice(),
                [XClientOutput::Error(XClientError {
                    code: XErrorCode::BadAccess,
                    ..
                })]
            ));
            assert_eq!(
                runtime
                    .drawable_image_region(namespace, pixmap, region)
                    .unwrap(),
                pixels
            );
        }
    }
}
