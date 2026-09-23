#[test]
fn post_dispatch_failure_preserves_peer_owned_mapping_and_cpu_effects() {
    let surface = SurfaceId::new(0x0020_0001, 7);
    let owner = XServerFrontendClientId::from_raw(1);
    let requestor = XServerFrontendClientId::from_raw(2);
    let transaction = TransactionId::from_raw(29);
    let mut response = XAuthorityResponsePacket::accepted(transaction);
    response.surfaces.push(sophia_protocol::AuthoritySurface {
        authority: sophia_protocol::AuthorityKind::SophiaX,
        local_id: sophia_protocol::AuthorityLocalId::new(
            surface.index().into(),
            surface.generation(),
        ),
        surface,
        namespace: Some(NamespaceId::from_raw(1)),
        presentation: sophia_protocol::SurfacePresentationRole::ClientPositioned,
        kind: sophia_protocol::LayoutNodeKind::Popup,
        placement_preference: sophia_protocol::SurfacePlacementPreference::Floating,
        presentation_owner: None,
        stack_rank: 0,
        mapped: true,
        geometry: Rect {
            x: 10,
            y: 20,
            width: 1,
            height: 1,
        },
        constraints: sophia_protocol::SurfaceConstraints {
            min_size: None,
            max_size: None,
        },
        generation: 1,
    });
    let pixels = Arc::new(vec![0x12, 0x34, 0x56, 0xff]);
    let cpu_update =
        crate::XAuthorityCpuBufferUpdate::Replace(crate::XAuthorityCpuBufferSnapshot {
            handle: 41,
            drawable: crate::XResourceId {
                local: sophia_protocol::AuthorityLocalId::new(
                    surface.index().into(),
                    surface.generation(),
                ),
            },
            size: sophia_protocol::Size {
                width: 1,
                height: 1,
            },
            stride: 4,
            format: 0x3432_5258,
            generation: 3,
            bytes: pixels.clone(),
        });
    let trace = X11DispatchObservation {
        transaction,
        client: requestor,
        admission: None,
        resource_id_range: crate::XWireClientResourceRange {
            base: 0x0040_0000,
            mask: 0x000f_ffff,
        },
        sequence: 5,
        major_opcode: 8,
        minor_opcode: 0,
        request_stage: X11ObservedRequestStage::Other,
        failure: None,
        result: XDispatchResult {
            response: Some(response),
            outputs: Vec::new(),
            metadata_candidates: Vec::new(),
        },
        surface_routes: vec![crate::XAuthoritySurfaceRouteObservation {
            surface,
            client: owner,
            admission: None,
        }],
        surface_output_reservations: Vec::new(),
        cpu_buffer_updates: vec![cpu_update],
        received_fd_count: 0,
        received_fds: Vec::new(),
        dri3_pixmap_import: None,
        dri3_fence_import: None,
        present_submission: None,
        software_present_submission: None,
        released_dma_bufs: vec![sophia_protocol::BufferHandle::from_raw(37)],
        released_fences: vec![sophia_protocol::FenceHandle::from_raw(38)],
        server_reply_fd_count: 0,
    };
    let retained = failed_x11_dispatch_observation(Some(trace), true, true, false).unwrap();
    assert_eq!(
        retained.failure, None,
        "delivery failure cannot turn complete effects into an aborted request"
    );
    let batch = XAuthorityObservedTransactionBatch::from_dispatch_observation(&retained).unwrap();
    assert_eq!(batch.client, Some(requestor));
    assert_eq!(batch.surface_routes[0].client, owner);
    assert!(batch.surface_presentations[0].mapped);
    assert_eq!(batch.surface_presentations[0].surface, surface);
    let [crate::XAuthorityCpuBufferUpdate::Replace(snapshot)] = batch.cpu_buffer_updates.as_slice()
    else {
        panic!("completed CPU update must survive failure");
    };
    assert!(Arc::ptr_eq(&snapshot.bytes, &pixels));
    assert_eq!(snapshot.generation, 3);
    assert_eq!(
        batch.released_dma_bufs,
        [sophia_protocol::BufferHandle::from_raw(37)]
    );
    assert_eq!(
        batch.released_fences,
        [sophia_protocol::FenceHandle::from_raw(38)]
    );
    let partial = failed_x11_dispatch_observation(Some(retained), true, false, false).unwrap();
    assert_eq!(
        partial.failure,
        Some(X11ObservedDispatchFailure::UnpublishedEffects)
    );
    assert!(
        XAuthorityObservedTransactionBatch::from_dispatch_observation(&partial).is_none(),
        "partial effects never certify an applied prefix"
    );
}

/// The tail tells a departed client from a broken service. A dispatch that
/// started and did not complete because the peer left is the client's, and
/// resolves its ticket with no facts; the same shape without a departure is the
/// service's, and stays the incomplete prefix the publishers must stop for. A
/// departure before dispatch began is still simply aborted.
#[cfg(unix)]
#[test]
fn a_departure_mid_dispatch_is_observed_as_the_clients_and_publishes_nothing() {
    let trace = || X11DispatchObservation {
        transaction: TransactionId::from_raw(9),
        client: XServerFrontendClientId(4),
        admission: None,
        resource_id_range: crate::XWireClientResourceRange {
            base: 0x0020_0000,
            mask: 0x001f_ffff,
        },
        sequence: 1,
        major_opcode: 146,
        minor_opcode: 2,
        request_stage: X11ObservedRequestStage::Other,
        failure: None,
        result: XDispatchResult {
            response: None,
            outputs: Vec::new(),
            metadata_candidates: Vec::new(),
        },
        surface_routes: Vec::new(),
        surface_output_reservations: Vec::new(),
        cpu_buffer_updates: Vec::new(),
        received_fd_count: 0,
        received_fds: Vec::new(),
        dri3_pixmap_import: None,
        dri3_fence_import: None,
        present_submission: None,
        software_present_submission: None,
        released_dma_bufs: Vec::new(),
        released_fences: Vec::new(),
        server_reply_fd_count: 0,
    };
    let departed = failed_x11_dispatch_observation(Some(trace()), true, false, true).unwrap();
    assert_eq!(departed.failure, Some(X11ObservedDispatchFailure::ClientDeparted));
    assert!(
        XAuthorityObservedTransactionBatch::from_dispatch_observation(&departed).is_none(),
        "a departed client's ticket resolves with no facts"
    );

    let broken = failed_x11_dispatch_observation(Some(trace()), true, false, false).unwrap();
    assert_eq!(
        broken.failure,
        Some(X11ObservedDispatchFailure::UnpublishedEffects),
        "a service ending mid-dispatch is still the incomplete prefix publishers stop for"
    );

    let aborted = failed_x11_dispatch_observation(Some(trace()), false, false, true).unwrap();
    assert_eq!(
        aborted.failure,
        Some(X11ObservedDispatchFailure::DispatchAborted),
        "leaving before dispatch began is an abort, whatever the cause"
    );
}
