    // A genuine receipt is still insufficient for any other residual or
    // incomplete phase. This checks refusal without exposing sealed fields.
    fn assert_other_phase_rejects_receipt(hold: &mut Hold) {
        let donor = Fixture::new();
        let mut retired = None;
        donor.press(274, &mut retired);
        let mut retired = retired.unwrap();
        donor.release(&donor.role, 274, &mut retired);
        let before = hold.status();
        assert!(matches!(
            hold.complete_shared_activation(retired.activation_retirement().unwrap()),
            Err(Refusal::WrongPhase)
        ));
        assert_eq!(hold.status(), before);
        assert!(hold.proof().is_none());
    }

    #[test]
    fn side_buttons_keep_automatic_capture_until_every_button_is_up() {
        let fixture = Fixture::new();
        let mut left = None;
        let mut side = None;
        fixture.press(272, &mut left);
        fixture.press(275, &mut side);
        let mut left = left.unwrap();
        let mut side = side.unwrap();
        fixture.release(&fixture.role, 272, &mut left);
        assert_eq!(fixture.masks(), (0, 0, 0));
        assert_eq!(
            left.status(),
            Status::Retained(Residual::Activation(
                crate::PointerActivationRetirement::StillRequiredByOtherButtons
            ))
        );
        assert!(left.proof().is_none());
        assert!(
            fixture
                .private
                .broker
                .registry
                .input_authority
                .lock()
                .unwrap()
                .pointer_grab(namespace())
                .is_some()
        );
        fixture.release(&fixture.role, 275, &mut side);
        assert_eq!(side.status(), Status::NativeReconciled);
        assert!(
            fixture
                .private
                .broker
                .registry
                .input_authority
                .lock()
                .unwrap()
                .pointer_grab(namespace())
                .is_none()
        );
        // A later release retiring capture is not retroactive proof for the
        // earlier debt; an origin continuation must explicitly revisit it.
        assert!(left.proof().is_none());
        left.complete_shared_activation(side.activation_retirement().unwrap())
            .unwrap();
        assert_eq!(left.status(), Status::NativeReconciled);
        assert!(!left.proof().unwrap().record_native().unwrap());
        assert!(!side.proof().unwrap().record_native().unwrap());
    }

    #[test]
    fn shared_activation_receipt_seals_both_native_debts_including_buttons_eight_and_nine() {
        for (a_button, b_button) in [(272, 273), (275, 276)] {
            let fixture = Fixture::new();
            let (mut a, mut b) = (None, None);
            fixture.press(a_button, &mut a);
            fixture.press(b_button, &mut b);
            let (mut a, mut b) = (a.unwrap(), b.unwrap());
            fixture.release(&fixture.role, a_button, &mut a);
            assert_eq!(
                a.status(),
                Status::Retained(Residual::Activation(
                    crate::PointerActivationRetirement::StillRequiredByOtherButtons
                ))
            );
            assert!(a.activation_retirement().is_none());
            assert!(b.activation_retirement().is_none());
            fixture.release(&fixture.role, b_button, &mut b);
            let receipt = b
                .activation_retirement()
                .expect("actual final native retirement");
            assert!(a.proof().is_none(), "receipt must be visited explicitly");
            a.complete_shared_activation(receipt).unwrap();
            assert_eq!(a.status(), Status::NativeReconciled);
            assert_eq!(a.proof().unwrap().incarnation(), a.incarnation().unwrap());
            assert!(!a.proof().unwrap().record_native().unwrap());
            assert!(!b.proof().unwrap().record_native().unwrap());
            fixture
                .private
                .controller
                .under_common_as_origin(|authority, _| {
                    let mut cursor = 0;
                    let first = authority.next_debt(&mut cursor).unwrap();
                    let second = authority.next_debt(&mut cursor).unwrap();
                    assert_ne!(first.0, second.0);
                    assert!([first.0, second.0].contains(&a.incarnation().unwrap()));
                    assert!([first.0, second.0].contains(&b.incarnation().unwrap()));
                    for (_, bits) in [first, second] {
                        assert!(bits.native_reconciled);
                        assert!(!bits.recipient_settled);
                    }
                })
                .unwrap();
        }
    }

    #[test]
    fn incomplete_release_cannot_be_promoted_by_a_retirement_receipt() {
        let fixture = Fixture::new();
        let mut hold = None;
        fixture.press(272, &mut hold);
        let mut hold = hold.unwrap();
        let before = fixture.masks();
        // API composition control: consume the permit before entering release
        // so the sealed hold retains an incomplete phase, without a source hook.
        let result = fixture.run(&fixture.role, |permit, _| {
            permit.begin_external_effect().map_err(Refusal::Authority)?;
            let connection = hold.connection();
            fixture
                .owner
                .lock_for_release(&connection)?
                .release(
                    permit,
                    &mut hold,
                    &fixture.route(272, false),
                    &Cell::new(false),
                )
                .map(|_| ())
        });
        assert!(matches!(result, Err(Refusal::Authority(_))));
        assert_eq!(hold.status(), Status::ReleaseEntered);
        assert_other_phase_rejects_receipt(&mut hold);
        assert_eq!(fixture.masks(), before);
    }

    #[test]
    fn earlier_or_foreign_activation_receipts_do_not_seal_a_later_debt() {
        let fixture = Fixture::new();
        let foreign = Fixture::new();
        let (mut earlier, mut alien) = (None, None);
        fixture.press(274, &mut earlier);
        foreign.press(274, &mut alien);
        let (mut earlier, mut alien) = (earlier.unwrap(), alien.unwrap());
        fixture.release(&fixture.role, 274, &mut earlier);
        foreign.release(&foreign.role, 274, &mut alien);
        let (mut a, mut b) = (None, None);
        fixture.press(272, &mut a);
        fixture.press(273, &mut b);
        let (mut a, mut b) = (a.unwrap(), b.unwrap());
        assert!(
            matches!(
                a.complete_shared_activation(earlier.activation_retirement().unwrap()),
                Err(Refusal::WrongPhase)
            ),
            "held is not native cleanup evidence"
        );
        fixture.release(&fixture.role, 272, &mut a);
        let before = fixture.masks();
        assert!(matches!(
            a.complete_shared_activation(alien.activation_retirement().unwrap()),
            Err(Refusal::ForeignOrigin)
        ));
        assert!(matches!(
            a.complete_shared_activation(earlier.activation_retirement().unwrap()),
            Err(Refusal::ActivationMismatch)
        ));
        assert!(a.proof().is_none());
        assert_eq!(fixture.masks(), before);
        fixture.release(&fixture.role, 273, &mut b);
        a.complete_shared_activation(b.activation_retirement().unwrap())
            .unwrap();
        assert_eq!(a.status(), Status::NativeReconciled);
    }

    #[test]
    fn visiting_retirement_evidence_never_replays_release_or_clears_new_activation() {
        let fixture = Fixture::new();
        let (mut a, mut b) = (None, None);
        fixture.press(272, &mut a);
        fixture.press(273, &mut b);
        let (mut a, mut b) = (a.unwrap(), b.unwrap());
        fixture.release(&fixture.role, 272, &mut a);
        fixture.release(&fixture.role, 273, &mut b);
        let mut newer = None;
        fixture.press(274, &mut newer);
        let newer = newer.unwrap();
        let masks = fixture.masks();
        let grab = fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .pointer_grab(namespace());
        let receipt = b.activation_retirement().unwrap();
        a.complete_shared_activation(receipt).unwrap();
        assert!(matches!(
            a.complete_shared_activation(receipt),
            Err(Refusal::WrongPhase)
        ));
        let changed = Cell::new(false);
        let replay = fixture.run(&fixture.role, |permit, _| {
            let connection = a.connection();
            fixture
                .owner
                .lock_for_release(&connection)?
                .release(permit, &mut a, &fixture.route(272, false), &changed)
                .map(|_| ())
        });
        assert_eq!(replay, Err(Refusal::WrongPhase));
        assert!(!changed.get());
        a.proof().unwrap().record_native().unwrap();
        assert_eq!(fixture.masks(), masks);
        assert_eq!(
            fixture
                .private
                .broker
                .registry
                .input_authority
                .lock()
                .unwrap()
                .pointer_grab(namespace()),
            grab
        );
        assert_eq!(newer.status(), Status::Held);
    }

    #[test]
    fn retiring_siblings_unproved_selection_remains_separate_from_its_receipt() {
        let fixture = Fixture::new();
        let (mut a, mut b) = (None, None);
        fixture.press(272, &mut a);
        fixture.press(273, &mut b);
        let (mut a, mut b) = (a.unwrap(), b.unwrap());
        fixture.release(&fixture.role, 272, &mut a);
        fixture.selections.lock().unwrap().pointer = None;
        fixture.release(&fixture.role, 273, &mut b);
        assert_eq!(b.status(), Status::Retained(Residual::SelectionUnavailable));
        assert_other_phase_rejects_receipt(&mut b);
        assert!(b.proof().is_none());
        a.complete_shared_activation(b.activation_retirement().unwrap())
            .unwrap();
        assert!(!a.proof().unwrap().record_native().unwrap());
        assert_eq!(b.status(), Status::Retained(Residual::SelectionUnavailable));
        assert!(b.proof().is_none());
        fixture
            .private
            .controller
            .under_common_as_origin(|authority, _| {
                let mut cursor = 0;
                let first = authority.next_debt(&mut cursor).unwrap();
                let second = authority.next_debt(&mut cursor).unwrap();
                assert_ne!(first.0, second.0);
                assert!([first.0, second.0].contains(&a.incarnation().unwrap()));
                assert!([first.0, second.0].contains(&b.incarnation().unwrap()));
                for (incarnation, bits) in [first, second] {
                    assert_eq!(
                        bits.native_reconciled,
                        incarnation == a.incarnation().unwrap()
                    );
                    assert!(!bits.recipient_settled);
                }
            })
            .unwrap();
    }

    #[test]
    fn replacing_the_shared_activation_does_not_mint_or_supply_its_retirement() {
        let fixture = Fixture::new();
        let (mut a, mut b) = (None, None);
        fixture.press(272, &mut a);
        fixture.press(273, &mut b);
        let (mut a, mut b) = (a.unwrap(), b.unwrap());
        fixture.release(&fixture.role, 272, &mut a);
        fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .grab_pointer(namespace(), implicit())
            .unwrap();
        fixture.release(&fixture.role, 273, &mut b);
        assert_eq!(
            b.status(),
            Status::Retained(Residual::Activation(
                crate::PointerActivationRetirement::Replaced
            ))
        );
        assert!(b.activation_retirement().is_none());
        fixture
            .private
            .broker
            .registry
            .input_authority
            .lock()
            .unwrap()
            .ungrab_pointer(namespace(), client().raw());
        let mut newer = None;
        fixture.press(274, &mut newer);
        let mut newer = newer.unwrap();
        fixture.release(&fixture.role, 274, &mut newer);
        let receipt = newer.activation_retirement().unwrap();
        assert!(matches!(
            a.complete_shared_activation(receipt),
            Err(Refusal::ActivationMismatch)
        ));
        assert!(matches!(
            b.complete_shared_activation(receipt),
            Err(Refusal::WrongPhase)
        ));
        assert!(a.proof().is_none());
        assert!(b.proof().is_none());
    }
