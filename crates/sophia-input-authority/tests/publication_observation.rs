//! Coordinator construction must derive committed state, never guess it.
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
fn a_pending_transition_never_exposes_the_previous_published_revision() {
    let (mut authority, issuer, _) = authority();
    assert_eq!(
        authority.published_revision(&issuer).unwrap(),
        PublishedRevision {
            control_epoch: 0,
            publication: 0
        }
    );
    authority.begin_transition(&issuer, 10, 4).unwrap();
    assert_eq!(
        authority.published_revision(&issuer),
        Err(RegistrationError::RoutingUnavailable)
    );
    assert!(authority.publish(&issuer, 9, 4).is_err());
    assert_eq!(
        authority.published_revision(&issuer),
        Err(RegistrationError::RoutingUnavailable)
    );
    authority.publish(&issuer, 10, 4).unwrap();
    assert_eq!(
        authority.published_revision(&issuer).unwrap(),
        PublishedRevision {
            control_epoch: 4,
            publication: 10
        }
    );
    authority.begin_transition(&issuer, 11, 4).unwrap();
    assert_eq!(
        authority.published_revision(&issuer),
        Err(RegistrationError::RoutingUnavailable)
    );
    authority.publish(&issuer, 11, 4).unwrap();
    assert_eq!(
        authority.published_revision(&issuer).unwrap(),
        PublishedRevision {
            control_epoch: 4,
            publication: 11
        }
    );
}

#[test]
fn an_identically_bound_foreign_issuer_cannot_read_coordinator_state() {
    let (authority, issuer, _) = authority();
    let (_, foreign, _) = self::authority();
    assert_eq!(
        authority.published_revision(&foreign),
        Err(RegistrationError::ForeignAuthority)
    );
    assert!(authority.published_revision(&issuer).is_ok());
}
