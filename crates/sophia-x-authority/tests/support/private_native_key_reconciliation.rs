#[test]
fn stopped_modifier_reconciles_original_xkb_history_without_recipient_proof() {
    let mut fixture = KeyFixture::new();
    install_stopped_lifecycle(&fixture.base);
    let mut hold = None;
    fixture.press(42, 70001, &mut hold).unwrap();
    let mut hold = hold.unwrap();
    assert_eq!(fixture.keyboards.modifiers(seat()), Some(1));
    close_stopped_lifecycle(&fixture.base);
    let connection = hold.connection();
    let incarnation = hold.incarnation().unwrap();
    fixture
        .base
        .private
        .controller
        .under_common_as_origin(|authority, issuer| {
            let permit = authority
                .native_reconciliation(issuer, Some(hold.grant()), incarnation)
                .unwrap();
            fixture
                .base
                .owner
                .lock_for_release(&connection)
                .unwrap()
                .reconcile_key(&permit, &mut hold, &mut fixture.keyboards)
                .unwrap();
        })
        .unwrap();
    assert_eq!(fixture.keyboards.modifiers(seat()), Some(0));
    assert_eq!(hold.status(), Status::NativeReconciled);
    assert!(hold.proof().unwrap().record_native().is_ok());
    let debt = fixture
        .base
        .private
        .controller
        .under_common(|authority| authority.next_debt(&mut 0))
        .unwrap()
        .unwrap();
    assert_eq!(debt.0, incarnation);
    assert!(debt.1.native_reconciled);
    assert!(!debt.1.recipient_settled);
}
