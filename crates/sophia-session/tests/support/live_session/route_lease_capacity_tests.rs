use crate::live_session::service_application_route_leases;
use sophia_engine::{
    ApplicationRouteLeaseBinding, ApplicationRouteLeaseCandidate, ApplicationRouteLeaseOrigin,
    ApplicationRouteLeaseState, ApplicationRouteScope,
};
use sophia_protocol::{
    ClientAdmissionContext, ClientAdmissionId, ClientAuthProvenance, ClientAuthenticationMethod,
    DeviceId, NamespaceCapabilities, NamespaceContext, NamespaceId, NamespaceProfile, OutputId,
    SeatId, SurfaceId,
};
use sophia_x_authority::{XAuthorityRouteLeaseUpdate, XAuthorityRouteLeaseUpdateKind};
use std::sync::mpsc::sync_channel;
use std::time::Instant;

fn candidate() -> ApplicationRouteLeaseCandidate {
    ApplicationRouteLeaseCandidate {
        seat: SeatId::from_raw(1),
        origin: ApplicationRouteLeaseOrigin::PointerBoundary,
        target_surface: SurfaceId::new(40, 3),
        admission: ClientAdmissionId::from_raw(7),
        scope: ApplicationRouteScope {
            profile: NamespaceProfile::Confined,
            authority: NamespaceId::from_raw(4),
        },
        authority_session_epoch: 9,
        binding: ApplicationRouteLeaseBinding::Bound {
            output: OutputId::from_raw(2),
            revision: 11,
        },
        initiating_device: Some(DeviceId::from_raw(5)),
        initiating_button: Some(0x110),
    }
}

fn admission() -> ClientAdmissionContext {
    ClientAdmissionContext::new(
        candidate().admission,
        NamespaceContext::new(
            NamespaceId::from_raw(4),
            NamespaceProfile::Confined,
            NamespaceCapabilities::NONE,
        )
        .unwrap(),
        ClientAuthProvenance::new(ClientAuthenticationMethod::PeerCredentials, 9).unwrap(),
    )
    .unwrap()
}

#[test]
fn route_lease_stale_drain_returns_capacity_wake_without_command() {
    let mut leases = ApplicationRouteLeaseState::default();
    let old = leases.begin_provisional(candidate()).unwrap();
    leases.reject(old.identity).unwrap();
    let (updates, receiver) = sync_channel(1);
    let (commands, command_receiver) = sync_channel(1);
    let wake = sophia_wake::Wake::new().unwrap();
    let slot = sophia_wake::WakeSlot::default();
    slot.set(wake.notifier());
    let commands = sophia_wake::SignalSender::new(commands, slot);
    wake.clear().unwrap();
    updates
        .try_send(XAuthorityRouteLeaseUpdate {
            identity: old.identity,
            target_surface: old.target_surface,
            admission: admission(),
            kind: XAuthorityRouteLeaseUpdateKind::Confirmed,
        })
        .unwrap();
    service_application_route_leases(
        &receiver,
        &mut leases,
        candidate().seat,
        Instant::now(),
        &commands,
    )
    .unwrap();
    assert!(wake.wait(Some(Instant::now())).unwrap());
    assert!(command_receiver.try_recv().is_err());
    assert!(leases.lease(candidate().seat).is_none());
    wake.clear().unwrap();
    service_application_route_leases(
        &receiver,
        &mut leases,
        candidate().seat,
        Instant::now(),
        &commands,
    )
    .unwrap();
    assert!(
        !wake.wait(Some(Instant::now())).unwrap(),
        "an empty drain must not spin the frontend"
    );
    assert!(command_receiver.try_recv().is_err());
}

#[test]
fn route_lease_old_notifications_cannot_change_replacement() {
    let mut leases = ApplicationRouteLeaseState::default();
    let old = leases.begin_provisional(candidate()).unwrap();
    let replacement = leases
        .replace_explicit_provisional(
            old.identity,
            ApplicationRouteLeaseCandidate {
                origin: ApplicationRouteLeaseOrigin::ExplicitPointer,
                initiating_device: None,
                initiating_button: None,
                ..candidate()
            },
        )
        .unwrap();
    let (updates, receiver) = sync_channel(1);
    let (commands, command_receiver) = sync_channel(1);
    let wake = sophia_wake::Wake::new().unwrap();
    let slot = sophia_wake::WakeSlot::default();
    slot.set(wake.notifier());
    let commands = sophia_wake::SignalSender::new(commands, slot);
    for kind in [
        XAuthorityRouteLeaseUpdateKind::Confirmed,
        XAuthorityRouteLeaseUpdateKind::Released,
        XAuthorityRouteLeaseUpdateKind::Rejected,
    ] {
        wake.clear().unwrap();
        updates
            .try_send(XAuthorityRouteLeaseUpdate {
                identity: old.identity,
                target_surface: old.target_surface,
                admission: admission(),
                kind,
            })
            .unwrap();
        service_application_route_leases(
            &receiver,
            &mut leases,
            candidate().seat,
            Instant::now(),
            &commands,
        )
        .unwrap();
        assert_eq!(leases.lease(candidate().seat), Some(replacement));
        assert!(wake.wait(Some(Instant::now())).unwrap());
        assert!(command_receiver.try_recv().is_err());
    }
}
