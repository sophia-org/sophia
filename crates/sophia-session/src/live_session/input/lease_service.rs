fn service_application_route_leases(
    receiver: &Receiver<XAuthorityRouteLeaseUpdate>,
    leases: &mut ApplicationRouteLeaseState,
    seat: sophia_protocol::SeatId,
    started: Instant,
    frontend_service_sender: &SyncSender<XServerFrontendServiceCommand>,
) -> Result<(), Box<dyn std::error::Error>> {
        let lease_updates = drain_application_route_lease_updates(
            receiver,
            leases,
        );
        if lease_updates.confirmed != 0
            || lease_updates.rejected != 0
            || lease_updates.released != 0
            || lease_updates.stale != 0
        {
            crate::session_println!(
                "sophia_live_input_lease schema=1 confirmed={} rejected={} released={} stale={}",
                lease_updates.confirmed,
                lease_updates.rejected,
                lease_updates.released,
                lease_updates.stale,
            );
        }
        if let sophia_engine::ApplicationRouteLeaseTimeout::Quarantine(lease) =
            leases.observe_timeout(
                seat,
                u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            )
        {
            frontend_service_sender.try_send(XServerFrontendServiceCommand::RevokeAdmission {
                admission: lease.admission,
            })?;
            crate::session_eprintln!(
                "sophia_live_input_lease schema=1 status=quarantined reason=release_timeout admission={}",
                lease.admission.raw(),
            );
        }
    Ok(())
}
