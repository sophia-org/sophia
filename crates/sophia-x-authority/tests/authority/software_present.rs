// Software presentation: pixmap materialisation for the renderer, one
// surface generation stream across present, SHM clears and core draws,
// over-capacity damage, and a departing client's windows. Included from
// authority.rs beside resources.rs (t026).

#[test]
fn software_present_materializes_pixmap_pixels_for_the_renderer() {
    let namespace = NamespaceId::from_raw(21);
    let window = XResourceId::new(0x66, 1);
    let pixmap = XResourceId::new(0x67, 1);
    let surface = SurfaceId::new(21, 1);
    let mut runtime = XAuthorityRuntime::new();
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(27),
        namespace,
        kind: XAuthorityRequestKind::CreateWindow {
            window,
            surface,
            geometry: Rect {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });
    runtime
        .create_pixmap(
            namespace,
            pixmap,
            Size {
                width: 4,
                height: 4,
            },
            24,
            1,
        )
        .unwrap();
    let image = vec![0x5a; 4 * 4 * 4];
    assert_eq!(
        runtime
            .apply_put_image(
                TransactionId::from_raw(28),
                namespace,
                pixmap,
                Region::single(Rect {
                    x: 0,
                    y: 0,
                    width: 4,
                    height: 4,
                }),
                Some(&image), None,)
            .outcome,
        XAuthorityResponseOutcome::Accepted
    );

    let response = runtime.present_standard_pixmap(
        TransactionId::from_raw(29),
        namespace,
        window,
        pixmap,
        2,
        1,
        None,
        None,
    );

    assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
    assert_eq!(response.transactions.len(), 1);
    let XAuthorityCpuBufferUpdate::Replace(snapshot) = runtime
        .take_cpu_buffer_update()
        .expect("software Present must export immutable pixels")
    else {
        panic!("first software Present must replace the presentation buffer");
    };
    assert_eq!(snapshot.drawable, window);
    assert!(snapshot.bytes.contains(&0x5a));
    assert_eq!(
        response.transactions[0].target_buffer(),
        BufferSource::CpuBuffer {
            handle: snapshot.handle
        }
    );
    assert_eq!(
        response.transactions[0].damage,
        Region::single(Rect {
            x: 2,
            y: 1,
            width: 4,
            height: 4,
        })
    );

    let changed = vec![0x6b; 4 * 4 * 4];
    runtime.apply_put_image(
        TransactionId::from_raw(30),
        namespace,
        pixmap,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 4,
        }),
        Some(&changed), None,);
    let update_region = Region {
        rects: vec![
            Rect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            Rect {
                x: 3,
                y: 3,
                width: 1,
                height: 1,
            },
        ],
    };
    let response = runtime.present_standard_pixmap(
        TransactionId::from_raw(31),
        namespace,
        window,
        pixmap,
        2,
        1,
        None,
        Some(update_region),
    );
    let XAuthorityCpuBufferUpdate::PatchBatch(batch) = runtime
        .take_cpu_buffer_update()
        .expect("later software Present must export damage patches")
    else {
        panic!("later software Present must retain the presentation handle");
    };
    assert_eq!(batch.handle, snapshot.handle);
    assert_eq!(batch.generation, 2);
    assert_eq!(
        batch
            .patches
            .iter()
            .map(|patch| patch.rect)
            .collect::<Vec<_>>(),
        vec![
            Rect {
                x: 2,
                y: 1,
                width: 1,
                height: 1,
            },
            Rect {
                x: 5,
                y: 4,
                width: 1,
                height: 1,
            },
        ]
    );
    assert_eq!(response.transactions[0].damage.rects, batch.patches.iter().map(|patch| {
        Rect {
            x: patch.rect.x,
            y: patch.rect.y,
            width: patch.rect.width,
            height: patch.rect.height,
        }
    }).collect::<Vec<_>>());
    let mut materialized = std::collections::BTreeMap::new();
    XAuthorityCpuBufferUpdate::Replace(snapshot.clone())
        .apply_to(&mut materialized)
        .unwrap();
    XAuthorityCpuBufferUpdate::PatchBatch(batch)
        .apply_to(&mut materialized)
        .unwrap();
    let materialized = materialized.get(&snapshot.handle).unwrap();
    assert_eq!(materialized.generation, 2);
    assert!(materialized.bytes.contains(&0x5a));
    assert!(materialized.bytes.contains(&0x6b));
}

#[test]
fn present_shm_clear_and_core_draw_share_one_surface_generation_stream() {
    let namespace = NamespaceId::from_raw(22);
    let window = XResourceId::new(0x68, 1);
    let pixmap = XResourceId::new(0x69, 1);
    let surface = SurfaceId::new(22, 1);
    let size = Size {
        width: 8,
        height: 8,
    };
    let full = Rect {
        x: 0,
        y: 0,
        width: size.width,
        height: size.height,
    };
    let mut runtime = XAuthorityRuntime::new();
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(90),
        namespace,
        kind: XAuthorityRequestKind::CreateWindow {
            window,
            surface,
            geometry: full,
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });
    runtime
        .create_pixmap(namespace, pixmap, size, 24, 1)
        .unwrap();
    runtime.apply_put_image(
        TransactionId::from_raw(91),
        namespace,
        pixmap,
        Region::single(full),
        Some(&vec![0x21; 8 * 8 * 4]), None,);

    let present = runtime.present_standard_pixmap(
        TransactionId::from_raw(92),
        namespace,
        window,
        pixmap,
        0,
        0,
        None,
        None,
    );
    let present_update = runtime.take_cpu_buffer_update().unwrap();
    let shm = runtime.apply_put_image(
        TransactionId::from_raw(93),
        namespace,
        window,
        Region::single(full),
        Some(&vec![0x42; 8 * 8 * 4]), None,);
    let shm_update = runtime.take_cpu_buffer_update().unwrap();
    let clear = runtime.apply_clear(
        TransactionId::from_raw(94),
        namespace,
        window,
        Region::single(Rect {
            x: 1,
            y: 1,
            width: 3,
            height: 3,
        }),
    );
    let clear_update = runtime.take_cpu_buffer_update().unwrap();
    let core = runtime.apply_core_draw(
        TransactionId::from_raw(95),
        namespace,
        window,
        Region::single(Rect {
            x: 4,
            y: 4,
            width: 2,
            height: 2,
        }),
    );
    let core_update = runtime.take_cpu_buffer_update().unwrap();

    let responses = [&present, &shm, &clear, &core];
    for (index, response) in responses.into_iter().enumerate() {
        assert_eq!(response.outcome, XAuthorityResponseOutcome::Accepted);
        assert_eq!(response.transactions.len(), 1);
        assert_eq!(response.transactions[0].surface, surface);
        assert_eq!(
            response.transactions[0].previous_committed_generation,
            index as u64 + 1
        );
    }
    let updates = [present_update, shm_update, clear_update, core_update];
    assert!(matches!(updates[0], XAuthorityCpuBufferUpdate::Replace(_)));
    assert!(updates[1..]
        .iter()
        .all(|update| matches!(update, XAuthorityCpuBufferUpdate::PatchBatch(_))));
    assert!(updates
        .iter()
        .all(|update| update.handle() == updates[0].handle()));
    assert_eq!(
        updates
            .iter()
            .map(XAuthorityCpuBufferUpdate::generation)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
}

#[test]
fn software_present_rejects_pixmap_without_materialized_pixels() {
    let namespace = NamespaceId::from_raw(22);
    let window = XResourceId::new(0x68, 1);
    let pixmap = XResourceId::new(0x69, 1);
    let mut runtime = XAuthorityRuntime::new();
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(30),
        namespace,
        kind: XAuthorityRequestKind::CreateWindow {
            window,
            surface: SurfaceId::new(22, 1),
            geometry: Rect {
                x: 0,
                y: 0,
                width: 8,
                height: 8,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });
    runtime
        .create_pixmap(
            namespace,
            pixmap,
            Size {
                width: 4,
                height: 4,
            },
            24,
            1,
        )
        .unwrap();

    let response = runtime.present_standard_pixmap(
        TransactionId::from_raw(31),
        namespace,
        window,
        pixmap,
        0,
        0,
        None,
        None,
    );

    assert_eq!(
        response.outcome,
        XAuthorityResponseOutcome::Rejected(XAuthorityRuntimeError::InvalidResource)
    );
    assert!(response.transactions.is_empty());
    assert!(runtime.take_cpu_buffer_update().is_none());
}

/// A damage list longer than the transport bound patches, and patches exactly.
///
/// A busy client -- a browser is the usual one -- reports far more than the
/// thirty-two rectangles the batch carries. That used to fall back to replacing
/// the whole presentation buffer, which is the largest copy on this path made
/// for the client whose buffers are biggest. The list is coalesced to the bound
/// instead.
///
/// The check is equivalence, not size: replaying the coalesced batch must
/// produce the same pixels a full replacement would. A merged cover is allowed
/// to be larger than the damage, because the patch is read from the buffer the
/// client's pixels were already composed into, and a larger rectangle carries
/// more already-correct bytes. It is not allowed to be smaller, which would
/// leave a region stale in a frame that is otherwise presentable.
///
/// `NC2` in `validation/specula/stable-x-backing-lease-modeling-brief.md`,
/// which violates `RegistryMatchesStore`.
#[test]
fn over_capacity_damage_coalesces_to_a_batch_equivalent_to_replacement() {
    let namespace = NamespaceId::from_raw(57);
    let window = XResourceId::new(0x91, 1);
    let mut runtime = XAuthorityRuntime::new();
    runtime.apply(XAuthorityRequestPacket {
        transaction: TransactionId::from_raw(140),
        namespace,
        kind: XAuthorityRequestKind::CreateWindow {
            window,
            surface: SurfaceId::new(57, 1),
            geometry: Rect {
                x: 0,
                y: 0,
                width: 200,
                height: 200,
            },
            constraints: SurfaceConstraints {
                min_size: None,
                max_size: None,
            },
            generation: 1,
        },
    });

    // Establish the buffer, and keep a materialized copy alongside so the
    // batch below can be replayed against the same base the session holds.
    runtime.apply_core_draw(
        TransactionId::from_raw(141),
        namespace,
        window,
        Region::single(Rect {
            x: 0,
            y: 0,
            width: 200,
            height: 200,
        }),
    );
    let mut replayed = std::collections::BTreeMap::new();
    let base = runtime.take_cpu_buffer_update().unwrap();
    assert!(base.is_replacement(), "the first update establishes a base");
    base.apply_to(&mut replayed).unwrap();

    // Sixty-four scattered rectangles: twice what the transport carries, and
    // spread so a merged cover stays well under half the buffer.
    let scattered = Region {
        rects: (0..64)
            .map(|index: i32| Rect {
                x: (index % 8) * 25,
                y: (index / 8) * 25,
                width: 3,
                height: 3,
            })
            .collect(),
    };
    runtime.apply_core_draw_with_gc(
        TransactionId::from_raw(142),
        namespace,
        window,
        scattered,
        &XGraphicsContextValues {
            foreground: 0x00ff_8800,
            ..XGraphicsContextValues::default()
        },
    );
    let update = runtime.take_cpu_buffer_update().unwrap();
    let XAuthorityCpuBufferUpdate::PatchBatch(batch) = &update else {
        panic!("a sixty-four-rectangle damage list must still patch: {update:?}");
    };
    assert!(
        batch.patches.len() <= X_AUTHORITY_CPU_PATCH_BATCH_MAX_RECTS,
        "a coalesced batch must fit the transport bound, got {}",
        batch.patches.len()
    );
    update.apply_to(&mut replayed).unwrap();

    // Every damaged pixel must have arrived. A cover that dropped a rectangle
    // leaves exactly this behind: a buffer that is the right size, the right
    // generation, and stale in one place.
    let replayed_buffer = replayed.get(&update.handle()).expect("replayed base");
    let stride = usize::try_from(replayed_buffer.stride).unwrap();
    let pixel_at = |x: usize, y: usize| -> u32 {
        let offset = y * stride + x * 4;
        u32::from_le_bytes(replayed_buffer.bytes[offset..offset + 4].try_into().unwrap())
    };
    for index in 0..64usize {
        let left = (index % 8) * 25;
        let top = (index / 8) * 25;
        for y in top..top + 3 {
            for x in left..left + 3 {
                assert_eq!(
                    pixel_at(x, y) & 0x00ff_ffff,
                    0x00ff_8800,
                    "rectangle {index} at ({x},{y}) was not carried by the coalesced batch"
                );
            }
        }
    }

    // And nothing else moved: the first draw left the buffer clear, so a pixel
    // between the damage rectangles still reads as it did.
    assert_eq!(
        pixel_at(12, 12) & 0x00ff_ffff,
        0,
        "a pixel outside the damage must not have been repainted"
    );
}

#[test]
fn departing_client_destroys_its_windows_deepest_first() {
    // The X11 DestroyNotify specification requires a window's inferiors to be
    // reported before the window itself, whatever caused the destruction. A
    // client going away destroys its whole set at once, and resource records
    // arrive in XID allocation order, which is ordinarily the exact reverse:
    // parents are allocated before the children they contain.
    let namespace = NamespaceId::from_raw(29);
    let parent = XResourceId::new(0x0020_0001, 1);
    let child = XResourceId::new(0x0020_0002, 1);
    let grandchild = XResourceId::new(0x0020_0003, 1);
    let mut runtime = XAuthorityRuntime::new();

    for (index, window) in [parent, child, grandchild].into_iter().enumerate() {
        let surface = 300 + index as u64;
        assert_eq!(
            runtime
                .apply(XAuthorityRequestPacket {
                    transaction: TransactionId::from_raw(surface),
                    namespace,
                    kind: XAuthorityRequestKind::CreateWindow {
                        window,
                        surface: SurfaceId::new(surface as u32, 1),
                        geometry: Rect {
                            x: 0,
                            y: 0,
                            width: 40,
                            height: 30,
                        },
                        constraints: SurfaceConstraints {
                            min_size: None,
                            max_size: None,
                        },
                        generation: 1,
                    },
                })
                .outcome,
            XAuthorityResponseOutcome::Accepted
        );
    }
    // Ascending XIDs nest downward, so allocation order and destruction order
    // disagree. Were they the same, this test would pass without the ordering.
    runtime
        .set_window_parent(namespace, child, parent)
        .unwrap();
    runtime
        .set_window_parent(namespace, grandchild, child)
        .unwrap();

    let release = runtime
        .release_client_resource_range(
            namespace,
            XWireClientResourceRange {
                base: 0x0020_0000,
                mask: X_SETUP_DEFAULT_RESOURCE_ID_MASK,
            },
        )
        .unwrap();

    assert_eq!(
        release.destroyed_windows,
        vec![grandchild, child, parent],
        "a departing client's windows must be destroyed deepest-first, so the \
         notifications driven from this order report inferiors before their \
         ancestors"
    );
}
