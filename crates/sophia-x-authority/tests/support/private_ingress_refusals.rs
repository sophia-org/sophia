// The private producer admission's refusal fan-out at the ingress, for the
// arms nothing had witnessed: a delivery identity already live, a request
// counter that cannot advance, the order's exhaustion latch, an authority
// nobody can read, and an admission that has been revoked. Every refusal
// hands the work back whole, rolls back only the reservation it took, and
// leaves what the order accepted before it in place.

/// A frontend over a store this control can read the credit of, with the
/// named clients admitted to the boundary.
#[cfg(unix)]
struct IngressRefusalFixture {
    durable: crate::PrivateSettlementOwner,
    keeper: crate::PrivateServiceOwner,
    private: crate::PrivateXServerFrontend,
    _registrations: Vec<XServerFrontendClientRouteRegistration>,
}

#[cfg(unix)]
impl IngressRefusalFixture {
    fn admitting(clients: &[u64]) -> Self {
        let durable = crate::PrivateSettlementOwner::default();
        let keeper = service_owner(&durable, 16);
        let private = private_for_roles(&keeper);
        let registrations = clients
            .iter()
            .map(|client| admit_role_client(&private, XServerFrontendClientId(*client)))
            .collect();
        Self {
            durable,
            keeper,
            private,
            _registrations: registrations,
        }
    }


    fn route(client: u64, delivery: u64) -> XAuthorityRoutedInput {
        motion_to(
            SurfaceId::new(u32::try_from(client).expect("a client id that is a surface owner"), 1),
            XAuthorityInputDeliveryId::from_raw(delivery),
        )
    }

    /// The completion the recovery ledger holds for a delivery, if it is
    /// tracked at all.
    fn tracked(&self, delivery: u64) -> Option<Arc<PrivateDeliveryCompletion>> {
        self.private
            .broker
            .registry
            .input_recovery
            .completion_for(XAuthorityInputDeliveryId::from_raw(delivery))
            .expect("a readable ledger")
    }

    /// What the store has reserved: one credit per item the order accepted.
    fn credit(&self) -> Option<usize> {
        self.durable.reserved()
    }

}

/// A producer for an admitted client, from the frontend directly so that a
/// lease on the fixture's keeper can be held across the call.
#[cfg(unix)]
fn ingress(private: &mut crate::PrivateXServerFrontend, client: u64, device: u64) -> PrivateIngress {
    private
        .ingress_for(XServerFrontendClientId(client), DeviceId::from_raw(device))
        .expect("a capability for an admitted client")
}

/// Drain the order through the real service visits; how many items ran.
#[cfg(unix)]
fn drained(private: &mut crate::PrivateXServerFrontend) -> usize {
    let mut keyboards = private.keyboards().expect("this instance's state");
    let mut drained = 0;
    loop {
        let ran = private
            .route_pending_ordered(&mut keyboards, &control_watchdog())
            .expect("a readable order")
            .len();
        if ran == 0 {
            return drained;
        }
        drained += ran;
    }
}

/// The refused route, its delivery checked to be the one submitted.
#[cfg(unix)]
fn handed_back(route: XAuthorityRoutedInput, delivery: u64) {
    assert_eq!(
        route.delivery,
        Some(XAuthorityInputDeliveryId::from_raw(delivery)),
        "the refused work is handed back intact"
    );
}

#[cfg(unix)]
#[test]
fn a_delivery_identity_already_live_is_refused_at_admission_and_the_live_one_is_untouched() {
    let mut fixture = IngressRefusalFixture::admitting(&[701, 702]);
    let first = ingress(&mut fixture.private, 701, 1);
    let second = ingress(&mut fixture.private, 702, 2);
    let lease = fixture.keeper.lease();
    first
        .submit(&lease, IngressRefusalFixture::route(701, 7010))
        .expect("the order accepts the first");
    let live = fixture.tracked(7010).expect("the accepted delivery is tracked");
    let credit = fixture.credit();
    assert_eq!(credit, Some(1), "one accepted item, one credit");

    // THE SAME IDENTITY FROM ANOTHER GRANT. Its own cell is free and the
    // order has room, so the only thing that can refuse it is the identity
    // being live -- and that is what it is refused as, not saturation and
    // not denial.
    let refused = second.submit(&lease, IngressRefusalFixture::route(702, 7010));
    let Err(crate::PrivateSendError::DeliveryAlreadyTracked(route)) = refused else {
        panic!("a live delivery identity is refused as itself, got {refused:?}");
    };
    handed_back(route, 7010);
    assert!(
        Arc::ptr_eq(&live, &fixture.tracked(7010).expect("still tracked")),
        "the live delivery keeps the cell its own admission minted"
    );
    assert_eq!(
        fixture.credit(),
        credit,
        "the refusal took no credit and released none"
    );

    // The refused grant is free: a different identity from it is accepted,
    // and the order holds exactly the two it accepted.
    second
        .submit(&lease, IngressRefusalFixture::route(702, 7011))
        .expect("the same grant with an identity of its own");
    assert_eq!(drained(&mut fixture.private), 2, "both accepted deliveries ran, and nothing else did");
}

#[cfg(unix)]
#[test]
fn an_ingress_whose_request_numbers_are_spent_refuses_exhausted_and_stays_exhausted() {
    let mut fixture = IngressRefusalFixture::admitting(&[711, 712, 713]);
    let spent = ingress(&mut fixture.private, 711, 1);
    let earlier = ingress(&mut fixture.private, 712, 2);
    let unaffected = ingress(&mut fixture.private, 713, 3);
    let lease = fixture.keeper.lease();
    earlier
        .submit(&lease, IngressRefusalFixture::route(712, 7120))
        .expect("work accepted before the exhaustion");
    let credit = fixture.credit();
    assert_eq!(credit, Some(1));

    // THE COUNTER AT ITS END. Its next number does not exist, and the
    // refusal says so as itself: not busy, which is worth retrying, and not
    // a decision on the terms of the request.
    spent.requests.store(u64::MAX, Ordering::Relaxed);
    let refused = spent.submit(&lease, IngressRefusalFixture::route(711, 7110));
    let Err(crate::PrivateSendError::Exhausted(route)) = refused else {
        panic!("a spent request counter is exhaustion, got {refused:?}");
    };
    handed_back(route, 7110);
    assert!(
        fixture.tracked(7110).is_none(),
        "the delivery reservation the send took was rolled back"
    );
    assert_eq!(fixture.credit(), credit, "and no credit was taken or released");

    // TERMINAL. The same answer again: a number that does not exist is not
    // created by asking twice, and nothing resets the counter.
    let again = spent.submit(&lease, IngressRefusalFixture::route(711, 7111));
    assert!(
        matches!(again, Err(crate::PrivateSendError::Exhausted(_))),
        "exhaustion is not retried into acceptance, got {again:?}"
    );
    assert!(fixture.tracked(7111).is_none());

    // ONE PRODUCER'S EXHAUSTION IS ITS OWN. Another grant is accepted, and
    // what was accepted before is still the order's.
    unaffected
        .submit(&lease, IngressRefusalFixture::route(713, 7130))
        .expect("another producer's numbers are its own");
    assert_eq!(drained(&mut fixture.private), 2, "the earlier item and the unaffected one ran");
}

#[cfg(unix)]
#[test]
fn an_order_that_exhausted_its_positions_refuses_every_later_submission_and_keeps_what_it_accepted() {
    let mut fixture = IngressRefusalFixture::admitting(&[721, 722]);
    let before = ingress(&mut fixture.private, 721, 1);
    let after = ingress(&mut fixture.private, 722, 2);
    let lease = fixture.keeper.lease();
    before
        .submit(&lease, IngressRefusalFixture::route(721, 7210))
        .expect("accepted before the order ran out of positions");
    let credit = fixture.credit();
    assert_eq!(credit, Some(1));

    // THE LATCH, SET AS THE ORDER SETS IT when a position cannot be named.
    // The stream's own counter cannot be driven to its end from here, so
    // what this witnesses is the latch's contract: once set it refuses as
    // itself, keeps what was accepted, and is never reset.
    fixture.private.admission.exhausted.store(true, Ordering::Release);
    let refused = after.submit(&lease, IngressRefusalFixture::route(722, 7220));
    let Err(crate::PrivateSendError::Exhausted(route)) = refused else {
        panic!("an exhausted order refuses as exhausted, got {refused:?}");
    };
    handed_back(route, 7220);
    assert!(
        fixture.tracked(7220).is_none(),
        "the refused send's delivery reservation was rolled back"
    );
    assert_eq!(fixture.credit(), credit, "and no credit moved");
    assert_eq!(
        drained(&mut fixture.private),
        1,
        "what was accepted before exhaustion is still the order's"
    );

    // Draining did not reset it, and neither does anything else.
    let still = after.submit(&lease, IngressRefusalFixture::route(722, 7221));
    assert!(
        matches!(still, Err(crate::PrivateSendError::Exhausted(_))),
        "exhaustion is never reset, got {still:?}"
    );
}

#[cfg(unix)]
#[test]
fn an_authority_nobody_can_read_refuses_a_submission_as_unavailable_not_denied() {
    let mut fixture = IngressRefusalFixture::admitting(&[731]);
    let ingress = ingress(&mut fixture.private, 731, 1);
    let lease = fixture.keeper.lease();
    let credit = fixture.credit();

    // COMMON POISONED. Reserving asks the authority under common; an
    // authority nobody can read established nothing, and the refusal must
    // say that rather than report a decision that was never made.
    let common = Arc::clone(&fixture.private.authority().common);
    assert!(
        std::thread::spawn(move || {
            let _guard = common.lock().unwrap();
            panic!("poisoning common");
        })
        .join()
        .is_err()
    );
    let refused = ingress.submit(&lease, IngressRefusalFixture::route(731, 7310));
    let Err(crate::PrivateSendError::Unavailable(route)) = refused else {
        panic!("an unreadable authority is unavailable, not a denial, got {refused:?}");
    };
    handed_back(route, 7310);
    assert!(
        fixture.tracked(7310).is_none(),
        "the delivery reservation the send took was rolled back"
    );
    assert_eq!(fixture.credit(), credit, "and no credit was taken or released");
}

#[cfg(unix)]
#[test]
fn a_revoked_admission_is_refused_as_a_denial_at_submission_and_the_work_comes_back() {
    let mut fixture = IngressRefusalFixture::admitting(&[741]);
    let ingress = ingress(&mut fixture.private, 741, 1);
    let lease = fixture.keeper.lease();
    let credit = fixture.credit();

    // REVOKED, THEN ASKED. The grant this ingress reserves against was
    // retired with its admission, so the authority refuses on the terms of
    // the request: a decision, which is a denial, and not something busy or
    // unreadable that a producer should wait on.
    let retired = fixture
        .private
        .admission_participant()
        .revoke_admission(
            XServerFrontendClientId(741),
            sophia_protocol::ClientAdmissionId::from_raw(741),
        )
        .expect("the boundary to revoke");
    assert_eq!(
        retired,
        crate::PrivateRevocation {
            closed: 1,
            retired: 1
        }
    );
    let refused = ingress.submit(&lease, IngressRefusalFixture::route(741, 7410));
    let Err(crate::PrivateSendError::Denied(route)) = refused else {
        panic!("a revoked admission is a denial, got {refused:?}");
    };
    handed_back(route, 7410);
    assert!(
        fixture.tracked(7410).is_none(),
        "the delivery reservation the send took was rolled back"
    );
    assert_eq!(fixture.credit(), credit, "and no credit was taken or released");
}
