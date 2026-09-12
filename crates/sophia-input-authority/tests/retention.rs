//! Regression cases for source reincarnation and joined-holder retirement.
use sophia_input_authority::*;
use sophia_protocol::{DeviceId, SeatId};

fn instance() -> (AuthorityInstance, IssuerHandle, SubmitHandle) {
    AuthorityInstance::new(
        SeatBinding::new(InstanceId::new(7), SeatId::from_raw(1)),
        Capacity::PLANNED,
        9,
    )
    .unwrap()
}
fn grant(a: &mut AuthorityInstance, issuer: &IssuerHandle, device: u64) -> DeviceCapability {
    let (grant, generation) = a.issue_grant(issuer).unwrap();
    a.allocate_device(issuer, grant, generation, DeviceId::from_raw(device))
        .unwrap()
}
fn context(cap: DeviceCapability) -> ExecutionContext {
    ExecutionContext {
        generation: cap.generation(),
        epoch: 0,
        publication: 0,
        request: 1,
    }
}
fn recipient() -> Recipient {
    Recipient {
        recipient: 17,
        connection_generation: 1,
    }
}
fn settled() -> SettlementBit {
    SettlementBit {
        native_reconciled: true,
        recipient_settled: true,
    }
}

#[test]
fn retiring_one_injector_preserves_the_other_and_both_references_settle() {
    let (mut a, issuer, submit) = instance();
    let first = grant(&mut a, &issuer, 1);
    let second = grant(&mut a, &issuer, 2);
    let key = Input::key(30).unwrap();
    let press = a
        .execute_press(&submit, first, key, context(first), recipient())
        .unwrap();
    let join = a
        .execute_press(&submit, second, key, context(second), recipient())
        .unwrap();
    assert!(press.first_press());
    assert!(!join.first_press());
    assert_eq!(press.incarnation(), join.incarnation());
    assert_eq!(
        a.revoke_grant(&issuer, first.grant()).unwrap(),
        RetiredDebt {
            owed_releases: 0,
            survivors: 1
        }
    );
    let ReleaseOutcome::DeliverTo(owed) = a.release(&submit, second, key, context(second)).unwrap()
    else {
        panic!("survivor must still hold the original input");
    };
    assert_eq!(owed, press.incarnation());
    assert!(
        a.settle(&issuer, Some(second.grant()), key, owed, settled())
            .unwrap()
    );
    a.revoke_grant(&issuer, second.grant()).unwrap();
    // Both participant references must retire, not just the release's owner.
    let fresh: Vec<_> = (0..16).map(|_| a.issue_grant(&issuer).unwrap()).collect();
    assert_eq!(fresh.len(), 16);
    assert!(a.issue_grant(&issuer).is_err());
}

#[test]
fn two_devices_of_one_grant_release_one_aggregate_and_reclaim_the_slot() {
    let (mut a, issuer, submit) = instance();
    let first = grant(&mut a, &issuer, 1);
    let second = a
        .allocate_device(
            &issuer,
            first.grant(),
            first.generation(),
            DeviceId::from_raw(2),
        )
        .unwrap();
    let key = Input::key(30).unwrap();
    let press = a
        .execute_press(&submit, first, key, context(first), recipient())
        .unwrap();
    a.execute_press(&submit, second, key, context(second), recipient())
        .unwrap();
    assert_eq!(
        a.revoke_grant(&issuer, first.grant()).unwrap(),
        RetiredDebt {
            owed_releases: 1,
            survivors: 1
        }
    );
    assert!(
        a.settle(
            &issuer,
            Some(first.grant()),
            key,
            press.incarnation(),
            settled()
        )
        .unwrap()
    );
    for _ in 0..16 {
        a.issue_grant(&issuer).unwrap();
    }
    assert!(a.issue_grant(&issuer).is_err());
}

#[test]
fn a_replaced_physical_source_does_not_inherit_origin_or_release_authority() {
    let (mut a, issuer, _) = instance();
    let old = a.register_physical(&issuer, DeviceId::from_raw(1)).unwrap();
    let key = Input::key(30).unwrap();
    let press = a
        .execute_physical_press(&issuer, old, key, recipient())
        .unwrap();
    a.retire_physical(&issuer, old).unwrap();
    assert!(
        a.settle(&issuer, None, key, press.incarnation(), settled())
            .unwrap()
    );
    let new = a.register_physical(&issuer, DeviceId::from_raw(2)).unwrap();
    assert_ne!(old, new);
    assert!(!a.is_physical(old));
    assert_eq!(a.device_of(old), None);
    assert!(a.is_physical(new));
    a.execute_physical_press(&issuer, new, key, recipient())
        .unwrap();
    assert_eq!(
        a.release_physical(&issuer, old, key).unwrap_err(),
        RegistrationError::ForeignAuthority
    );
    assert!(matches!(
        a.release_physical(&issuer, new, key).unwrap(),
        ReleaseOutcome::DeliverTo(_)
    ));
}

#[test]
fn capacity_overflow_and_a_different_key_domain_are_refused() {
    let binding = SeatBinding::new(InstanceId::new(7), SeatId::from_raw(1));
    for capacity in [
        Capacity {
            grants: usize::MAX,
            ..Capacity::PLANNED
        },
        Capacity {
            keys: 247,
            ..Capacity::PLANNED
        },
        Capacity {
            buttons: 256,
            ..Capacity::PLANNED
        },
    ] {
        assert!(AuthorityInstance::new(binding, capacity, capacity.buttons as u16).is_err());
    }
}
