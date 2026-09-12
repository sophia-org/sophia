//! Independent pool and lifecycle regressions from the external M2 review.
//!
//! The original failing 5a85cbca, 55651225 and 935c3ac8 archives and results
//! remain outside this checkout. Assertions below state the required safety;
//! they do not turn a reproduced defect into a passing test.
use sophia_input_authority::{
    AuthorityInstance, Capacity, DeviceCapability, ExecutionContext, GrantGeneration, GrantId,
    HoldIncarnation, Input, InstanceId, IssuerHandle, Recipient, ReleaseOutcome, SeatBinding,
    SettlementBit, SubmitHandle,
};
use sophia_protocol::{DeviceId, SeatId};

struct Fixture {
    authority: AuthorityInstance,
    issuer: IssuerHandle,
    submit: SubmitHandle,
    devices: Vec<(DeviceCapability, GrantGeneration)>,
}

fn fixture(instance: u64) -> Fixture {
    let (authority, issuer, submit) = AuthorityInstance::new(
        SeatBinding::new(InstanceId::new(instance), SeatId::from_raw(1)),
        Capacity::PLANNED,
        9,
    )
    .unwrap();
    Fixture {
        authority,
        issuer,
        submit,
        devices: Vec::new(),
    }
}

fn granted(f: &mut Fixture) -> (GrantId, GrantGeneration, DeviceCapability) {
    let (grant, generation) = f.authority.issue_grant(&f.issuer).unwrap();
    let capability = f
        .authority
        .allocate_device(
            &f.issuer,
            grant,
            generation,
            DeviceId::from_raw(100 + u64::try_from(f.devices.len()).unwrap()),
        )
        .unwrap();
    f.devices.push((capability, generation));
    (grant, generation, capability)
}

fn context(generation: GrantGeneration) -> ExecutionContext {
    ExecutionContext {
        generation,
        epoch: 0,
        publication: 0,
        request: 1,
    }
}

fn to(recipient: u64) -> Recipient {
    Recipient {
        recipient,
        connection_generation: 1,
    }
}

fn input() -> Input {
    Input::key(30).unwrap()
}

fn press(f: &mut Fixture, device: DeviceCapability, generation: GrantGeneration, recipient: u64) {
    f.authority
        .execute_press(
            &f.submit,
            device,
            input(),
            context(generation),
            to(recipient),
        )
        .unwrap();
}

fn release(f: &mut Fixture, device: DeviceCapability, recipient: u64) -> HoldIncarnation {
    assert_release(release_result(f, device).unwrap(), input(), recipient)
}

fn assert_release(outcome: ReleaseOutcome, input: Input, recipient: u64) -> HoldIncarnation {
    let ReleaseOutcome::DeliverTo(incarnation) = outcome else {
        panic!("expected last-holder release, got {outcome:?}")
    };
    assert_eq!(incarnation.input, input);
    assert_eq!(incarnation.recipient, recipient);
    assert_eq!(incarnation.connection_generation, 1);
    incarnation
}

fn cleared() -> SettlementBit {
    SettlementBit {
        native_reconciled: true,
        recipient_settled: true,
    }
}

fn native_cleared() -> SettlementBit {
    SettlementBit {
        native_reconciled: true,
        recipient_settled: false,
    }
}

fn settle(f: &mut Fixture, grant: GrantId, incarnation: HoldIncarnation) -> bool {
    f.authority
        .settle(
            &f.issuer,
            Some(grant),
            incarnation.input,
            incarnation,
            cleared(),
        )
        .unwrap()
}

fn release_result(
    f: &mut Fixture,
    device: DeviceCapability,
) -> Result<ReleaseOutcome, sophia_input_authority::RegistrationError> {
    let generation = f
        .devices
        .iter()
        .find(|(known, _)| *known == device)
        .unwrap()
        .1;
    f.authority
        .release(&f.submit, device, input(), context(generation))
}

#[test]
fn revoked_generation_cannot_allocate_and_execute_again() {
    let mut f = fixture(1);
    let (grant, generation, _) = granted(&mut f);
    f.authority.revoke_grant(&f.issuer, grant).unwrap();
    let result = f
        .authority
        .allocate_device(&f.issuer, grant, generation, DeviceId::from_raw(101))
        .and_then(|device| {
            f.authority
                .execute_press(&f.submit, device, input(), context(generation), to(41))
        });
    assert!(
        result.is_err(),
        "revoked generation allocated and executed: {result:?}"
    );
}

#[test]
fn settled_empty_grants_are_reusable_after_sixteen_clients() {
    let mut f = fixture(1);
    for connection in 0..17 {
        let grant = f.authority.issue_grant(&f.issuer);
        assert!(
            grant.is_ok(),
            "empty settled slots exhausted at {connection}: {grant:?}"
        );
        f.authority
            .revoke_grant(&f.issuer, grant.unwrap().0)
            .unwrap();
    }
}

#[test]
fn foreign_physical_source_must_be_registered_locally() {
    let mut local = fixture(1);
    let mut foreign = fixture(2);
    let source = foreign
        .authority
        .register_physical(&foreign.issuer, DeviceId::from_raw(1))
        .unwrap();
    let result = local
        .authority
        .execute_physical_press(&local.issuer, source, input(), to(41));
    assert!(
        result.is_err(),
        "foreign unregistered physical source executed: {result:?}"
    );
}

#[test]
fn pending_clearing_for_a_does_not_block_synthetic_recipient_b() {
    let mut f = fixture(1);
    let (grant, first_generation, first) = granted(&mut f);
    let (_, second_generation, second) = granted(&mut f);
    press(&mut f, first, first_generation, 41);
    let a = release(&mut f, first, 41);
    assert!(
        !f.authority
            .settle(&f.issuer, Some(grant), input(), a, native_cleared())
            .unwrap()
    );
    let result = f.authority.execute_press(
        &f.submit,
        second,
        input(),
        context(second_generation),
        to(42),
    );
    assert!(
        result.is_ok(),
        "A's transport debt blocks independent recipient B: {result:?}"
    );
}

#[test]
fn pending_synthetic_clearing_does_not_block_physical_recipient_b() {
    let mut f = fixture(1);
    let (grant, generation, device) = granted(&mut f);
    let physical = f
        .authority
        .register_physical(&f.issuer, DeviceId::from_raw(1))
        .unwrap();
    press(&mut f, device, generation, 41);
    let a = release(&mut f, device, 41);
    assert!(
        !f.authority
            .settle(&f.issuer, Some(grant), input(), a, native_cleared())
            .unwrap()
    );
    let result = f
        .authority
        .execute_physical_press(&f.issuer, physical, input(), to(42));
    assert!(
        result.is_ok(),
        "A's transport debt blocks physical recipient B: {result:?}"
    );
}

#[test]
fn old_publication_cannot_reenable_a_transition() {
    let mut f = fixture(1);
    let (_, generation, device) = granted(&mut f);
    f.authority.begin_transition(&f.issuer, 10, 3).unwrap();
    f.authority.publish(&f.issuer, 10, 3).unwrap();
    f.authority.begin_transition(&f.issuer, 11, 4).unwrap();
    let result = f.authority.publish(&f.issuer, 10, 3).and_then(|_| {
        let mut old_context = context(generation);
        old_context.epoch = 3;
        old_context.publication = 10;
        f.authority
            .execute_press(&f.submit, device, input(), old_context, to(41))
    });
    assert!(
        result.is_err(),
        "old publication reopened transition: {result:?}"
    );
}

#[test]
fn adapter_release_cannot_execute_during_publication_gap() {
    let mut f = fixture(1);
    let (_, generation, device) = granted(&mut f);
    press(&mut f, device, generation, 41);
    f.authority.begin_transition(&f.issuer, 1, 1).unwrap();
    let result = release_result(&mut f, device);
    assert!(
        result.is_err(),
        "ordinary release mutates authority during publication gap: {result:?}"
    );
}

#[test]
fn old_epoch_capability_cannot_release_after_epoch_advance() {
    let mut f = fixture(1);
    let (_, generation, device) = granted(&mut f);
    press(&mut f, device, generation, 41);
    f.authority.begin_transition(&f.issuer, 1, 1).unwrap();
    f.authority.publish(&f.issuer, 1, 1).unwrap();
    let result = release_result(&mut f, device);
    assert!(
        result.is_err(),
        "old-epoch capability still releases: {result:?}"
    );
}

#[test]
fn reused_callback_identity_cannot_let_an_old_completion_clear_new_debt() {
    // The callback no longer exists. The adapted attack supplies the same
    // permitted Recipient twice; the authority must mint different identities.
    let mut f = fixture(1);
    let (grant, generation, device) = granted(&mut f);
    press(&mut f, device, generation, 41);
    let old = release(&mut f, device, 41);
    assert!(settle(&mut f, grant, old));
    press(&mut f, device, generation, 41);
    let new = release(&mut f, device, 41);
    assert_ne!(old, new);
    assert!(!settle(&mut f, grant, old));
    assert!(settle(&mut f, grant, new));
}

#[test]
fn duplicate_and_barrier_refusal_do_not_invoke_delivery() {
    // Recipient replaced the effectful callback. Observe unchanged Applied
    // identity on duplicate, and no hold added behind a refused barrier.
    let mut f = fixture(1);
    let (_, generation, device) = granted(&mut f);
    let first = f
        .authority
        .execute_press(&f.submit, device, input(), context(generation), to(41))
        .unwrap();
    let duplicate = f
        .authority
        .execute_press(&f.submit, device, input(), context(generation), to(42))
        .unwrap();
    assert_eq!(first.incarnation(), duplicate.incarnation());
    assert!(
        first.first_press(),
        "the first aggregate press requires delivery"
    );
    assert!(
        !duplicate.first_press(),
        "a duplicate must not deliver another press"
    );
    release(&mut f, device, 41);
    assert!(
        f.authority
            .execute_press(&f.submit, device, input(), context(generation), to(41))
            .is_err()
    );
    assert_eq!(
        release_result(&mut f, device).unwrap(),
        ReleaseOutcome::NotHeld
    );
}

#[test]
fn stale_identity_and_one_settlement_layer_do_not_clear_current_debt() {
    let mut f = fixture(1);
    let (grant, generation, device) = granted(&mut f);
    press(&mut f, device, generation, 41);
    let old = release(&mut f, device, 41);
    assert!(
        !f.authority
            .settle(&f.issuer, Some(grant), input(), old, native_cleared())
            .unwrap()
    );
    assert!(
        f.authority
            .settle(
                &f.issuer,
                Some(grant),
                input(),
                old,
                SettlementBit {
                    native_reconciled: false,
                    recipient_settled: true
                }
            )
            .unwrap()
    );
    press(&mut f, device, generation, 41);
    let new = release(&mut f, device, 41);
    assert!(!settle(&mut f, grant, old));
    assert!(settle(&mut f, grant, new));
}

#[test]
fn one_grant_cannot_overwrite_a_with_b_recipient_debt() {
    let mut f = fixture(1);
    let (grant, generation, device) = granted(&mut f);
    press(&mut f, device, generation, 41);
    let a = release(&mut f, device, 41);
    assert!(
        !f.authority
            .settle(&f.issuer, Some(grant), input(), a, native_cleared())
            .unwrap()
    );
    press(&mut f, device, generation, 42);
    let b = release(&mut f, device, 42);
    assert!(
        settle(&mut f, grant, a),
        "A's retained debt was overwritten by B's release on the same grant/input"
    );
    assert!(
        settle(&mut f, grant, b),
        "both independent recipient obligations must remain settleable"
    );
}

#[test]
fn another_grants_pending_b_does_not_hide_as_clearing_barrier() {
    let mut f = fixture(1);
    let (grant_a, generation_a, device_a) = granted(&mut f);
    let (grant_b, generation_b, device_b) = granted(&mut f);
    press(&mut f, device_a, generation_a, 41);
    let a = release(&mut f, device_a, 41);
    assert!(
        !f.authority
            .settle(&f.issuer, Some(grant_a), input(), a, native_cleared())
            .unwrap()
    );
    press(&mut f, device_b, generation_b, 42);
    let b = release(&mut f, device_b, 42);
    assert!(
        !f.authority
            .settle(&f.issuer, Some(grant_b), input(), b, native_cleared())
            .unwrap()
    );
    let new_a =
        f.authority
            .execute_press(&f.submit, device_a, input(), context(generation_a), to(41));
    assert!(
        new_a.is_err(),
        "recording B's debt hid A's still-pending same-recipient barrier: {new_a:?}"
    );
}

#[test]
fn ordinary_release_debt_keeps_revoked_grant_slot_occupied() {
    let mut f = fixture(1);
    let (old_grant, generation, device) = granted(&mut f);
    for _ in 1..Capacity::PLANNED.grants {
        f.authority.issue_grant(&f.issuer).unwrap();
    }
    press(&mut f, device, generation, 41);
    let owed = release(&mut f, device, 41);
    f.authority.revoke_grant(&f.issuer, old_grant).unwrap();
    assert!(
        f.authority.issue_grant(&f.issuer).is_err(),
        "the full registry reused a slot with an ordinary release still outstanding"
    );
    assert!(settle(&mut f, old_grant, owed));
    assert!(
        f.authority.issue_grant(&f.issuer).is_ok(),
        "settlement must make the retiring slot reusable"
    );
}

#[test]
fn stale_revocation_cannot_kill_reused_slot_generation() {
    let mut f = fixture(1);
    let (old_grant, _) = f.authority.issue_grant(&f.issuer).unwrap();
    for _ in 1..Capacity::PLANNED.grants {
        f.authority.issue_grant(&f.issuer).unwrap();
    }
    f.authority.revoke_grant(&f.issuer, old_grant).unwrap();
    let (new_grant, new_generation, new_device) = granted(&mut f);
    assert_ne!(
        new_grant, old_grant,
        "a reused slot must carry a fresh authority generation"
    );
    let _ = f.authority.revoke_grant(&f.issuer, old_grant);
    let still_live = f.authority.execute_press(
        &f.submit,
        new_device,
        input(),
        context(new_generation),
        to(41),
    );
    assert!(
        still_live.is_ok(),
        "old grant cleanup revoked a later generation: {still_live:?}"
    );
}

#[test]
fn physical_same_recipient_press_needs_clearing_order_not_bypass() {
    let mut f = fixture(1);
    let (grant, generation, device) = granted(&mut f);
    let physical = f
        .authority
        .register_physical(&f.issuer, DeviceId::from_raw(1))
        .unwrap();
    press(&mut f, device, generation, 41);
    let old_release = release(&mut f, device, 41);
    assert!(
        !f.authority
            .settle(
                &f.issuer,
                Some(grant),
                input(),
                old_release,
                native_cleared()
            )
            .unwrap()
    );
    // Applied currently carries no deferred/ordered-clearing outcome. An
    // unconditional applied press permits the already-issued old release to
    // arrive later at the same recipient and clear the new physical hold.
    let new_press = f
        .authority
        .execute_physical_press(&f.issuer, physical, input(), to(41));
    assert!(
        new_press.is_err(),
        "physical delivery bypassed its still-pending recipient release: {new_press:?}"
    );
}

#[test]
fn full_synthetic_pool_refuses_before_mutation_and_preserves_physical_reserve() {
    let mut f = fixture(1);
    let grants: Vec<_> = (0..Capacity::PLANNED.grants)
        .map(|_| granted(&mut f))
        .collect();
    let inputs: Vec<_> = (8..=255)
        .map(|key| Input::key(key).unwrap())
        .chain((1..=9).map(|button| Input::button(button, 9).unwrap()))
        .collect();
    assert_eq!(inputs.len(), Capacity::PLANNED.input_slots());
    let mut owed = Vec::with_capacity(Capacity::PLANNED.debt_records());
    for &(grant, generation, device) in &grants {
        for &input in &inputs {
            let recipient = 1 + u64::try_from(owed.len()).unwrap();
            f.authority
                .execute_press(&f.submit, device, input, context(generation), to(recipient))
                .unwrap();
            let receipt = assert_release(
                f.authority
                    .release(&f.submit, device, input, context(generation))
                    .unwrap(),
                input,
                recipient,
            );
            assert!(
                !f.authority
                    .settle(&f.issuer, Some(grant), input, receipt, native_cleared())
                    .unwrap()
            );
            owed.push((grant, receipt));
        }
    }
    assert_eq!(owed.len(), Capacity::PLANNED.debt_records());
    let (grant, generation, device) = grants[0];
    let overflow =
        f.authority
            .execute_press(&f.submit, device, input(), context(generation), to(999_999));
    assert!(
        overflow.is_err(),
        "full pool accepted another synthetic hold: {overflow:?}"
    );
    assert_eq!(
        release_result(&mut f, device).unwrap(),
        ReleaseOutcome::NotHeld,
        "failed reservation left an applied synthetic contribution"
    );

    // Synthetic debt cannot consume the separate physical reserve. These are
    // registered model devices, not a claim about the native hardware mapping.
    let physical = f
        .authority
        .register_physical(&f.issuer, DeviceId::from_raw(1))
        .unwrap();
    f.authority
        .execute_physical_press(&f.issuer, physical, input(), to(888_888))
        .unwrap();
    let physical_receipt = assert_release(
        f.authority
            .release_physical(&f.issuer, physical, input())
            .unwrap(),
        input(),
        888_888,
    );
    assert!(
        f.authority
            .settle(&f.issuer, None, input(), physical_receipt, cleared())
            .unwrap()
    );

    assert!(settle(&mut f, owed[0].0, owed[0].1));
    f.authority
        .execute_press(&f.submit, device, input(), context(generation), to(999_999))
        .unwrap();
    let new = release(&mut f, device, 999_999);
    assert!(
        settle(&mut f, grant, new),
        "a settled record must be reusable without losing new debt"
    );
}
