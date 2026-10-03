#[test]
fn stale_controls_release_only_their_exact_layout_obligations() {
    let gone = SurfaceId::new(10, 1);
    let survivor = SurfaceId::new(11, 1);
    let transaction = TransactionId::from_raw(148);
    let geometry = Rect {
        x: 0,
        y: 0,
        width: 80,
        height: 60,
    };
    let state = sophia_protocol::PolicyPresentationState::default();
    let mut layout = PersistentLiveLayout {
        focus_to_apply: Some((transaction, gone)),
        pending: Some(PendingLiveWmLayout {
            transaction,
            layers: vec![test_layer(gone, geometry), test_layer(survivor, geometry)],
            requested_sizes: BTreeMap::from([(
                gone,
                Size {
                    width: 80,
                    height: 60,
                },
            )]),
            presentation_states: BTreeMap::from([(gone, state), (survivor, state)]),
            presentation_settlements: BTreeSet::from([gone]),
            configure_deliveries: 1,
            focus: Some(gone),
            deadline: Instant::now(),
            update: sophia_engine::WmTransactionUpdate {
                commit: TransactionCommit {
                    transaction,
                    outcome: TransactionOutcome::Committed,
                    applied_surfaces: vec![gone, survivor],
                },
            },
            moved_surfaces: 0,
            staged_transactions: BTreeMap::new(),
            admission_surfaces: BTreeSet::from([gone]),
            source: None,
            policy_settlement: None,
        }),
        ..Default::default()
    };
    let mut key = crate::session_control::SessionControlKey {
        client: sophia_x_authority::XServerFrontendClientId::from_raw(1),
        kind: sophia_x_authority::XAuthorityControlKind::ConfigureSurface,
        transaction: TransactionId::from_raw(147),
        surface: gone,
    };
    layout.retire_stale_control(key);
    assert!(!layout.pending_is_ready());
    assert_eq!(layout.focus_to_apply, Some((transaction, gone)));
    key.transaction = transaction;
    layout.retire_stale_control(key);
    assert!(layout.pending_is_ready());
    assert_eq!(layout.focus_to_apply, None);
    let pending = layout.pending.as_ref().unwrap();
    assert_eq!(
        pending.layers.iter().map(|l| l.surface).collect::<Vec<_>>(),
        [survivor]
    );
    assert_eq!(pending.update.commit.applied_surfaces, [survivor]);
    assert_eq!(
        pending.presentation_states,
        BTreeMap::from([(survivor, state)])
    );
    assert!(pending.admission_surfaces.is_empty());
    assert_eq!(pending.focus, None);
    // A duplicate completion cannot acquire a new obligation.
    layout.retire_stale_control(key);
    assert!(layout.pending_is_ready());
    let result = layout
        .resolve_pending()
        .expect("the same control pass can resolve without input");
    assert_eq!(result.update.commit.applied_surfaces, [survivor]);
    assert_eq!(result.update.commit.outcome, TransactionOutcome::Committed);
    assert!(!layout.layers.contains_key(&gone));
}
