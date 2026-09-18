fn install_stopped_lifecycle(fixture: &Fixture) {
    let registry = &fixture.private.broker.registry;
    let owner = &fixture.private.terminal.lifecycle;
    let lease = owner
        .register(client(), namespaced(client(), namespace()))
        .unwrap();
    registry
        .input_recovery
        .attach_lifecycle(client(), lease.gate())
        .unwrap();
    owner
        .register_query_gate(&lease.gate(), namespace(), client())
        .unwrap();
    *fixture._registration.lifecycle.lock().unwrap() = Some(lease);
}

fn close_stopped_lifecycle(fixture: &Fixture) {
    fixture.private.terminal.lifecycle.close_all();
    fixture
        .private
        .terminal
        .lifecycle
        .drive(NonZeroUsize::new(16).unwrap())
        .unwrap();
}

fn reconcile_stopped_pointer(
    fixture: &Fixture,
    hold: &mut Hold,
) -> Result<bool, PrivateTerminalDriveRefusal> {
    let connection = hold.connection();
    fixture
        .private
        .controller
        .under_common_as_origin(|authority, issuer| {
            let permit = authority
                .native_reconciliation(issuer, Some(hold.grant()), hold.incarnation().unwrap())
                .map_err(|cause| {
                    PrivateTerminalDriveRefusal::Common(PrivateAuthorityRefusal::Authority(cause))
                })?;
            fixture
                .owner
                .lock_for_release(&connection)
                .map_err(PrivateTerminalDriveRefusal::Native)?
                .reconcile_pointer(&permit, hold)
                .map_err(PrivateTerminalDriveRefusal::Native)
        })
        .unwrap()
}

#[test]
fn stopped_pointer_requires_original_cleanup_receipt_and_zero_holder_permission() {
    let fixture = Fixture::new();
    install_stopped_lifecycle(&fixture);
    let mut hold = None;
    fixture.press(272, &mut hold);
    let mut hold = hold.unwrap();
    assert!(matches!(
        reconcile_stopped_pointer(&fixture, &mut hold),
        Err(PrivateTerminalDriveRefusal::Common(
            PrivateAuthorityRefusal::Authority(
                sophia_input_authority::RegistrationError::ReleaseBarrier
            )
        ))
    ));
    assert_eq!(fixture.masks().0, 0x100);
    close_stopped_lifecycle(&fixture);
    assert!(reconcile_stopped_pointer(&fixture, &mut hold).unwrap());
    assert_eq!(hold.status(), Status::NativeReconciled);
    assert_eq!(fixture.masks(), (0, 0, 0));
    assert!(hold.proof().unwrap().record_native().is_ok());
    let debt = fixture
        .private
        .controller
        .under_common(|authority| authority.next_debt(&mut 0))
        .unwrap()
        .unwrap();
    assert!(debt.1.native_reconciled);
    assert!(
        !debt.1.recipient_settled,
        "native cleanup fabricates no transport receipt"
    );
}

#[test]
fn stopped_pointer_withheld_cleanup_receipt_retains_physical_debt() {
    let fixture = Fixture::new();
    install_stopped_lifecycle(&fixture);
    let mut hold = None;
    fixture.press(272, &mut hold);
    let mut hold = hold.unwrap();
    // The legacy source really removes this state, but issues no exact
    // activation receipt to the original admission. Later absence cannot help.
    fixture
        .private
        .broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .cleanup_owner(client().raw());
    close_stopped_lifecycle(&fixture);
    assert_eq!(
        reconcile_stopped_pointer(&fixture, &mut hold),
        Err(PrivateTerminalDriveRefusal::Native(
            Refusal::ActivationMismatch
        ))
    );
    assert_eq!(fixture.masks().0, 0x100);
    assert!(hold.proof().is_none());
}

#[test]
fn stopped_query_scope_clone_and_replacement_never_answer_original_scope() {
    let fixture = Fixture::new();
    install_stopped_lifecycle(&fixture);
    let registry = &fixture.private.broker.registry;
    let original = registry
        .input_authority
        .lock()
        .unwrap()
        .ordered_query_scope(namespace())
        .unwrap();
    let mut cloned = registry.input_authority.lock().unwrap().clone();
    cloned.cleanup_ordered_owner(namespace(), client().raw());
    assert!(
        !original.retired(),
        "cloning native state creates independent cleanup history"
    );
    close_stopped_lifecycle(&fixture);
    assert!(original.retired());
    let mut authority = registry.input_authority.lock().unwrap();
    authority.register_query_client(namespace(), client().raw());
    let replacement = authority.ordered_query_scope(namespace()).unwrap();
    assert!(!replacement.retired());
    assert!(
        original.retired(),
        "a replacement does not reset an old receipt"
    );
}

#[test]
fn stopped_cleanup_receipt_rejects_foreign_authority_and_replaced_admission() {
    let fixture = Fixture::new();
    install_stopped_lifecycle(&fixture);
    let mut hold = None;
    fixture.press(272, &mut hold);
    let hold = hold.unwrap();
    close_stopped_lifecycle(&fixture);
    let registry = &fixture.private.broker.registry;
    assert!(
        hold.endpoint()
            .native_cleanup_receipt(&registry.input_authority)
            .is_some()
    );
    let other = Fixture::new();
    assert!(
        hold.endpoint()
            .native_cleanup_receipt(&other.private.broker.registry.input_authority)
            .is_none()
    );
    let mut replacement = hold.endpoint().clone();
    replacement.admission = sophia_protocol::ClientAdmissionId::from_raw(99999);
    assert!(
        replacement
            .native_cleanup_receipt(&registry.input_authority)
            .is_none()
    );
}

#[test]
fn stopped_pointer_cleanup_preserves_replacement_query_and_sibling_activation() {
    let fixture = Fixture::new();
    install_stopped_lifecycle(&fixture);
    let mut hold = None;
    fixture.press(272, &mut hold);
    let mut hold = hold.unwrap();
    close_stopped_lifecycle(&fixture);
    let registry = &fixture.private.broker.registry;
    let replacement = crate::XActiveInputGrab {
        owner: client().raw() + 1,
        ..implicit()
    };
    {
        let mut authority = registry.input_authority.lock().unwrap();
        authority.register_query_client(namespace(), replacement.owner);
        authority.grab_pointer(namespace(), replacement).unwrap();
        authority.observe_query_input(
            namespace(),
            window(),
            XAuthorityInputEvent::Pointer(XAuthorityPointerEvent {
                kind: XAuthorityPointerEventKind::Motion,
                surface: surface(),
                root_x: 90,
                root_y: 91,
                event_x: 40,
                event_y: 41,
                state: 0x204,
                time_msec: 200,
            }),
        );
    }
    assert!(reconcile_stopped_pointer(&fixture, &mut hold).unwrap());
    let authority = registry.input_authority.lock().unwrap();
    assert_eq!(authority.pointer_query_state(namespace()).mask, 0x204);
    assert_eq!(authority.pointer_grab(namespace()), Some(replacement));
    assert!(
        !authority
            .ordered_query_scope(namespace())
            .unwrap()
            .retired()
    );
}

#[test]
fn stopped_donor_scan_retains_exact_dependents_and_progresses_past_foreign_residuals() {
    let mut fixture = Fixture::new();
    install_stopped_lifecycle(&fixture);
    let mut donor = None;
    let mut dependent = None;
    fixture.press(272, &mut donor);
    fixture.press(273, &mut dependent);
    let mut donor = donor.unwrap();
    let dependent = dependent.unwrap();
    close_stopped_lifecycle(&fixture);
    assert!(reconcile_stopped_pointer(&fixture, &mut donor).unwrap());
    assert!(dependent.needs_retirement_from(&donor));
    let foreign = Fixture::new();
    let mut independent = None;
    foreign.press(272, &mut independent);
    let independent = independent.unwrap();
    assert!(
        !independent.needs_retirement_from(&donor),
        "colliding numeric stamps from another native origin are unrelated"
    );
    // Labelled component custody: move these actual source-owned obligations
    // into preallocated terminal slots to exercise the bounded donor scan.
    for hold in [donor, dependent, independent] {
        let incarnation = hold.incarnation().unwrap();
        let grant = hold.grant();
        fixture.private.terminal.holds.push(PrivateHoldRecord {
            incarnation,
            reached: PrivateReachedResources {
                client: hold.client(),
                window: window(),
                surface: Some(surface()),
                namespace: namespace(),
                seat: seat(),
                grant,
            },
            custody: PrivateDeliveryCustody::new(1, None),
            native: Some(PrivateNativeHold::Pointer(hold)),
        });
    }
    let mut cursor = PrivateTerminalDriveCursor::default();
    for _ in 0..4 {
        assert!(!fixture.private.terminal.native_disposal_ready(&mut cursor));
    }
    assert_eq!(
        cursor.recipient, 1,
        "an unresolved dependent keeps its donor but other candidates receive a turn"
    );
    let controller = fixture.private.controller.clone();
    let owner = fixture.owner.clone();
    let hold = fixture.private.terminal.holds[1]
        .native
        .as_mut()
        .unwrap()
        .pointer_mut()
        .unwrap();
    let connection = hold.connection();
    controller
        .under_common_as_origin(|authority, issuer| {
            let permit = authority
                .native_reconciliation(issuer, Some(hold.grant()), hold.incarnation().unwrap())
                .unwrap();
            owner
                .lock_for_release(&connection)
                .unwrap()
                .reconcile_pointer(&permit, hold)
                .unwrap();
        })
        .unwrap();
    cursor.next_recipient(0);
    for _ in 0..3 {
        assert!(!fixture.private.terminal.native_disposal_ready(&mut cursor));
    }
    assert!(
        fixture.private.terminal.native_disposal_ready(&mut cursor),
        "the exact dependent consumed its proof; an unrelated residual does not retain this donor"
    );
}
