fn encoded(
    emission: &super::super::PrivateOrderedEmission,
    order: XByteOrder,
    sequence: u16,
) -> Vec<Vec<u8>> {
    (0..emission.frame_count())
        .map(|index| {
            emission
                .encode_frame(index, order, sequence)
                .unwrap()
                .as_bytes()
                .to_vec()
        })
        .collect()
}

#[test]
fn emission_keeps_source_identity_and_bytes_after_selection_and_geometry_change() {
    let fixture = Fixture::new();
    let mut route = fixture.route(272, true);
    route.delivery = Some(XAuthorityInputDeliveryId::from_raw(773));
    let mut hold = None;
    fixture
        .press_route(&fixture.role, &route, &mut hold)
        .unwrap();
    let mut hold = hold.unwrap();
    let emission = hold.take_press_emission().unwrap();
    assert!(hold.take_press_emission().is_none());
    assert_eq!(emission.delivery(), route.delivery);
    assert_eq!(emission.incarnation(), hold.incarnation().unwrap());
    assert_eq!(emission.connection(), fixture.role.connection());
    assert!(emission.answers_for(&fixture.private.broker.registry));
    assert!(!emission.answers_for(&Fixture::new().private.broker.registry));
    let before = encoded(&emission, XByteOrder::LittleEndian, 19);
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].len(), 32);
    assert_eq!(before[0][0], 4);
    assert_eq!(
        &before[0][12..16],
        &(window().local.raw() as u32).to_le_bytes()
    );
    assert_eq!(&before[0][24..28], &[20, 0, 30, 0]);
    {
        let mut selections = fixture.selections.lock().unwrap();
        selections.configure_geometry(window(), Some(900), Some(800), None, None);
        selections.update(window(), Some(0), None);
    }
    assert_eq!(encoded(&emission, XByteOrder::LittleEndian, 19), before);
    assert!(
        emission
            .encode_frame(1, XByteOrder::LittleEndian, 19)
            .is_none()
    );
    let big = encoded(&emission, XByteOrder::BigEndian, 0x1234);
    assert_eq!(&big[0][2..4], &[0x12, 0x34]);
    assert_eq!(&big[0][24..28], &[0, 20, 0, 30]);
}

#[test]
fn release_emission_uses_each_retained_targets_current_coordinates_and_its_own_delivery() {
    let fixture = Fixture::new();
    let root = XResourceId::new(u64::from(X_SETUP_DEFAULT_ROOT), 1);
    {
        let mut selections = fixture.selections.lock().unwrap();
        selections.update(
            window(),
            Some((1 << 2) | (1 << 3) | (1 << 4) | (1 << 5)),
            None,
        );
    }
    fixture
        .private
        .broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .select_xi_events(
            namespace(),
            client().raw(),
            root,
            &[(
                crate::X_INPUT_POINTER_SOURCE_ID,
                vec![(1 << 4) | (1 << 5) | (1 << 7), 1],
            )],
        );
    let mut hold = None;
    let mut press = fixture.route(272, true);
    press.delivery = Some(XAuthorityInputDeliveryId::from_raw(900));
    fixture
        .press_route(&fixture.role, &press, &mut hold)
        .unwrap();
    let mut hold = hold.unwrap();
    let press_emission = hold.take_press_emission().unwrap();
    assert!(
        press_emission.frame_count() > 2,
        "source and core enter records exist"
    );
    {
        let mut selections = fixture.selections.lock().unwrap();
        selections.configure_geometry(window(), Some(50), Some(60), None, None);
        selections.update(window(), Some(0), None);
    }
    fixture
        .private
        .broker
        .registry
        .input_authority
        .lock()
        .unwrap()
        .select_xi_events(
            namespace(),
            client().raw(),
            root,
            &[(crate::X_INPUT_POINTER_SOURCE_ID, vec![0, 0])],
        );
    let mut release = fixture.route(272, false);
    release.delivery = Some(XAuthorityInputDeliveryId::from_raw(901));
    release.request.global_position = Point { x: 90.0, y: 100.0 };
    release.request.local_position = Point {
        x: -777.0,
        y: -888.0,
    };
    release.request.time_msec = 444;
    let (outcome, event) = fixture
        .run(&fixture.role, |permit, _| {
            let connection = hold.connection();
            fixture.owner.lock_for_release(&connection)?.release(
                permit,
                &mut hold,
                &release,
                &Cell::new(false),
            )
        })
        .unwrap();
    assert_eq!(
        outcome,
        ReleaseOutcome::DeliverTo(hold.incarnation().unwrap())
    );
    assert!(event.unwrap().is_some());
    let emission = hold.take_release_emission().unwrap();
    assert!(hold.take_release_emission().is_none());
    assert_eq!(emission.delivery(), release.delivery);
    assert_eq!(emission.incarnation(), press_emission.incarnation());
    let frames = encoded(&emission, XByteOrder::LittleEndian, 7);
    assert_eq!(frames.len(), 2, "no press crossings replayed");
    let xi = &frames[0];
    assert_eq!(xi[0], 35);
    assert_eq!(&xi[8..10], &5u16.to_le_bytes());
    assert_eq!(&xi[10..12], &crate::X_INPUT_POINTER_SOURCE_ID.to_le_bytes());
    assert_eq!(&xi[24..28], &(root.local.raw() as u32).to_le_bytes());
    assert_eq!(&xi[40..44], &(90u32 << 16).to_le_bytes());
    assert_eq!(&xi[44..48], &(100u32 << 16).to_le_bytes());
    let core = &frames[1];
    assert_eq!(core[0], 5);
    assert_eq!(&core[12..16], &(window().local.raw() as u32).to_le_bytes());
    assert_eq!(&core[24..28], &[40, 0, 40, 0]);
    assert_eq!(&core[28..30], &0x100u16.to_le_bytes());
    assert_eq!(&core[4..8], &444u32.to_le_bytes());
    // Formatting a release is neither its transport receipt nor a native bit.
    fixture
        .private
        .controller
        .under_common_as_origin(|authority, _| {
            let (_, bits) = authority.next_debt(&mut 0).unwrap();
            assert!(!bits.recipient_settled);
            assert!(!bits.native_reconciled);
        })
        .unwrap();
}

#[test]
fn vanished_emission_target_retains_the_real_release_debt_and_does_not_reuse_press_bytes() {
    let fixture = Fixture::new();
    let mut hold = None;
    fixture.press(272, &mut hold);
    let mut hold = hold.unwrap();
    fixture.selections.lock().unwrap().remove(window());
    let (outcome, event) = fixture
        .run(&fixture.role, |permit, _| {
            let connection = hold.connection();
            fixture.owner.lock_for_release(&connection)?.release(
                permit,
                &mut hold,
                &fixture.route(272, false),
                &Cell::new(false),
            )
        })
        .unwrap();
    assert_eq!(
        outcome,
        ReleaseOutcome::DeliverTo(hold.incarnation().unwrap())
    );
    assert_eq!(event, Err(PrivateAppliedRefusal::HierarchyMissing));
    assert!(hold.take_release_emission().is_none());
    assert!(
        hold.take_press_emission().is_some(),
        "separate source slot not overwritten"
    );
    assert_eq!(
        fixture
            .private
            .broker
            .registry
            .pointer_state
            .lock()
            .unwrap()[&(namespace(), seat())]
            .state(),
        0
    );
    fixture
        .private
        .controller
        .under_common_as_origin(|authority, _| {
            assert_eq!(
                authority.next_debt(&mut 0).unwrap().0,
                hold.incarnation().unwrap()
            );
        })
        .unwrap();
}

#[test]
fn joined_press_and_survivor_do_not_create_another_emission() {
    let fixture = Fixture::new();
    let other = fixture
        .private
        .reservation_role(client(), DeviceId::from_raw(2))
        .unwrap();
    let mut hold = None;
    fixture.press(272, &mut hold);
    let mut hold = hold.unwrap();
    let _owned = hold.take_press_emission().unwrap();
    let applied = fixture
        .run(&other, |permit, _| {
            let connection = hold.connection();
            fixture
                .owner
                .lock_for_release(&connection)?
                .join(permit, &hold, &Cell::new(false))
        })
        .unwrap();
    assert!(!applied.first_press());
    assert!(hold.take_press_emission().is_none());
    let (outcome, event) = fixture.release(&fixture.role, 272, &mut hold);
    assert!(matches!(outcome, ReleaseOutcome::SurvivorRemains));
    assert!(event.is_none());
    assert!(hold.take_release_emission().is_none());
    fixture.release(&other, 272, &mut hold);
    assert!(hold.take_release_emission().is_some());
}

// A source-level fixture for the writer ownership controls. This builds a real
// common/native press but does not claim PrivateIngress or terminal delivery.
/// Two emissions from ONE registration.
///
/// A fixture per capsule gives two registrations whose numbers agree and whose
/// endpoints do not, which is exactly what an endpoint check refuses. A
/// control about one connection being served twice has to press twice on the
/// same connection.
pub(super) fn emissions_for_one_writer_fixture(
    first: u64,
    second: u64,
) -> (
    super::super::PrivateOrderedEmission,
    super::super::PrivateOrderedEmission,
    super::super::PrivateEndpointIdentity,
) {
    let fixture = Fixture::new();
    let take = |delivery: u64, button: u32| {
        let mut route = fixture.route(button, true);
        route.delivery = Some(XAuthorityInputDeliveryId::from_raw(delivery));
        let mut hold = None;
        fixture
            .press_route(&fixture.role, &route, &mut hold)
            .unwrap();
        hold.as_mut().unwrap().take_press_emission().unwrap()
    };
    let one = take(first, 272);
    let two = take(second, 273);
    let endpoint = fixture
        .private
        .endpoint_for(&fixture._registration)
        .expect("the fixture's own registration");
    (one, two, endpoint)
}

/// One emission, and the endpoint taken from the registration that produced
/// it -- not from the emission.
///
/// A writer's expectation has to come from its own registration. Taking it
/// from the capsule would let the thing being checked supply the answer.
pub(super) fn emission_and_endpoint_for_writer_fixture(
    delivery: u64,
) -> (
    super::super::PrivateOrderedEmission,
    super::super::PrivateEndpointIdentity,
) {
    let fixture = Fixture::new();
    let mut route = fixture.route(272, true);
    route.delivery = Some(XAuthorityInputDeliveryId::from_raw(delivery));
    let mut hold = None;
    fixture
        .press_route(&fixture.role, &route, &mut hold)
        .unwrap();
    let emission = hold.as_mut().unwrap().take_press_emission().unwrap();
    let endpoint = fixture
        .private
        .endpoint_for(&fixture._registration)
        .expect("the fixture's own registration");
    (emission, endpoint)
}

pub(super) fn emission_for_writer_fixture(delivery: u64) -> super::super::PrivateOrderedEmission {
    let fixture = Fixture::new();
    let mut route = fixture.route(272, true);
    route.delivery = Some(XAuthorityInputDeliveryId::from_raw(delivery));
    let mut hold = None;
    fixture
        .press_route(&fixture.role, &route, &mut hold)
        .unwrap();
    hold.as_mut().unwrap().take_press_emission().unwrap()
}

#[test]
fn capsule_assembly_derives_every_identity_and_returns_missing_delivery_whole() {
    let source = emission_for_writer_fixture(891);
    let incarnation = source.incarnation();
    let connection = source.connection();
    let before = encoded(&source, XByteOrder::LittleEndian, 2);
    let capsule = crate::routing_types::XAuthorityOrderedDelivery::from_emission(source).unwrap();
    assert_eq!(capsule.client().raw(), connection.recipient);
    assert_eq!(capsule.recipient(), connection);
    assert_eq!(capsule.incarnation(), incarnation);
    assert_eq!(capsule.delivery(), XAuthorityInputDeliveryId::from_raw(891));
    assert_eq!(
        encoded(capsule.emission(), XByteOrder::LittleEndian, 2),
        before
    );
    let fixture = Fixture::new();
    let mut hold = None;
    fixture.press(272, &mut hold);
    let mut hold = hold.unwrap();
    let source = hold.take_press_emission().unwrap();
    let before = encoded(&source, XByteOrder::LittleEndian, 3);
    let (cause, retained) =
        crate::routing_types::XAuthorityOrderedDelivery::from_emission(source).unwrap_err();
    assert_eq!(
        cause,
        crate::routing_types::XAuthorityOrderedAssemblyRefusal::DeliveryMissing
    );
    assert_eq!(retained.incarnation(), hold.incarnation().unwrap());
    assert!(retained.answers_for(&fixture.private.broker.registry));
    assert_eq!(encoded(&retained, XByteOrder::LittleEndian, 3), before);
}

#[test]
fn a_resource_that_cannot_be_encoded_refuses_before_any_native_or_common_hold() {
    let fixture = Fixture::new();
    let invalid = XResourceId::new(u64::from(u32::MAX) + 1, 1);
    {
        let mut selected = fixture.selections.lock().unwrap();
        selected.register(
            invalid,
            window(),
            Rect {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
        );
        selected.observe_mapped(invalid);
        selected.update(invalid, Some(1 << 2), None);
    }
    let mut hold = None;
    assert_eq!(
        fixture.press_route(&fixture.role, &fixture.route(272, true), &mut hold),
        Err(Refusal::Resolution(
            PrivateAppliedRefusal::WireResourceOverflow
        ))
    );
    assert!(hold.is_none());
    assert_eq!(
        fixture
            .private
            .broker
            .registry
            .pointer_state
            .lock()
            .unwrap()[&(namespace(), seat())]
            .state(),
        0
    );
    fixture.selections.lock().unwrap().remove(invalid);
    fixture.press(272, &mut hold);
    assert!(hold.as_mut().unwrap().take_press_emission().is_some());
}

#[test]
fn emission_keeps_its_origin_and_exact_connection_after_the_source_owner_leaves() {
    let fixture = Fixture::new();
    let origin = Arc::downgrade(&fixture.private.broker.registry.clients);
    let common = Arc::downgrade(&fixture.private.controller.common);
    let selections = Arc::downgrade(&fixture.selections);
    let mut hold = None;
    fixture.press(272, &mut hold);
    let mut hold = hold.unwrap();
    let emission = hold.take_press_emission().unwrap();
    let bytes = encoded(&emission, XByteOrder::LittleEndian, 12);
    drop(hold);
    drop(fixture);
    assert!(origin.upgrade().is_some());
    assert!(common.upgrade().is_some());
    assert!(selections.upgrade().is_some());
    assert_eq!(encoded(&emission, XByteOrder::LittleEndian, 12), bytes);
    drop(emission);
    assert!(origin.upgrade().is_none());
    assert!(common.upgrade().is_none());
    assert!(selections.upgrade().is_none());
}
