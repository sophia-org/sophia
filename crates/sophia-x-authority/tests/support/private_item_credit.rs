// Prepared private runner controls using the actual admission store and
// common completion. Native/render facts are supplied by the shared fixture.

#[test]
fn completed_ordered_input_returns_its_storage_credit_after_item_disposal() {
    let durable = PrivateSettlementOwner::with_capacity(4);
    let mut fixture = prepared_ordered_fixture_with_store(
        XServerFrontendClientId::from_raw(9680),
        durable.clone(),
    );
    for offset in 0..12 {
        let route = button_to(
            fixture.surface,
            XAuthorityInputDeliveryId::from_raw(99800 + offset),
            272,
            false,
        );
        fixture.ingress.submit(&fixture.keeper.lease(), route).unwrap();
        assert_eq!(durable.reserved(), Some(1));
        let PrivatePreparedRunner { frontend, keyboards, watch, .. } = &mut fixture.runner;
        let private = frontend.as_mut().unwrap();
        assert!(matches!(
            private.step_once(keyboards, &mut |_, _| Ok(()), watch.as_ref().unwrap()).unwrap(),
            PrivateOrderedStep::Decided(_)
        ));
        assert_eq!(durable.reserved(), Some(1), "execution does not dispose the item");
        assert!(matches!(
            private.deliver_one(&mut |_, _| Ok(())).unwrap(),
            PrivateDeliveryStep::Advanced { report: Some(PrivateDelivered { completion: Some(_), .. }), .. }
        ));
        assert!(private.terminal.turn.is_empty() && private.terminal.delivering.is_empty());
        assert_eq!(durable.reserved(), Some(0), "the exact disposed item returns one storage credit");
    }
}
