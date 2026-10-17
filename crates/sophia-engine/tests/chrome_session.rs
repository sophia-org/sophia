mod support;
use support::*;

#[test]
fn metadata_preparation_leaves_the_table_unchanged_and_moves_label_storage() {
    let surface = SurfaceId::new(15, 1);
    let mut table = ChromeDescriptorTable::default();
    table.apply_metadata(metadata(surface, "Current", 9));
    let candidate = metadata(surface, "Replacement", 10);
    let storage = candidate.label.as_ref().unwrap().as_ptr();
    let prepared = table.prepare_metadata(candidate).unwrap();
    assert_eq!(prepared.label.as_ref().unwrap().text.as_ptr(), storage);
    assert_eq!(table.get(surface).unwrap().generation, 9);
    // Dropping a prepared effect is an abort, not a partial update.
    drop(prepared);
    assert_eq!(
        table.get(surface).unwrap().label.as_ref().unwrap().text,
        "Current"
    );
    for (candidate, expected) in [
        (
            metadata(surface, "Stale", 8),
            MetadataChromeRejectReason::StaleGeneration,
        ),
        (
            metadata(surface, "Bad\nlabel", 10),
            MetadataChromeRejectReason::InvalidLabel,
        ),
        (
            metadata(SurfaceId::new(0, 0), "Invalid", 10),
            MetadataChromeRejectReason::InvalidSurface,
        ),
    ] {
        assert_eq!(table.prepare_metadata(candidate), Err(expected));
        assert_eq!(table.get(surface).unwrap().generation, 9);
        assert_eq!(table.len(), 1);
    }
    let prepared = table
        .prepare_metadata(metadata(surface, "Replacement", 10))
        .unwrap();
    table.upsert(prepared);
    assert_eq!(table.get(surface).unwrap().generation, 10);
}

#[test]
fn chrome_broker_keeps_metadata_separate_from_layout() {
    let mut broker = ChromeDescriptorTable::default();
    let surface = SurfaceId::new(3, 1);

    broker.upsert(ChromeDescriptor {
        surface,
        label: Some(DisplayLabel {
            text: "Redacted Title".to_owned(),
            redacted: true,
        }),
        icon: Some(IconTokenId::from_raw(12)),
        trust_level: TrustLevel::Isolated,
        attention: AttentionState::None,
        generation: 4,
    });

    let descriptor = broker.get(surface).unwrap();

    assert_eq!(broker.len(), 1);
    assert_eq!(
        descriptor.label.as_ref().map(|label| label.redacted),
        Some(true)
    );
    assert_eq!(descriptor.icon, Some(IconTokenId::from_raw(12)));
    assert_eq!(descriptor.trust_level, TrustLevel::Isolated);
}

#[test]
fn chrome_broker_removes_surface_metadata() {
    let mut broker = ChromeDescriptorTable::default();
    let surface = SurfaceId::new(4, 1);

    broker.upsert(ChromeDescriptor {
        surface,
        label: None,
        icon: None,
        trust_level: TrustLevel::Unknown,
        attention: AttentionState::None,
        generation: 1,
    });

    assert!(broker.remove_surface(surface).is_some());
    assert!(broker.get(surface).is_none());
    assert!(broker.is_empty());
}

#[test]
fn metadata_broker_output_updates_chrome_descriptor() {
    let mut broker = ChromeDescriptorTable::default();
    let surface = SurfaceId::new(5, 1);

    assert_eq!(
        broker.apply_metadata(SanitizedChromeMetadata {
            surface,
            label: Some("Untrusted Browser".to_owned()),
            label_redacted: true,
            icon: Some(IconTokenId::from_raw(7)),
            trust_level: TrustLevel::Untrusted,
            attention: AttentionState::Notice,
            generation: 3,
        }),
        MetadataChromeUpdate::Upserted { surface }
    );

    let descriptor = broker.get(surface).unwrap();
    assert_eq!(descriptor.surface, surface);
    assert_eq!(
        descriptor.label.as_ref(),
        Some(&DisplayLabel {
            text: "Untrusted Browser".to_owned(),
            redacted: true,
        })
    );
    assert_eq!(descriptor.icon, Some(IconTokenId::from_raw(7)));
    assert_eq!(descriptor.trust_level, TrustLevel::Untrusted);
    assert_eq!(descriptor.attention, AttentionState::Notice);
    assert_eq!(descriptor.generation, 3);
}

#[test]
fn metadata_broker_output_rejects_stale_generation() {
    let mut broker = ChromeDescriptorTable::default();
    let surface = SurfaceId::new(6, 1);

    broker.apply_metadata(metadata(surface, "Current", 9));
    let update = broker.apply_metadata(metadata(surface, "Old", 8));

    assert_eq!(
        update,
        MetadataChromeUpdate::Rejected(MetadataChromeRejectReason::StaleGeneration)
    );
    assert_eq!(
        broker
            .get(surface)
            .and_then(|descriptor| descriptor.label.as_ref())
            .map(|label| label.text.as_str()),
        Some("Current")
    );
}

#[test]
fn metadata_broker_output_rejects_unsanitized_label() {
    let mut broker = ChromeDescriptorTable::default();
    let surface = SurfaceId::new(7, 1);
    let mut metadata = metadata(surface, "Bad\nTitle", 1);
    metadata.label_redacted = false;

    let update = broker.apply_metadata(metadata);

    assert_eq!(
        update,
        MetadataChromeUpdate::Rejected(MetadataChromeRejectReason::InvalidLabel)
    );
    assert!(broker.get(surface).is_none());
}

#[test]
fn metadata_broker_removal_clears_descriptor_with_generation_check() {
    let mut broker = ChromeDescriptorTable::default();
    let surface = SurfaceId::new(8, 1);

    broker.apply_metadata(metadata(surface, "Visible", 4));
    assert_eq!(
        broker.remove_metadata(surface, 3),
        MetadataChromeUpdate::Rejected(MetadataChromeRejectReason::StaleGeneration)
    );
    assert!(broker.get(surface).is_some());

    assert_eq!(
        broker.remove_metadata(surface, 4),
        MetadataChromeUpdate::Removed { surface }
    );
    assert!(broker.get(surface).is_none());
}

#[test]
fn notification_chrome_presents_only_after_delivery_command() {
    let mut presenter = NotificationChromePresenter::new();
    let request = notification_request(42);
    let transfer = request.transfer;

    assert_eq!(
        presenter.stage_request(&request),
        NotificationChromeUpdate::Staged { transfer }
    );
    assert!(presenter.pending(transfer).is_some());
    assert!(presenter.visible(transfer).is_none());

    let update = presenter.apply_portal_command(&PortalCommand::DeliverNotification { transfer });

    assert_eq!(update, NotificationChromeUpdate::Presented { transfer });
    assert!(presenter.pending(transfer).is_none());
    let visible = presenter.visible(transfer).unwrap();
    assert_eq!(visible.summary, "Build finished");
    assert_eq!(visible.body.as_deref(), Some("Sophia smoke completed"));
    assert_eq!(visible.urgency, NotificationUrgency::Normal);
}

#[test]
fn notification_chrome_drop_dismisses_pending_notification() {
    let mut presenter = NotificationChromePresenter::new();
    let request = notification_request(43);
    let transfer = request.transfer;

    presenter.stage_request(&request);
    let update = presenter.apply_portal_command(&PortalCommand::DropNotification { transfer });

    assert_eq!(update, NotificationChromeUpdate::Dismissed { transfer });
    assert!(presenter.pending(transfer).is_none());
    assert!(presenter.visible(transfer).is_none());
}

#[test]
fn notification_chrome_rejects_unknown_delivery() {
    let mut presenter = NotificationChromePresenter::new();
    let transfer = PortalTransferId::from_raw(99);

    let update = presenter.apply_portal_command(&PortalCommand::DeliverNotification { transfer });

    assert_eq!(
        update,
        NotificationChromeUpdate::Rejected(NotificationChromeRejectReason::UnknownTransfer)
    );
}

#[test]
fn notification_chrome_ignores_unrelated_portal_commands() {
    let transfer = PortalTransferId::from_raw(12);

    assert_eq!(
        notification_chrome_command_from_portal(&PortalCommand::HandoffClipboard { transfer }),
        None
    );
}

#[test]
fn chrome_close_request_validates_generation_and_closability() {
    let engine = HeadlessEngine::default();
    let surface = SurfaceId::new(9, 1);
    let nodes = vec![layout_node(surface, 3, true)];
    let request = ChromeActionRequest {
        surface,
        generation: 3,
        kind: ChromeActionKind::CloseSurfaceRequested,
    };

    assert_eq!(
        engine.validate_chrome_action(&request, &nodes),
        ChromeActionDecision::RequestPoliteClose { surface }
    );
}

#[test]
fn chrome_close_request_rejects_unknown_surface() {
    let engine = HeadlessEngine::default();
    let request = ChromeActionRequest {
        surface: SurfaceId::new(99, 1),
        generation: 1,
        kind: ChromeActionKind::CloseSurfaceRequested,
    };

    assert_eq!(
        engine.validate_chrome_action(&request, &[]),
        ChromeActionDecision::Rejected(ChromeActionRejectReason::UnknownSurface)
    );
}

#[test]
fn chrome_close_request_rejects_stale_generation() {
    let engine = HeadlessEngine::default();
    let surface = SurfaceId::new(10, 1);
    let nodes = vec![layout_node(surface, 7, true)];
    let request = ChromeActionRequest {
        surface,
        generation: 6,
        kind: ChromeActionKind::CloseSurfaceRequested,
    };

    assert_eq!(
        engine.validate_chrome_action(&request, &nodes),
        ChromeActionDecision::Rejected(ChromeActionRejectReason::StaleGeneration)
    );
}

#[test]
fn chrome_close_request_rejects_non_closable_surface() {
    let engine = HeadlessEngine::default();
    let surface = SurfaceId::new(11, 1);
    let nodes = vec![layout_node(surface, 2, false)];
    let request = ChromeActionRequest {
        surface,
        generation: 2,
        kind: ChromeActionKind::CloseSurfaceRequested,
    };

    assert_eq!(
        engine.validate_chrome_action(&request, &nodes),
        ChromeActionDecision::Rejected(ChromeActionRejectReason::NotClosable)
    );
}

#[test]
fn session_event_routes_accepted_chrome_close_to_x_bridge_command() {
    let engine = HeadlessEngine::default();
    let surface = SurfaceId::new(12, 1);
    let nodes = vec![layout_node(surface, 4, true)];
    let request = ChromeActionRequest {
        surface,
        generation: 4,
        kind: ChromeActionKind::CloseSurfaceRequested,
    };

    let update = engine.handle_session_event(SessionEvent::ChromeAction(request), &nodes);

    assert_eq!(
        update.chrome_decision,
        Some(ChromeActionDecision::RequestPoliteClose { surface })
    );
    assert_eq!(
        update.commands,
        vec![SessionCommand::RequestPoliteClose { surface }]
    );
}

#[test]
fn session_event_does_not_emit_close_command_for_rejected_chrome_action() {
    let engine = HeadlessEngine::default();
    let surface = SurfaceId::new(13, 1);
    let nodes = vec![layout_node(surface, 8, true)];
    let request = ChromeActionRequest {
        surface,
        generation: 7,
        kind: ChromeActionKind::CloseSurfaceRequested,
    };

    let update = engine.handle_session_event(SessionEvent::ChromeAction(request), &nodes);

    assert_eq!(
        update.chrome_decision,
        Some(ChromeActionDecision::Rejected(
            ChromeActionRejectReason::StaleGeneration
        ))
    );
    assert!(update.commands.is_empty());
}

#[test]
fn session_event_notifies_wm_only_after_surface_removed() {
    let engine = HeadlessEngine::default();
    let surface = SurfaceId::new(14, 1);
    let workspace = WorkspaceId::from_raw(3);
    let transaction = TransactionId::from_raw(99);

    let update = engine.handle_session_event(
        SessionEvent::SurfaceRemoved {
            transaction,
            surface,
            workspace,
        },
        &[],
    );

    assert_eq!(update.chrome_decision, None);
    assert_eq!(update.commands.len(), 1);
    let SessionCommand::SendWmRequest(request) = &update.commands[0] else {
        panic!("expected WM request command");
    };
    assert_eq!(request.transaction, transaction);
    assert_eq!(
        request.kind,
        WmRequestKind::SurfaceRemoved { surface, workspace }
    );
}
