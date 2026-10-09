#[cfg(feature = "libinput-events")]
fn libinput_motion_event(serial: u64, x: f64, y: f64) -> InputEventPacket {
    InputEventPacket {
        serial,
        seat: SeatId::from_raw(1),
        device: DeviceId::from_raw(2),
        time_msec: serial * 10,
        kind: InputEventKind::PointerMotion,
        global_position: Some(Point { x, y }),
        target_surface: None,
        local_position: None,
    }
}

#[test]
fn native_libdrm_atomic_commit_request_reports_reduced_flags() {
    let default_request =
        LibdrmNativeAtomicCommitRequest::new(drm::control::atomic::AtomicModeReq::new());
    assert_eq!(
        default_request.reduced_scope(),
        LibdrmNativeAtomicCommitRequestScope::PageFlip
    );
    assert_eq!(
        default_request.reduced_flags(),
        LibdrmNativeAtomicCommitFlagsReport {
            page_flip_event: true,
            nonblocking: true,
            allow_modeset: false,
            test_only: false,
        }
    );
    assert_eq!(
        LibdrmNativeAtomicCommitRequest::modeset(drm::control::atomic::AtomicModeReq::new())
            .reduced_scope(),
        LibdrmNativeAtomicCommitRequestScope::Modeset
    );

    let explicit_request =
        LibdrmNativeAtomicCommitRequest::new(drm::control::atomic::AtomicModeReq::new())
            .without_page_flip_event()
            .blocking()
            .allow_modeset()
            .test_only();
    assert_eq!(
        explicit_request.reduced_flags(),
        LibdrmNativeAtomicCommitFlagsReport {
            page_flip_event: false,
            nonblocking: false,
            allow_modeset: true,
            test_only: true,
        }
    );
}

#[test]
fn native_libdrm_primary_plane_property_discovery_feeds_request_builder() {
    let discovery = discover_native_primary_plane_property_handles(
        &full_property_lookup_device(),
        connector_handle(),
        crtc_handle(),
        plane_handle(),
    );

    assert_eq!(
        discovery.status,
        LibdrmNativePrimaryPlanePropertyDiscoveryStatus::Discovered
    );
    let properties = discovery
        .properties
        .expect("complete lookup should produce private property handles");
    assert_eq!(properties.plane_in_formats(), Some(property_handle(114)));
    let build = build_native_primary_plane_atomic_request(
        primary_plane_objects(Size {
            width: 1280,
            height: 720,
        }),
        properties,
    );

    assert_eq!(build.status, LibdrmNativeAtomicRequestBuildStatus::Built);
    assert!(build.request.is_some());
}

#[test]
fn native_libdrm_primary_plane_property_discovery_keeps_in_formats_optional() {
    let without_in_formats = FakeNativePropertyLookupDevice {
        plane: Ok(LibdrmNativePropertyHandleSet::new([
            ("FB_ID", property_handle(104)),
            ("CRTC_ID", property_handle(105)),
            ("SRC_X", property_handle(106)),
            ("SRC_Y", property_handle(107)),
            ("SRC_W", property_handle(108)),
            ("SRC_H", property_handle(109)),
            ("CRTC_X", property_handle(110)),
            ("CRTC_Y", property_handle(111)),
            ("CRTC_W", property_handle(112)),
            ("CRTC_H", property_handle(113)),
        ])),
        ..full_property_lookup_device()
    };

    let discovery = discover_native_primary_plane_property_handles(
        &without_in_formats,
        connector_handle(),
        crtc_handle(),
        plane_handle(),
    );

    assert_eq!(
        discovery.status,
        LibdrmNativePrimaryPlanePropertyDiscoveryStatus::Discovered
    );
    assert_eq!(
        discovery
            .properties
            .expect("complete request properties should still be discovered")
            .plane_in_formats(),
        None
    );
}

#[test]
fn native_libdrm_plane_format_modifier_table_reduces_supported_modifiers() {
    let blob = format_modifier_blob(
        &[
            drm::buffer::DrmFourcc::Xrgb8888 as u32,
            drm::buffer::DrmFourcc::Argb8888 as u32,
        ],
        &[(0b01, 0, u64::from(drm::buffer::DrmModifier::Linear))],
    );

    let parsed = LibdrmNativePlaneFormatModifierTable::parse_for_format(
        &blob,
        drm::buffer::DrmFourcc::Xrgb8888,
    );

    assert_eq!(
        parsed.status,
        LibdrmNativePlaneFormatModifierTableParseStatus::Parsed
    );
    let table = parsed
        .table
        .expect("supported modifier blob should produce a table");
    assert_eq!(table.modifiers(), &[drm::buffer::DrmModifier::Linear]);
    assert_eq!(
        table.reduced_status(),
        LibdrmNativePlaneFormatModifierSupportStatus::Linear
    );

    let unsupported_format = LibdrmNativePlaneFormatModifierTable::parse_for_format(
        &blob,
        drm::buffer::DrmFourcc::Rgb565,
    );
    assert_eq!(
        unsupported_format.status,
        LibdrmNativePlaneFormatModifierTableParseStatus::FormatUnsupported
    );

    let unsupported_modifier = LibdrmNativePlaneFormatModifierTable::parse_for_format(
        &blob,
        drm::buffer::DrmFourcc::Argb8888,
    );
    assert_eq!(
        unsupported_modifier.status,
        LibdrmNativePlaneFormatModifierTableParseStatus::ModifierUnsupported
    );
}

#[test]
fn native_libdrm_plane_format_modifier_table_rejects_malformed_blobs() {
    assert_eq!(
        LibdrmNativePlaneFormatModifierTable::parse_for_format(
            &[0; 8],
            drm::buffer::DrmFourcc::Xrgb8888
        )
        .status,
        LibdrmNativePlaneFormatModifierTableParseStatus::Malformed
    );

    let mut unsupported_version = format_modifier_blob(
        &[drm::buffer::DrmFourcc::Xrgb8888 as u32],
        &[(0b1, 0, u64::from(drm::buffer::DrmModifier::Linear))],
    );
    write_u32(&mut unsupported_version, 0, 2);
    assert_eq!(
        LibdrmNativePlaneFormatModifierTable::parse_for_format(
            &unsupported_version,
            drm::buffer::DrmFourcc::Xrgb8888
        )
        .status,
        LibdrmNativePlaneFormatModifierTableParseStatus::UnsupportedVersion
    );
}

fn format_modifier_blob(formats: &[u32], modifiers: &[(u64, u32, u64)]) -> Vec<u8> {
    let formats_offset = 24usize;
    let modifiers_offset = align_to(formats_offset + formats.len() * 4, 8);
    let mut blob = vec![0; modifiers_offset + modifiers.len() * 24];
    write_u32(&mut blob, 0, 1);
    write_u32(&mut blob, 8, formats.len() as u32);
    write_u32(&mut blob, 12, formats_offset as u32);
    write_u32(&mut blob, 16, modifiers.len() as u32);
    write_u32(&mut blob, 20, modifiers_offset as u32);

    for (index, format) in formats.iter().enumerate() {
        write_u32(&mut blob, formats_offset + index * 4, *format);
    }
    for (index, (format_mask, offset, modifier)) in modifiers.iter().enumerate() {
        let base = modifiers_offset + index * 24;
        write_u64(&mut blob, base, *format_mask);
        write_u32(&mut blob, base + 8, *offset);
        write_u64(&mut blob, base + 16, *modifier);
    }

    blob
}

fn align_to(value: usize, alignment: usize) -> usize {
    (value + alignment - 1) & !(alignment - 1)
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_ne_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_ne_bytes());
}

#[test]
fn native_libdrm_primary_plane_property_discovery_fails_closed_for_missing_groups() {
    let missing_connector = FakeNativePropertyLookupDevice {
        connector: Ok(LibdrmNativePropertyHandleSet::new(Vec::<(
            &str,
            drm::control::property::Handle,
        )>::new())),
        ..full_property_lookup_device()
    };
    assert_eq!(
        discover_native_primary_plane_property_handles(
            &missing_connector,
            connector_handle(),
            crtc_handle(),
            plane_handle(),
        )
        .status,
        LibdrmNativePrimaryPlanePropertyDiscoveryStatus::MissingConnectorProperty
    );

    let missing_crtc = FakeNativePropertyLookupDevice {
        crtc: Ok(LibdrmNativePropertyHandleSet::new([(
            "MODE_ID",
            property_handle(102),
        )])),
        ..full_property_lookup_device()
    };
    assert_eq!(
        discover_native_primary_plane_property_handles(
            &missing_crtc,
            connector_handle(),
            crtc_handle(),
            plane_handle(),
        )
        .status,
        LibdrmNativePrimaryPlanePropertyDiscoveryStatus::MissingCrtcProperty
    );

    let missing_plane = FakeNativePropertyLookupDevice {
        plane: Ok(LibdrmNativePropertyHandleSet::new([
            ("FB_ID", property_handle(104)),
            ("CRTC_ID", property_handle(105)),
        ])),
        ..full_property_lookup_device()
    };
    assert_eq!(
        discover_native_primary_plane_property_handles(
            &missing_plane,
            connector_handle(),
            crtc_handle(),
            plane_handle(),
        )
        .status,
        LibdrmNativePrimaryPlanePropertyDiscoveryStatus::MissingPlaneProperty
    );
}

#[test]
fn native_libdrm_primary_plane_property_discovery_fails_closed_on_read_error() {
    let read_failed = FakeNativePropertyLookupDevice {
        connector: Err(io::Error::from(io::ErrorKind::PermissionDenied)),
        ..full_property_lookup_device()
    };
    let discovery = discover_native_primary_plane_property_handles(
        &read_failed,
        connector_handle(),
        crtc_handle(),
        plane_handle(),
    );

    assert_eq!(
        discovery.status,
        LibdrmNativePrimaryPlanePropertyDiscoveryStatus::ReadFailed
    );
    assert!(discovery.properties.is_none());
}

#[test]
fn native_vrr_discovery_requires_capability_value_and_enable_property() {
    let capable = FakeNativePropertyLookupDevice {
        connector: Ok(LibdrmNativePropertyHandleSet::new([
            ("CRTC_ID", property_handle(101)),
            ("vrr_capable", property_handle(115)),
        ])),
        crtc: Ok(LibdrmNativePropertyHandleSet::new([
            ("MODE_ID", property_handle(102)),
            ("ACTIVE", property_handle(103)),
            ("VRR_ENABLED", property_handle(116)),
        ])),
        connector_value: Ok(Some(1)),
        ..full_property_lookup_device()
    };
    let discovered = discover_native_vrr_properties(&capable, connector_handle(), crtc_handle());
    assert_eq!(
        discovered.status,
        LibdrmNativeVrrPropertyDiscoveryStatus::Discovered
    );
    assert!(discovered.capable);
    assert_eq!(discovered.enable_property, Some(property_handle(116)));

    let unsupported = FakeNativePropertyLookupDevice {
        connector_value: Ok(Some(0)),
        ..capable
    };
    assert_eq!(
        discover_native_vrr_properties(&unsupported, connector_handle(), crtc_handle()).status,
        LibdrmNativeVrrPropertyDiscoveryStatus::Unsupported
    );
}

#[test]
fn native_libdrm_primary_plane_selection_feeds_request_builder() {
    let selection = select_native_primary_plane_target(&full_kms_selection_device());

    assert_eq!(
        selection.status,
        LibdrmNativePrimaryPlaneSelectionStatus::Selected
    );
    let selected = selection
        .selection
        .expect("complete KMS path should select a private primary plane target");
    assert_eq!(
        selected.size(),
        Size {
            width: 1280,
            height: 720,
        }
    );
    let resource_create = create_native_primary_plane_resources(
        &full_primary_plane_resource_device(),
        selected,
        &scanout_buffer(selected.size()),
    );
    assert_eq!(
        resource_create.status,
        LibdrmNativePrimaryPlaneResourceCreateStatus::Created
    );
    let objects = resource_create
        .resources
        .expect("complete resource device should produce framebuffer and mode blob")
        .into_objects(selected);
    let properties = discover_native_primary_plane_property_handles(
        &full_property_lookup_device(),
        connector_handle(),
        crtc_handle(),
        plane_handle(),
    )
    .properties
    .expect("complete property lookup should produce private property handles");
    let build = build_native_primary_plane_atomic_request(objects, properties);

    assert_eq!(build.status, LibdrmNativeAtomicRequestBuildStatus::Built);
    assert!(build.request.is_some());
}

#[test]
fn native_multi_output_selection_is_deterministic_and_disjoint() {
    let connector_a = drm::control::from_u32::<drm::control::connector::Handle>(21).unwrap();
    let connector_b = drm::control::from_u32::<drm::control::connector::Handle>(22).unwrap();
    let encoder_a = drm::control::from_u32::<drm::control::encoder::Handle>(31).unwrap();
    let encoder_b = drm::control::from_u32::<drm::control::encoder::Handle>(32).unwrap();
    let crtc_a = drm::control::from_u32::<drm::control::crtc::Handle>(41).unwrap();
    let crtc_b = drm::control::from_u32::<drm::control::crtc::Handle>(42).unwrap();
    let plane_a = drm::control::from_u32::<drm::control::plane::Handle>(51).unwrap();
    let plane_b = drm::control::from_u32::<drm::control::plane::Handle>(52).unwrap();
    let device = FakeMultiNativeKmsSelectionDevice {
        connectors: vec![
            (
                connector_b,
                LibdrmNativeConnectorSnapshot::new(
                    true,
                    Some(encoder_b),
                    [encoder_b],
                    Some(Size {
                        width: 1920,
                        height: 1080,
                    }),
                ),
            ),
            (
                connector_a,
                LibdrmNativeConnectorSnapshot::new(
                    true,
                    Some(encoder_a),
                    [encoder_a],
                    Some(Size {
                        width: 1280,
                        height: 720,
                    }),
                ),
            ),
        ],
        crtcs: vec![crtc_b, crtc_a],
        encoders: vec![
            (
                encoder_a,
                LibdrmNativeEncoderSnapshot::new(Some(crtc_a), [crtc_a]),
            ),
            (
                encoder_b,
                LibdrmNativeEncoderSnapshot::new(Some(crtc_b), [crtc_b]),
            ),
        ],
        planes: vec![
            (plane_b, LibdrmNativePlaneSnapshot::new([crtc_b])),
            (plane_a, LibdrmNativePlaneSnapshot::new([crtc_a])),
        ],
        cursor_planes: Vec::new(),
    };

    let selected = select_native_primary_plane_targets(&device);
    assert_eq!(
        selected.status,
        LibdrmNativePrimaryPlaneSelectionSetStatus::SelectedAll
    );
    assert_eq!(selected.connected_connectors, 2);
    assert_eq!(selected.selections.len(), 2);
    assert_eq!(selected.selections[0].connector_id(), 21);
    assert_eq!(selected.selections[0].crtc_id(), 41);
    assert_eq!(selected.selections[0].plane_id(), 51);
    assert_eq!(selected.selections[1].connector_id(), 22);
    assert_eq!(selected.selections[1].crtc_id(), 42);
    assert_eq!(selected.selections[1].plane_id(), 52);
}

#[test]
fn an_ungrouped_connector_is_its_own_logical_output() {
    // The ordinary desktop, and the default. A connector in no group must not be
    // reported as sharing an output, or an unconfigured machine would mirror by
    // accident.
    let grouping = NativeMirrorGrouping::none();

    assert!(grouping.is_empty());
    assert_eq!(grouping.group_of("DP-1"), None);
    assert!(!grouping.is_mirrored("DP-1"));
}

#[test]
fn excluded_connector_cannot_consume_the_requested_heads_only_crtc() {
    let first = drm::control::from_u32::<drm::control::connector::Handle>(21).unwrap();
    let second = drm::control::from_u32::<drm::control::connector::Handle>(22).unwrap();
    let encoder = drm::control::from_u32::<drm::control::encoder::Handle>(31).unwrap();
    let crtc = drm::control::from_u32::<drm::control::crtc::Handle>(41).unwrap();
    let plane = drm::control::from_u32::<drm::control::plane::Handle>(51).unwrap();
    let device = FakeMultiNativeKmsSelectionDevice {
        connectors: [first, second]
            .into_iter()
            .map(|handle| {
                (
                    handle,
                    LibdrmNativeConnectorSnapshot::new(
                        true,
                        Some(encoder),
                        [encoder],
                        Some(Size {
                            width: 1920,
                            height: 1080,
                        }),
                    ),
                )
            })
            .collect(),
        crtcs: vec![crtc],
        encoders: vec![(
            encoder,
            LibdrmNativeEncoderSnapshot::new(Some(crtc), [crtc]),
        )],
        planes: vec![(plane, LibdrmNativePlaneSnapshot::new([crtc]))],
        cursor_planes: Vec::new(),
    };
    let all = select_native_primary_plane_targets(&device);
    assert_eq!(
        all.status,
        LibdrmNativePrimaryPlaneSelectionSetStatus::Partial
    );
    assert_eq!(all.selections[0].connector_id(), 21);
    let selected =
        sophia_backend_live::select_native_primary_plane_targets_matching(&device, |id| id == 22);
    assert_eq!(
        selected.status,
        LibdrmNativePrimaryPlaneSelectionSetStatus::SelectedAll
    );
    assert_eq!(selected.connected_connectors, 1);
    assert_eq!(selected.selections[0].connector_id(), 22);
    assert_eq!(selected.selections[0].crtc_id(), 41);
}

#[test]
fn connectors_in_one_group_share_a_logical_output() {
    let grouping = NativeMirrorGrouping::new([
        vec!["DP-1".to_owned(), "DP-2".to_owned()],
        vec!["DP-3".to_owned()],
    ])
    .expect("the grouping is well formed");

    assert_eq!(grouping.group_of("DP-1"), grouping.group_of("DP-2"));
    assert_ne!(grouping.group_of("DP-1"), grouping.group_of("DP-3"));
    assert!(grouping.is_mirrored("DP-1"));
    assert!(grouping.is_group_primary("DP-1"));
    assert!(!grouping.is_group_primary("DP-2"));
    // A one-member group shares its output with nobody, so it is not mirrored even
    // though it was named. Reporting it as mirrored would make the mirror path
    // reachable for a desktop that asked for nothing.
    assert!(!grouping.is_mirrored("DP-3"));
}

#[test]
fn one_connector_cannot_belong_to_two_groups() {
    // Two groups claiming a connector leaves the identity of its logical output
    // undefined, which is the one thing a grouping exists to settle.
    assert_eq!(
        NativeMirrorGrouping::new([
            vec!["DP-1".to_owned(), "DP-2".to_owned()],
            vec!["DP-2".to_owned(), "DP-3".to_owned()],
        ]),
        Err(NativeMirrorGroupingError::ConnectorInTwoGroups(
            "DP-2".to_owned()
        ))
    );
}

#[test]
fn an_empty_group_identifies_no_output() {
    assert_eq!(
        NativeMirrorGrouping::new([vec!["DP-1".to_owned()], Vec::new()]),
        Err(NativeMirrorGroupingError::EmptyGroup)
    );
}

/// A card that offers cursor planes has them discovered beside the primaries,
/// one per CRTC and never shared.
///
/// Discovery only; nothing commits a cursor plane yet. A card without one --
/// or whose single cursor plane is already serving another head -- keeps the
/// legacy ioctl, so the absence has to be an ordinary answer rather than a
/// failure.
#[test]
fn native_selection_discovers_a_cursor_plane_per_crtc() {
    let connector_a = drm::control::from_u32::<drm::control::connector::Handle>(21).unwrap();
    let connector_b = drm::control::from_u32::<drm::control::connector::Handle>(22).unwrap();
    let encoder_a = drm::control::from_u32::<drm::control::encoder::Handle>(31).unwrap();
    let encoder_b = drm::control::from_u32::<drm::control::encoder::Handle>(32).unwrap();
    let crtc_a = drm::control::from_u32::<drm::control::crtc::Handle>(41).unwrap();
    let crtc_b = drm::control::from_u32::<drm::control::crtc::Handle>(42).unwrap();
    let plane_a = drm::control::from_u32::<drm::control::plane::Handle>(51).unwrap();
    let plane_b = drm::control::from_u32::<drm::control::plane::Handle>(52).unwrap();
    let cursor_a = drm::control::from_u32::<drm::control::plane::Handle>(61).unwrap();
    let cursor_b = drm::control::from_u32::<drm::control::plane::Handle>(62).unwrap();

    let device = FakeMultiNativeKmsSelectionDevice {
        connectors: vec![
            (
                connector_a,
                LibdrmNativeConnectorSnapshot::new(
                    true,
                    Some(encoder_a),
                    [encoder_a],
                    Some(Size {
                        width: 1280,
                        height: 720,
                    }),
                ),
            ),
            (
                connector_b,
                LibdrmNativeConnectorSnapshot::new(
                    true,
                    Some(encoder_b),
                    [encoder_b],
                    Some(Size {
                        width: 1920,
                        height: 1080,
                    }),
                ),
            ),
        ],
        crtcs: vec![crtc_a, crtc_b],
        encoders: vec![
            (
                encoder_a,
                LibdrmNativeEncoderSnapshot::new(Some(crtc_a), [crtc_a]),
            ),
            (
                encoder_b,
                LibdrmNativeEncoderSnapshot::new(Some(crtc_b), [crtc_b]),
            ),
        ],
        planes: vec![
            (plane_a, LibdrmNativePlaneSnapshot::new([crtc_a])),
            (plane_b, LibdrmNativePlaneSnapshot::new([crtc_b])),
            (cursor_a, LibdrmNativePlaneSnapshot::new([crtc_a])),
            (cursor_b, LibdrmNativePlaneSnapshot::new([crtc_b])),
        ],
        cursor_planes: vec![cursor_a, cursor_b],
    };

    let selected = select_native_primary_plane_targets(&device);
    assert_eq!(
        selected.status,
        LibdrmNativePrimaryPlaneSelectionSetStatus::SelectedAll
    );
    assert_eq!(selected.selections.len(), 2);
    assert_eq!(
        selected.selections[0].cursor_plane().map(u32::from),
        Some(61)
    );
    assert_eq!(
        selected.selections[1].cursor_plane().map(u32::from),
        Some(62)
    );
    assert_ne!(
        selected.selections[0].cursor_plane(),
        selected.selections[1].cursor_plane(),
        "a cursor plane drives one CRTC at a time"
    );
}

/// A card with no cursor plane selects exactly as before. The legacy ioctl is
/// still the path there, and discovery reporting nothing is the ordinary
/// answer rather than a refusal.
#[test]
fn native_selection_without_a_cursor_plane_is_unchanged() {
    let connector = drm::control::from_u32::<drm::control::connector::Handle>(21).unwrap();
    let encoder = drm::control::from_u32::<drm::control::encoder::Handle>(31).unwrap();
    let crtc = drm::control::from_u32::<drm::control::crtc::Handle>(41).unwrap();
    let plane = drm::control::from_u32::<drm::control::plane::Handle>(51).unwrap();

    let device = FakeMultiNativeKmsSelectionDevice {
        connectors: vec![(
            connector,
            LibdrmNativeConnectorSnapshot::new(
                true,
                Some(encoder),
                [encoder],
                Some(Size {
                    width: 1280,
                    height: 720,
                }),
            ),
        )],
        crtcs: vec![crtc],
        encoders: vec![(
            encoder,
            LibdrmNativeEncoderSnapshot::new(Some(crtc), [crtc]),
        )],
        planes: vec![(plane, LibdrmNativePlaneSnapshot::new([crtc]))],
        cursor_planes: Vec::new(),
    };

    let selected = select_native_primary_plane_targets(&device);
    assert_eq!(
        selected.status,
        LibdrmNativePrimaryPlaneSelectionSetStatus::SelectedAll
    );
    assert_eq!(selected.selections[0].plane_id(), 51);
    assert_eq!(selected.selections[0].cursor_plane(), None);
}

/// One cursor plane between two CRTCs goes to the first, and the second head
/// keeps the legacy path rather than sharing it.
#[test]
fn a_single_cursor_plane_is_not_shared_between_heads() {
    let connector_a = drm::control::from_u32::<drm::control::connector::Handle>(21).unwrap();
    let connector_b = drm::control::from_u32::<drm::control::connector::Handle>(22).unwrap();
    let encoder_a = drm::control::from_u32::<drm::control::encoder::Handle>(31).unwrap();
    let encoder_b = drm::control::from_u32::<drm::control::encoder::Handle>(32).unwrap();
    let crtc_a = drm::control::from_u32::<drm::control::crtc::Handle>(41).unwrap();
    let crtc_b = drm::control::from_u32::<drm::control::crtc::Handle>(42).unwrap();
    let plane_a = drm::control::from_u32::<drm::control::plane::Handle>(51).unwrap();
    let plane_b = drm::control::from_u32::<drm::control::plane::Handle>(52).unwrap();
    let shared_cursor = drm::control::from_u32::<drm::control::plane::Handle>(61).unwrap();

    let device = FakeMultiNativeKmsSelectionDevice {
        connectors: vec![
            (
                connector_a,
                LibdrmNativeConnectorSnapshot::new(
                    true,
                    Some(encoder_a),
                    [encoder_a],
                    Some(Size {
                        width: 1280,
                        height: 720,
                    }),
                ),
            ),
            (
                connector_b,
                LibdrmNativeConnectorSnapshot::new(
                    true,
                    Some(encoder_b),
                    [encoder_b],
                    Some(Size {
                        width: 1920,
                        height: 1080,
                    }),
                ),
            ),
        ],
        crtcs: vec![crtc_a, crtc_b],
        encoders: vec![
            (
                encoder_a,
                LibdrmNativeEncoderSnapshot::new(Some(crtc_a), [crtc_a]),
            ),
            (
                encoder_b,
                LibdrmNativeEncoderSnapshot::new(Some(crtc_b), [crtc_b]),
            ),
        ],
        planes: vec![
            (plane_a, LibdrmNativePlaneSnapshot::new([crtc_a])),
            (plane_b, LibdrmNativePlaneSnapshot::new([crtc_b])),
            (
                shared_cursor,
                LibdrmNativePlaneSnapshot::new([crtc_a, crtc_b]),
            ),
        ],
        cursor_planes: vec![shared_cursor],
    };

    let selected = select_native_primary_plane_targets(&device);
    assert_eq!(selected.selections.len(), 2);
    assert_eq!(
        selected.selections[0].cursor_plane().map(u32::from),
        Some(61)
    );
    assert_eq!(
        selected.selections[1].cursor_plane(),
        None,
        "the second head cannot borrow a cursor plane the first is driving"
    );
}
