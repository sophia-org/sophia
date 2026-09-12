//! What the ledger must never get wrong.
//!
//! Each test here corresponds to a way a real desktop breaks: a key nobody
//! releases, a release that clears someone else's hold, an injector reaching a
//! seat it was never granted.

use sophia_input_authority::{
    AuthorityInstance, Capacity, CapacityError, ExecutionContext, HoldIncarnation, Input,
    InstanceId, IssuerHandle, Origin, RegistrationError, ReleaseOutcome, SeatBinding,
};
use sophia_protocol::{DeviceId, SeatId};

fn authority() -> (AuthorityInstance, IssuerHandle) {
    let binding = SeatBinding::new(InstanceId::new(1), SeatId::from_raw(1));
    let (instance, issuer, _submit) =
        AuthorityInstance::new(binding, Capacity::PLANNED, 9).expect("planned capacity");
    (instance, issuer)
}

fn context(generation: sophia_input_authority::GrantGeneration) -> ExecutionContext {
    ExecutionContext {
        generation,
        epoch: 0,
        publication: 0,
        request: 1,
    }
}

fn recipient(id: u64) -> impl FnOnce() -> HoldIncarnation {
    move || HoldIncarnation {
        recipient: id,
        connection_generation: 1,
        input: Input::Key(30),
        hold: id,
    }
}

#[test]
fn a_physical_hold_survives_the_synthetic_source_that_shared_it_retiring() {
    let (mut authority, issuer) = authority();
    let physical = authority
        .register_physical(&issuer, DeviceId::from_raw(1))
        .expect("physical registers");
    let (grant, generation) = authority.issue_grant(&issuer).expect("grant");
    let synthetic = authority
        .allocate_device(&issuer, grant, generation, DeviceId::from_raw(100))
        .expect("device");

    // The operator is holding the key; an injector then holds it too.
    authority
        .execute_physical_press(&issuer, physical, Input::Key(30), recipient(7))
        .expect("the physical press applies");
    authority
        .execute_press(synthetic, Input::Key(30), context(generation), recipient(7))
        .expect("second source joins the hold");

    // Retiring the injector must leave the other hold intact.
    let debt = authority.retire_source(synthetic.source());
    assert_eq!(
        debt.owed_releases, 0,
        "a survivor remains, so nothing is owed"
    );
    assert_eq!(debt.survivors, 1);
    assert_eq!(
        authority.release(physical, Input::Key(30)),
        ReleaseOutcome::DeliverTo(HoldIncarnation {
            recipient: 7,
            connection_generation: 1,
            input: Input::Key(30),
            hold: 7,
        }),
        "the surviving source still holds it and its release is still owed"
    );
}

#[test]
fn a_capability_from_another_seat_cannot_execute_here() {
    let (mut authority, issuer) = authority();
    let (grant, generation) = authority.issue_grant(&issuer).expect("grant");
    let capability = authority
        .allocate_device(&issuer, grant, generation, DeviceId::from_raw(100))
        .expect("device");

    let other_binding = SeatBinding::new(InstanceId::new(2), SeatId::from_raw(1));
    let (mut other, other_issuer, _) =
        AuthorityInstance::new(other_binding, Capacity::PLANNED, 9).expect("second authority");
    // The other authority must hold a source at the same index, or the lookup
    // fails first and this proves nothing about the binding check. That is how
    // the first version of this test passed with the check removed.
    let (other_grant, other_generation) = other.issue_grant(&other_issuer).expect("grant");
    let native = other
        .allocate_device(
            &other_issuer,
            other_grant,
            other_generation,
            DeviceId::from_raw(100),
        )
        .expect("device");
    assert_eq!(
        native.source(),
        capability.source(),
        "the test needs both authorities to have a source at this index"
    );

    assert_eq!(
        other
            .execute_press(
                capability,
                Input::Key(30),
                context(generation),
                recipient(1)
            )
            .unwrap_err(),
        RegistrationError::ForeignBinding,
        "a capability names the authority it belongs to and cannot be replayed into another"
    );
}

#[test]
fn execution_refuses_while_a_transition_has_not_published() {
    let (mut authority, issuer) = authority();
    let (grant, generation) = authority.issue_grant(&issuer).expect("grant");
    let capability = authority
        .allocate_device(&issuer, grant, generation, DeviceId::from_raw(100))
        .expect("device");

    authority.begin_transition();
    assert_eq!(
        authority
            .execute_press(
                capability,
                Input::Key(30),
                context(generation),
                recipient(1)
            )
            .unwrap_err(),
        RegistrationError::RoutingUnavailable,
        "between commit and publication there is no current target, so it refuses"
    );

    authority.publish(1, 0);
    let context = ExecutionContext {
        generation,
        epoch: 0,
        publication: 1,
        request: 1,
    };
    assert!(
        authority
            .execute_press(capability, Input::Key(30), context, recipient(1))
            .is_ok(),
        "the matching publication re-enables routing"
    );
}

#[test]
fn a_button_domain_that_does_not_match_the_preallocation_is_refused() {
    let binding = SeatBinding::new(InstanceId::new(1), SeatId::from_raw(1));
    let refused = AuthorityInstance::new(binding, Capacity::PLANNED, 12);
    assert!(refused.is_err(), "a wider domain must be refused");
    assert_eq!(
        refused.err().expect("refused"),
        CapacityError::ButtonDomainMismatch {
            advertised: 12,
            expected: 9,
        },
        "the record count is derived from the domain, so a wider domain must not \
         be silently under-allocated"
    );
}

#[test]
fn a_physical_source_is_distinguishable_from_a_synthetic_one() {
    let (mut authority, issuer) = authority();
    let physical = authority
        .register_physical(&issuer, DeviceId::from_raw(1))
        .expect("physical");
    let (grant, generation) = authority.issue_grant(&issuer).expect("grant");
    let synthetic = authority
        .allocate_device(&issuer, grant, generation, DeviceId::from_raw(100))
        .expect("device");

    // Both arms, or the mutant that makes is_physical always true survives.
    assert!(
        authority.is_physical(physical),
        "a registered physical source is physical"
    );
    assert!(
        !authority.is_physical(synthetic.source()),
        "a granted device is not physical, which is what keeps synthetic input \
         out of the emergency recognizer"
    );
    // And the issuer-only physical path must refuse a synthetic source.
    assert_eq!(
        authority
            .execute_physical_press(&issuer, synthetic.source(), Input::Key(31), recipient(9))
            .unwrap_err(),
        RegistrationError::ForeignBinding,
        "a synthetic source cannot enter through the physical path"
    );
    assert_eq!(
        authority.owner_of(physical),
        None,
        "physical answers to no grant"
    );
    assert_eq!(authority.owner_of(synthetic.source()), Some(grant));
    let _ = Origin::Physical;
}
