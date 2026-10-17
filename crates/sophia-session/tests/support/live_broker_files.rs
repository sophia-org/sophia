//! Protected live Session broker integration without a desktop or device access.
#![cfg(test)]
use super::*;
use sophia_protocol::{
    DisplayLabel, MetadataDisclosure, ReducedMetadataCandidate, ToplevelActionCapabilityRef,
};
use sophia_x_authority::{XAuthorityClientMetadataCandidate, XServerFrontendClientId};

#[test]
#[ignore = "requires SOPHIA_TEST_EXECUTABLE and SOPHIA_TEST_BUBBLEWRAP absolute paths"]
fn protected_live_broker_routes_and_retires_native_metadata() {
    let executable = std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_EXECUTABLE").unwrap());
    let bubblewrap = std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_BUBBLEWRAP").unwrap());
    assert!(executable.is_absolute() && bubblewrap.is_absolute());
    let mut broker = LiveMetadataBroker::start_executable(&executable, &bubblewrap).unwrap();
    let surface = SurfaceId::new(17, 2);
    let client = XServerFrontendClientId::from_raw(7);
    let other = XServerFrontendClientId::from_raw(8);
    assert!(
        broker
            .admit_surface(client, surface, NamespaceProfile::Confined)
            .unwrap()
            .is_some()
    );
    assert!(
        broker
            .admit_surface(client, surface, NamespaceProfile::Confined)
            .unwrap()
            .is_none()
    );
    assert!(
        broker
            .admit_surface(other, surface, NamespaceProfile::Confined)
            .is_err()
    );
    let candidate = ReducedMetadataCandidate {
        surface,
        disclosure: MetadataDisclosure::ClassOnly,
        label: Some(DisplayLabel {
            text: "Editor".into(),
            redacted: false,
        }),
        generation: 1,
    };
    assert!(
        broker
            .apply_candidate(XAuthorityClientMetadataCandidate {
                client: other,
                candidate: candidate.clone()
            })
            .is_err()
    );
    assert!(broker.descriptors().is_empty());
    broker
        .apply_candidate(XAuthorityClientMetadataCandidate {
            client,
            candidate: candidate.clone(),
        })
        .unwrap();
    let sources = broker.shell_sources(&[surface].into());
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].descriptor.label.as_ref().unwrap().text, "Editor");
    let grant = sources[0].grant;
    let action = ToplevelActionCapabilityRef {
        token: grant.token,
        issuer_epoch: broker.connection_epoch(),
        issuer_revocation_epoch: grant.revocation_epoch,
        recipient_epoch: 1,
        target_slot: 1,
        target_generation: grant.target_generation,
    };
    assert_eq!(broker.resolve_toplevel_action(action), Some(surface));
    let mut newer = candidate.clone();
    newer.generation = 2;
    broker
        .apply_candidate(XAuthorityClientMetadataCandidate {
            client,
            candidate: newer,
        })
        .unwrap();
    assert_eq!(broker.resolve_toplevel_action(action), None);
    broker.poll().unwrap();
    broker.retire_surface(surface).unwrap();
    broker.retire_surface(surface).unwrap();
    assert!(broker.descriptors().is_empty());
    assert!(broker.shell_sources(&[surface].into()).is_empty());
    assert!(
        broker
            .admit_surface(client, surface, NamespaceProfile::Confined)
            .unwrap()
            .is_none()
    );
    broker
        .apply_candidate(XAuthorityClientMetadataCandidate { client, candidate })
        .unwrap();
    assert!(broker.descriptors().is_empty());
    // A retirement arriving before admission must also prevent later admission.
    let unseen = SurfaceId::new(18, 2);
    broker.retire_surface(unseen).unwrap();
    assert!(
        broker
            .admit_surface(client, unseen, NamespaceProfile::Confined)
            .unwrap()
            .is_none()
    );
}
