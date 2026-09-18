//! A coordinator's guard evidence must bind to the authority it was derived from.
use sophia_input_authority::*;
use sophia_protocol::SeatId;

fn authority() -> (AuthorityInstance, IssuerHandle, SubmitHandle) {
    AuthorityInstance::new(
        SeatBinding::new(InstanceId::new(1), SeatId::from_raw(1)),
        Capacity::PLANNED,
        9,
    )
    .unwrap()
}

#[test]
fn identical_public_bindings_have_distinct_control_identities() {
    let (mut first, first_issuer, _) = authority();
    let (mut second, second_issuer, _) = authority();
    let first_id = first.authority_identity(&first_issuer).unwrap();
    let second_id = second.authority_identity(&second_issuer).unwrap();
    assert_ne!(first_id, second_id);
    assert_eq!(
        first.control_permit(&first_issuer).unwrap().identity(),
        first_id
    );
    assert_eq!(
        second.control_permit(&second_issuer).unwrap().identity(),
        second_id
    );
}

#[test]
fn a_foreign_issuer_cannot_obtain_control_or_its_identity() {
    let (mut first, issuer, _) = authority();
    let (_, foreign, _) = authority();
    assert_eq!(
        first.authority_identity(&foreign),
        Err(RegistrationError::ForeignAuthority)
    );
    assert!(matches!(
        first.control_permit(&foreign),
        Err(RegistrationError::ForeignAuthority)
    ));
    assert!(first.control_permit(&issuer).is_ok());
}

#[test]
fn control_survives_revocation_and_unavailable_publication() {
    let (mut instance, issuer, _) = authority();
    let original = instance.authority_identity(&issuer).unwrap();
    let (grant, _) = instance
        .issue_grant(
            &issuer,
            ConnectionIdentity {
                recipient: 1,
                connection_generation: 1,
            },
        )
        .unwrap();
    instance.revoke_grant(&issuer, grant).unwrap();
    instance.begin_transition(&issuer, 1, 1).unwrap();
    assert_eq!(
        instance.published_revision(&issuer),
        Err(RegistrationError::RoutingUnavailable)
    );
    assert_eq!(
        instance.control_permit(&issuer).unwrap().identity(),
        original
    );
    instance.publish(&issuer, 1, 1).unwrap();
    assert_eq!(instance.authority_identity(&issuer).unwrap(), original);
}
