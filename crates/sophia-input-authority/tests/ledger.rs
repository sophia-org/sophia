//! What the ledger must never get wrong.
//!
//! Each test is a way a real desktop breaks: a key nobody releases, a release
//! that clears someone else's hold, an injector reaching a seat it was never
//! granted, or two different inputs quietly sharing one record.

use sophia_input_authority::{
    AuthorityInstance, Capacity, CapacityError, DeviceCapability, ExecutionContext,
    GrantGeneration, HoldIncarnation, Input, InputError, InstanceId, IssuerHandle,
    RegistrationError, ReleaseOutcome, SeatBinding, SubmitHandle,
};
use sophia_protocol::{DeviceId, SeatId};

struct Fixture {
    authority: AuthorityInstance,
    issuer: IssuerHandle,
    submit: SubmitHandle,
}

fn fixture() -> Fixture {
    fixture_for(InstanceId::new(1))
}

fn fixture_for(instance: InstanceId) -> Fixture {
    let binding = SeatBinding::new(instance, SeatId::from_raw(1));
    let (authority, issuer, submit) =
        AuthorityInstance::new(binding, Capacity::PLANNED, 9).expect("planned capacity");
    Fixture {
        authority,
        issuer,
        submit,
    }
}

fn context(generation: GrantGeneration) -> ExecutionContext {
    ExecutionContext {
        generation,
        epoch: 0,
        publication: 0,
        request: 1,
    }
}

fn to(id: u64) -> impl FnOnce() -> HoldIncarnation {
    move || HoldIncarnation {
        recipient: id,
        connection_generation: 1,
        input: Input::key(30).expect("valid keycode"),
        hold: id,
    }
}

fn granted(f: &mut Fixture, device: u64) -> (DeviceCapability, GrantGeneration) {
    let (grant, generation) = f.authority.issue_grant(&f.issuer).expect("grant");
    let capability = f
        .authority
        .allocate_device(&f.issuer, grant, generation, DeviceId::from_raw(device))
        .expect("device");
    (capability, generation)
}

#[test]
fn keys_and_buttons_never_share_a_record() {
    // X keycodes run to 255 and buttons from 1, so an unnormalized scheme puts
    // Key(249) and Button(1) in the same slot and one release clears both.
    let high_key = Input::key(249).expect("249 is a keycode");
    let first_button = Input::button(1, 9).expect("1 is a button");
    assert_ne!(
        high_key, first_button,
        "a high keycode and a low button must not collide"
    );

    let mut f = fixture();
    let (capability, generation) = granted(&mut f, 100);
    f.authority
        .execute_press(&f.submit, capability, high_key, context(generation), to(1))
        .expect("the key presses");
    assert_eq!(
        f.authority
            .release(&f.submit, capability, first_button)
            .expect("released"),
        ReleaseOutcome::NotHeld,
        "releasing a button must not clear a key that merely shares a number"
    );
}

#[test]
fn a_keycode_below_the_minimum_is_not_a_key() {
    assert_eq!(
        Input::key(7).unwrap_err(),
        InputError::KeycodeBelowMinimum(7),
        "X keycodes start at 8; accepting lower ones widens the domain silently"
    );
    assert_eq!(
        Input::button(10, 9).unwrap_err(),
        InputError::ButtonOutsideDomain {
            button: 10,
            domain: 9,
        }
    );
}

#[test]
fn a_physical_source_cannot_alias_a_synthetic_one() {
    // Sixteen grants of two devices fill the synthetic table exactly, so the
    // first physical source is the one that would alias injector zero under a
    // scheme that wraps.
    let mut f = fixture();
    let mut first_synthetic = None;
    for device in 0..16u64 {
        let (capability, _) = granted(&mut f, 200 + device);
        if device == 0 {
            first_synthetic = Some(capability);
        }
    }
    let first_synthetic = first_synthetic.expect("allocated");
    let physical = f
        .authority
        .register_physical(&f.issuer, DeviceId::from_raw(1))
        .expect("physical registers");

    let key = Input::key(30).expect("keycode");
    f.authority
        .execute_physical_press(&f.issuer, physical, key, to(5))
        .expect("the physical press applies");
    assert_eq!(
        f.authority
            .release(&f.submit, first_synthetic, key)
            .expect("released"),
        ReleaseOutcome::NotHeld,
        "an injector releasing must not clear a physical hold it never shared"
    );
}

#[test]
fn a_retired_grant_cannot_press_again() {
    let mut f = fixture();
    let (capability, generation) = granted(&mut f, 100);
    let key = Input::key(30).expect("keycode");
    f.authority
        .execute_press(&f.submit, capability, key, context(generation), to(1))
        .expect("presses while granted");

    let grant = f.authority.owner_of(capability.source()).expect("owned");
    f.authority.revoke_grant(&f.issuer, grant).expect("revoked");

    assert_eq!(
        f.authority
            .execute_press(&f.submit, capability, key, context(generation), to(1))
            .unwrap_err(),
        RegistrationError::StaleGeneration,
        "the capability must stop validating the moment its grant is revoked"
    );
}

#[test]
fn a_new_hold_waits_behind_an_unsettled_release() {
    let mut f = fixture();
    let (first, generation) = granted(&mut f, 100);
    let key = Input::key(30).expect("keycode");
    f.authority
        .execute_press(&f.submit, first, key, context(generation), to(11))
        .expect("held");
    let outcome = f
        .authority
        .release(&f.submit, first, key)
        .expect("released");
    let ReleaseOutcome::DeliverTo(owed) = outcome else {
        panic!("the last holder's release is owed: {outcome:?}");
    };

    let (second, second_generation) = granted(&mut f, 101);
    assert_eq!(
        f.authority
            .execute_press(&f.submit, second, key, context(second_generation), to(22))
            .unwrap_err(),
        RegistrationError::ReleaseBarrier,
        "a new hold must not take this input while the old release is unsettled"
    );

    // A late completion for a hold that is not this one settles nothing.
    let stale = HoldIncarnation { hold: 999, ..owed };
    assert!(
        !f.authority
            .settle(&f.issuer, key, stale, both_bits())
            .expect("settle runs"),
        "a completion naming another hold must not clear this barrier"
    );
    assert!(
        f.authority
            .settle(&f.issuer, key, owed, both_bits())
            .expect("settle runs"),
        "the matching completion clears it"
    );
    assert!(
        f.authority
            .execute_press(&f.submit, second, key, context(second_generation), to(22))
            .is_ok(),
        "and then the new hold may take the input"
    );
}

fn both_bits() -> sophia_input_authority::SettlementBit {
    sophia_input_authority::SettlementBit {
        native_reconciled: true,
        recipient_settled: true,
    }
}

#[test]
fn transport_settlement_alone_does_not_discharge_native_reconciliation() {
    let mut f = fixture();
    let (capability, generation) = granted(&mut f, 100);
    let key = Input::key(30).expect("keycode");
    f.authority
        .execute_press(&f.submit, capability, key, context(generation), to(11))
        .expect("held");
    let ReleaseOutcome::DeliverTo(owed) = f
        .authority
        .release(&f.submit, capability, key)
        .expect("released")
    else {
        panic!("a release is owed");
    };

    let transport_only = sophia_input_authority::SettlementBit {
        native_reconciled: false,
        recipient_settled: true,
    };
    assert!(
        !f.authority
            .settle(&f.issuer, key, owed, transport_only)
            .expect("settle runs"),
        "a flush proves the release left the server and nothing about the \
         shared modifier and grab state it never touched"
    );
}

#[test]
fn a_handle_from_another_authority_is_refused_despite_an_identical_binding() {
    // Both authorities are built for the same instance and seat, from public
    // values any caller can reconstruct. Only the identity they mint privately
    // tells them apart.
    let mut first = fixture();
    let second = fixture_for(InstanceId::new(1));
    assert_eq!(
        first.authority.issue_grant(&second.issuer).unwrap_err(),
        RegistrationError::ForeignAuthority,
        "a second authority's issuer must not command the first"
    );

    let (capability, generation) = granted(&mut first, 100);
    let key = Input::key(30).expect("keycode");
    assert_eq!(
        first
            .authority
            .execute_press(&second.submit, capability, key, context(generation), to(1))
            .unwrap_err(),
        RegistrationError::ForeignAuthority,
        "nor may its submit handle carry another's capability"
    );
}

#[test]
fn a_capacity_whose_sources_outrun_the_holder_set_is_refused() {
    let binding = SeatBinding::new(InstanceId::new(1), SeatId::from_raw(1));
    let too_many = Capacity {
        grants: 64,
        physical_sources: 64,
        ..Capacity::PLANNED
    };
    let refused = AuthorityInstance::new(binding, too_many, 9);
    assert!(refused.is_err(), "unaddressable sources must be refused");
    assert_eq!(
        refused.err().expect("refused"),
        CapacityError::HolderWidthExceeded {
            sources: 64 * 2 + 64,
            width: 64,
        },
        "a source that cannot be addressed would silently alias another"
    );
}

#[test]
fn a_physical_hold_survives_the_synthetic_source_that_shared_it_retiring() {
    let mut f = fixture();
    let physical = f
        .authority
        .register_physical(&f.issuer, DeviceId::from_raw(1))
        .expect("physical");
    let (synthetic, generation) = granted(&mut f, 100);
    let key = Input::key(30).expect("keycode");

    f.authority
        .execute_physical_press(&f.issuer, physical, key, to(7))
        .expect("the operator's press applies");
    f.authority
        .execute_press(&f.submit, synthetic, key, context(generation), to(7))
        .expect("the injector joins the hold");

    let grant = f.authority.owner_of(synthetic.source()).expect("owned");
    let debt = f.authority.revoke_grant(&f.issuer, grant).expect("revoked");
    assert_eq!(
        debt.owed_releases, 0,
        "a survivor remains, so the retirement owes no release"
    );
    assert_eq!(debt.survivors, 1);

    let outcome = f
        .authority
        .release_physical(&f.issuer, physical, key)
        .expect("released");
    assert!(
        matches!(outcome, ReleaseOutcome::DeliverTo(_)),
        "the operator still held it, and letting go still owes the release"
    );
}

#[test]
fn a_physical_source_is_distinguishable_from_a_synthetic_one() {
    let mut f = fixture();
    let physical = f
        .authority
        .register_physical(&f.issuer, DeviceId::from_raw(1))
        .expect("physical");
    let (synthetic, _) = granted(&mut f, 100);

    assert!(f.authority.is_physical(physical));
    assert!(
        !f.authority.is_physical(synthetic.source()),
        "a granted device is not physical, which is what keeps synthetic input \
         out of the emergency recognizer"
    );
    assert_eq!(f.authority.owner_of(physical), None);

    let key = Input::key(31).expect("keycode");
    assert_eq!(
        f.authority
            .execute_physical_press(&f.issuer, synthetic.source(), key, to(9))
            .unwrap_err(),
        RegistrationError::ForeignAuthority,
        "a synthetic source cannot enter through the physical path"
    );
}

#[test]
fn execution_refuses_while_a_transition_has_not_published() {
    let mut f = fixture();
    let (capability, generation) = granted(&mut f, 100);
    let key = Input::key(30).expect("keycode");

    f.authority.begin_transition(&f.issuer).expect("transition");
    assert_eq!(
        f.authority
            .execute_press(&f.submit, capability, key, context(generation), to(1))
            .unwrap_err(),
        RegistrationError::RoutingUnavailable,
        "between commit and publication there is no current target"
    );

    f.authority.publish(&f.issuer, 1, 0).expect("published");
    let after = ExecutionContext {
        generation,
        epoch: 0,
        publication: 1,
        request: 1,
    };
    assert!(
        f.authority
            .execute_press(&f.submit, capability, key, after, to(1))
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
        "the record count is derived from the domain"
    );
}

#[test]
fn a_grant_cannot_hold_more_devices_than_its_allowance() {
    let mut f = fixture();
    let (grant, generation) = f.authority.issue_grant(&f.issuer).expect("grant");
    for device in 0..2u64 {
        f.authority
            .allocate_device(&f.issuer, grant, generation, DeviceId::from_raw(device))
            .expect("two devices are allowed");
    }
    assert_eq!(
        f.authority
            .allocate_device(&f.issuer, grant, generation, DeviceId::from_raw(9))
            .unwrap_err(),
        RegistrationError::Capacity(CapacityError::NoDeviceSlot),
        "a keyboard and a pointer is the allowance; a third is refused"
    );
}
