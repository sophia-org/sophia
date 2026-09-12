//! Safety regressions ported from an independent review of `5a85cbca`.
//!
//! Every test here failed against that revision. They are kept under the names
//! the reviewer gave them so the external suite and this one name the same
//! defects, and so a later reader can find what each was protecting against:
//!
//! - inputs aliased, because keys and buttons shared a numbering;
//! - sources aliased, because the holder set wrapped at thirty-two;
//! - a revoked capability still pressing, because retirement marked nothing;
//! - an issuer forged, because handles compared only a public binding;
//! - a foreign source releasing a local hold, because release took a raw id;
//! - a new hold racing an unsettled release, because there was no barrier;
//! - a grant taking unlimited devices, because nothing counted them.

use sophia_input_authority::{
    AuthorityInstance, Capacity, CapacityError, DeviceCapability, ExecutionContext,
    GrantGeneration, Input, InstanceId, IssuerHandle, Recipient, RegistrationError, ReleaseOutcome,
    SeatBinding, SubmitHandle,
};
use sophia_protocol::{DeviceId, SeatId};

struct Fixture {
    authority: AuthorityInstance,
    issuer: IssuerHandle,
    submit: SubmitHandle,
}

fn fixture() -> Fixture {
    let binding = SeatBinding::new(InstanceId::new(1), SeatId::from_raw(1));
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

fn to(id: u64) -> Recipient {
    Recipient {
        recipient: id,
        connection_generation: 1,
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
fn key_255_and_button_7_are_independent_inputs() {
    let key = Input::key(255).expect("255 is a keycode");
    let button = Input::button(7, 9).expect("7 is a button");
    assert_ne!(key, button);

    let mut f = fixture();
    let (capability, generation) = granted(&mut f, 100);
    f.authority
        .execute_press(&f.submit, capability, key, context(generation), to(1))
        .expect("the key presses");
    assert_eq!(
        f.authority
            .release(
                &f.submit,
                capability,
                button,
                context(capability.generation())
            )
            .expect("release runs"),
        ReleaseOutcome::NotHeld,
        "releasing a button must not clear a key"
    );
}

#[test]
fn source_32_retirement_preserves_physical_source_0() {
    let mut f = fixture();
    let physical = f
        .authority
        .register_physical(&f.issuer, DeviceId::from_raw(1))
        .expect("physical source zero");
    // Fill the synthetic table: sixteen grants of two devices is thirty-two,
    // the index that wrapped onto zero under the old holder set.
    let mut last = None;
    for device in 0..16u64 {
        let (grant, generation) = f.authority.issue_grant(&f.issuer).expect("grant");
        for slot in 0..2u64 {
            last = Some(
                f.authority
                    .allocate_device(
                        &f.issuer,
                        grant,
                        generation,
                        DeviceId::from_raw(device * 2 + slot),
                    )
                    .expect("device"),
            );
        }
    }
    let last = last.expect("allocated");
    let key = Input::key(30).expect("keycode");
    f.authority
        .execute_physical_press(&f.issuer, physical, key, to(5))
        .expect("physical press");

    let grant = f.authority.owner_of(last.source()).expect("owned");
    f.authority.revoke_grant(&f.issuer, grant).expect("revoked");
    assert!(
        matches!(
            f.authority.release_physical(&f.issuer, physical, key),
            Ok(ReleaseOutcome::DeliverTo(_))
        ),
        "the physical hold must survive the last synthetic source retiring"
    );
}

#[test]
fn retired_device_cannot_replay_a_capability() {
    let mut f = fixture();
    let (capability, generation) = granted(&mut f, 100);
    let key = Input::key(30).expect("keycode");
    let grant = f.authority.owner_of(capability.source()).expect("owned");
    f.authority.revoke_grant(&f.issuer, grant).expect("revoked");
    assert_eq!(
        f.authority
            .execute_press(&f.submit, capability, key, context(generation), to(1))
            .unwrap_err(),
        RegistrationError::StaleGeneration
    );
}

#[test]
fn same_binding_does_not_forge_another_authoritys_issuer() {
    let mut first = fixture();
    let second = fixture();
    assert_eq!(
        first.authority.issue_grant(&second.issuer).unwrap_err(),
        RegistrationError::ForeignAuthority,
        "rebuilding the public binding must not produce a usable issuer"
    );
}

#[test]
fn foreign_source_id_cannot_release_a_local_hold() {
    let mut first = fixture();
    let mut second = fixture();
    let key = Input::key(30).expect("keycode");
    let (local, local_generation) = granted(&mut first, 100);
    first
        .authority
        .execute_press(&first.submit, local, key, context(local_generation), to(1))
        .expect("held here");

    let (foreign, _) = granted(&mut second, 100);
    assert_eq!(
        first
            .authority
            .release(&first.submit, foreign, key, context(foreign.generation()))
            .unwrap_err(),
        RegistrationError::ForeignAuthority,
        "a capability from another authority must not reach this hold"
    );
}

#[test]
fn uncleared_release_blocks_a_new_same_recipient_hold() {
    let mut f = fixture();
    let (first, first_generation) = granted(&mut f, 100);
    let key = Input::key(30).expect("keycode");
    f.authority
        .execute_press(&f.submit, first, key, context(first_generation), to(11))
        .expect("held");
    let ReleaseOutcome::DeliverTo(_) = f
        .authority
        .release(&f.submit, first, key, context(first.generation()))
        .expect("release runs")
    else {
        panic!("the last holder owes a release");
    };

    let (second, second_generation) = granted(&mut f, 101);
    assert_eq!(
        f.authority
            .execute_press(&f.submit, second, key, context(second_generation), to(11))
            .unwrap_err(),
        RegistrationError::ReleaseBarrier,
        "a new hold must wait until the previous release settles"
    );
}

#[test]
fn a_grant_cannot_allocate_more_than_two_devices() {
    let mut f = fixture();
    let (grant, generation) = f.authority.issue_grant(&f.issuer).expect("grant");
    for device in 0..2u64 {
        f.authority
            .allocate_device(&f.issuer, grant, generation, DeviceId::from_raw(device))
            .expect("a keyboard and a pointer");
    }
    assert_eq!(
        f.authority
            .allocate_device(&f.issuer, grant, generation, DeviceId::from_raw(9))
            .unwrap_err(),
        RegistrationError::Capacity(CapacityError::NoDeviceSlot)
    );
}
