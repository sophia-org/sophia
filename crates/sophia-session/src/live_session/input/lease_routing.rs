use super::*;

/// Held events belong to one exact lease. They never become a fresh selection
/// when that lease disappears or is replaced.
#[derive(Default)]
pub(super) struct PendingLeaseInput {
    held: Option<HeldLeaseInput>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HeldLeaseInputError {
    OutputChanged,
    Expired,
    Capacity,
}

struct HeldLeaseInput {
    identity: sophia_protocol::ApplicationRouteLeaseIdentity,
    output: sophia_protocol::OutputId,
    deadline_msec: u64,
    events: VecDeque<sophia_protocol::InputEventPacket>,
}

impl PendingLeaseInput {
    pub(super) fn cancel(&mut self, identity: sophia_protocol::ApplicationRouteLeaseIdentity) {
        if self
            .held
            .as_ref()
            .is_some_and(|held| held.identity == identity)
        {
            self.held = None;
        }
    }

    pub(super) fn retain_current(&mut self, state: &ApplicationRouteLeaseState) {
        if self.held.as_ref().is_some_and(|held| {
            state.lease(held.identity.seat).is_none_or(|lease| {
                lease.identity != held.identity
                    || matches!(lease.phase, ApplicationRouteLeasePhase::Releasing { .. })
            })
        }) {
            self.held = None;
        }
    }

    pub(super) fn expired(
        &self,
        now_msec: u64,
    ) -> Option<sophia_protocol::ApplicationRouteLeaseIdentity> {
        self.held
            .as_ref()
            .filter(|held| now_msec >= held.deadline_msec)
            .map(|held| held.identity)
    }

    pub(super) fn defer(
        &mut self,
        lease: sophia_engine::ApplicationRouteLease,
        output: sophia_protocol::OutputId,
        now_msec: u64,
        event: sophia_protocol::InputEventPacket,
    ) -> Result<(), HeldLeaseInputError> {
        if self
            .held
            .as_ref()
            .is_some_and(|held| held.identity != lease.identity)
        {
            self.held = None;
        }
        let deadline = match lease.binding() {
            sophia_engine::ApplicationRouteLeaseBinding::AwaitingPresentation {
                deadline_msec,
                ..
            } => deadline_msec,
            sophia_engine::ApplicationRouteLeaseBinding::Bound { .. } => {
                now_msec.saturating_add(sophia_engine::POINTER_FOCUS_HANDOFF_TIMEOUT_MSEC)
            }
        };
        let held = self.held.get_or_insert_with(|| HeldLeaseInput {
            identity: lease.identity,
            output,
            deadline_msec: deadline
                .min(now_msec.saturating_add(sophia_engine::POINTER_FOCUS_HANDOFF_TIMEOUT_MSEC)),
            events: VecDeque::new(),
        });
        if held.output != output {
            self.held = None;
            return Err(HeldLeaseInputError::OutputChanged);
        }
        if now_msec >= held.deadline_msec {
            self.held = None;
            return Err(HeldLeaseInputError::Expired);
        }
        if matches!(event.kind, sophia_protocol::InputEventKind::PointerMotion)
            && let Some(last) = held.events.back_mut()
            && matches!(last.kind, sophia_protocol::InputEventKind::PointerMotion)
            && last.device == event.device
        {
            *last = event;
            return Ok(());
        }
        if held.events.len() >= sophia_engine::POINTER_FOCUS_HANDOFF_CAPACITY {
            self.held = None;
            return Err(HeldLeaseInputError::Capacity);
        }
        held.events.push_back(event);
        Ok(())
    }
}

/// Application hit testing alone cannot establish scope: compositor targets
/// and their occlusion rectangles are deliberately absent from app layers.
#[allow(clippy::too_many_arguments)]
pub(super) fn presented_application_scope(
    event: &sophia_protocol::InputEventPacket,
    layers: &[LayerSnapshot],
    chrome_targets: &[sophia_engine::IndicatorChromeHitTarget],
    chrome_occlusion: Option<sophia_protocol::Rect>,
    descriptor_targets: &[sophia_engine::PresentedChromeTarget],
    descriptor_occlusion: Option<sophia_protocol::Rect>,
    _tab_occlusions: &[sophia_protocol::Rect],
    client_routes: &XAuthorityClientSurfaceRoutes,
) -> Option<ApplicationRouteScope> {
    let point = event.global_position?;
    if chrome_occlusion.is_some_and(|rect| point_is_inside_rect(point, rect))
        || descriptor_occlusion.is_some_and(|rect| point_is_inside_rect(point, rect))
        || chrome_targets
            .iter()
            .any(|target| point_is_inside_rect(point, target.geometry))
        || descriptor_targets
            .iter()
            .any(|target| point_is_inside_rect(point, target.geometry))
    {
        return None;
    }
    // Tab chrome is composed below applications. An eligible app hit above
    // it wins; an exposed tab has no app hit. Use the hit test, not rectangle
    // overlap, so transforms and input holes retain the same precedence.
    let surface = sophia_engine::hit_test_scene_surface_for_input(event, layers).target_surface?;
    let admission = client_routes.admission_for_surface(surface)?;
    Some(ApplicationRouteScope {
        profile: admission.namespace.profile,
        authority: admission.namespace.id,
    })
}

pub(super) fn cancel_application_lease(
    state: &mut ApplicationRouteLeaseState,
    client_routes: &XAuthorityClientSurfaceRoutes,
    sender: &SyncSender<XAuthorityRouteLeaseRelease>,
    held: &mut PendingLeaseInput,
    identity: sophia_protocol::ApplicationRouteLeaseIdentity,
    now_msec: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    held.cancel(identity);
    if state.lease(identity.seat).is_some_and(|lease| {
        lease.identity == identity
            && !matches!(lease.phase, ApplicationRouteLeasePhase::Releasing { .. })
    }) {
        request_application_route_lease_release(
            state,
            client_routes,
            sender,
            identity.seat,
            now_msec,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn reconcile_lease_presentation(
    state: &mut ApplicationRouteLeaseState,
    client_routes: &XAuthorityClientSurfaceRoutes,
    sender: &SyncSender<XAuthorityRouteLeaseRelease>,
    held: &mut PendingLeaseInput,
    seat: SeatId,
    pointer_output: Option<sophia_protocol::OutputId>,
    projections: &[sophia_backend_live::LivePresentedInputProjection],
    now_msec: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    held.retain_current(state);
    let expired = match state.observe_binding_deadline(seat, now_msec) {
        sophia_engine::ApplicationRouteLeaseBindingTimeout::Expired(lease) => Some(lease.identity),
        _ => held.expired(now_msec),
    };
    if let Some(identity) = expired {
        record_application_lease_refusal("binding_timeout");
        cancel_application_lease(state, client_routes, sender, held, identity, now_msec)?;
        return Ok(());
    }
    let Some(lease) = state.lease(seat) else {
        return Ok(());
    };
    if matches!(lease.phase, ApplicationRouteLeasePhase::Releasing { .. }) {
        return Ok(());
    }
    let (output, revision, awaiting) = match lease.binding() {
        sophia_engine::ApplicationRouteLeaseBinding::AwaitingPresentation {
            pinned_output, ..
        } => (pinned_output.or(pointer_output), 0, true),
        sophia_engine::ApplicationRouteLeaseBinding::Bound { output, revision } => {
            (Some(output), revision, false)
        }
    };
    let Some(output) = output else { return Ok(()) };
    let projection = projections
        .iter()
        .find(|projection| projection.output == output);
    if !awaiting && projection.is_some_and(|projection| projection.epoch == revision) {
        return Ok(());
    }
    let eligible = projection.is_some_and(|projection| {
        sophia_engine::scene_contains_input_surface(&projection.layers, lease.target_surface)
    });
    if eligible && awaiting {
        state
            .bind_presentation(
                lease.identity,
                output,
                projection.expect("eligible projection").epoch,
            )
            .map_err(|error| format!("failed to bind eligible application lease: {error:?}"))?;
    } else if !eligible && !awaiting {
        cancel_application_lease(state, client_routes, sender, held, lease.identity, now_msec)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn authorize_presented_lease(
    state: &mut ApplicationRouteLeaseState,
    lease: sophia_engine::ApplicationRouteLease,
    event: &sophia_protocol::InputEventPacket,
    client_routes: &XAuthorityClientSurfaceRoutes,
    scope: ApplicationRouteScope,
    output: sophia_protocol::OutputId,
    revision: u64,
    layers: &[LayerSnapshot],
) -> Result<sophia_engine::ApplicationRouteLease, sophia_engine::ApplicationRouteLeaseError> {
    let owner = client_routes
        .admission_for_surface(lease.target_surface)
        .ok_or(sophia_engine::ApplicationRouteLeaseError::IdentityMismatch)?;
    state.authorize(
        lease.identity,
        sophia_engine::ApplicationRouteTargetEvidence {
            resolved_scope: scope,
            target_surface: lease.target_surface,
            target_admission: owner.client_id,
            target_eligible: sophia_engine::scene_contains_input_surface(
                layers,
                lease.target_surface,
            ),
            presentation_revision: revision,
            output,
            device: event.device,
            authority_session_epoch: owner.auth_provenance.session_generation,
        },
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn flush_held_lease_input<S: RoutedInputIngress>(
    held: &mut PendingLeaseInput,
    state: &mut ApplicationRouteLeaseState,
    client_routes: &XAuthorityClientSurfaceRoutes,
    projections: &[sophia_backend_live::LivePresentedInputProjection],
    sender: &S,
    release_sender: &SyncSender<XAuthorityRouteLeaseRelease>,
    next_delivery: &mut u64,
    now_msec: u64,
    report: &mut PhysicalInputRouteReport,
) -> Result<(), Box<dyn std::error::Error>> {
    held.retain_current(state);
    let Some(pending) = held.held.as_ref() else {
        return Ok(());
    };
    if state.routing_readiness(pending.identity.seat)
        != Some(sophia_engine::ApplicationRouteLeaseReadiness::ReadyForEvidenceValidation)
    {
        return Ok(());
    }
    let mut pending = held.held.take().expect("held lease input");
    if now_msec >= pending.deadline_msec {
        cancel_application_lease(
            state,
            client_routes,
            release_sender,
            held,
            pending.identity,
            now_msec,
        )?;
        return Ok(());
    }
    while let Some(event) = pending.events.pop_front() {
        let Some(lease) = state
            .lease(pending.identity.seat)
            .filter(|lease| lease.identity == pending.identity)
        else {
            break;
        };
        let projection = projections
            .iter()
            .find(|projection| projection.output == pending.output);
        let validated = projection.and_then(|projection| {
            let scope = presented_application_scope(
                &event,
                &projection.layers,
                &projection.chrome_targets,
                projection.chrome_occlusion,
                &projection.descriptor_targets,
                projection.descriptor_occlusion,
                &projection.tab_occlusions,
                client_routes,
            )?;
            authorize_presented_lease(
                state,
                lease,
                &event,
                client_routes,
                scope,
                projection.output,
                projection.epoch,
                &projection.layers,
            )
            .ok()?;
            let route = sophia_engine::route_scene_surface_for_input(
                &event,
                &projection.layers,
                lease.target_surface,
            );
            Some((projection, route.local_position?))
        });
        let Some((projection, local_position)) = validated else {
            report.pointer_lease_rejections += 1;
            record_application_lease_refusal("held_evidence");
            cancel_application_lease(
                state,
                client_routes,
                release_sender,
                held,
                pending.identity,
                now_msec,
            )?;
            break;
        };
        let request = sophia_protocol::RoutedInputRequest {
            serial: event.serial,
            seat: event.seat,
            device: event.device,
            time_msec: event.time_msec,
            target_surface: lease.target_surface,
            global_position: event.global_position.expect("validated spatial input"),
            local_position,
            kind: event.kind,
        };
        let route_lease = application_route_lease_for_request(
            &request,
            client_routes,
            state,
            Some(projection.output),
            projection.epoch,
        )?;
        let delivery = XAuthorityInputDeliveryId::from_raw(*next_delivery);
        *next_delivery = next_delivery
            .checked_add(1)
            .ok_or("live-session input delivery ID exhausted")?;
        if !route_bounded_input(
            sender,
            XAuthorityRoutedInput {
                request,
                route_lease,
                delivery: Some(delivery),
                mode: XAuthorityRoutedInputMode::Deliver,
                origin: sophia_x_authority::XAuthorityRoutedInputOrigin::Physical,
            },
            sophia_protocol::CapacityClass::Ordered,
            &mut report.ingress_saturation,
        )? {
            cancel_application_lease(
                state,
                client_routes,
                release_sender,
                held,
                pending.identity,
                now_msec,
            )?;
            break;
        }
        report.pointer_routed += 1;
        match event.kind {
            sophia_protocol::InputEventKind::PointerButton { .. } => {
                report.pointer_buttons_routed += 1;
                report.pointer_button_targets.push(lease.target_surface);
            }
            sophia_protocol::InputEventKind::PointerAxis { .. } => {
                report.pointer_axes_routed += 1;
                report.pointer_axis_targets.push(lease.target_surface);
            }
            _ => {}
        }
        report.deliveries.push(delivery);
    }
    Ok(())
}

pub(super) fn record_application_lease_refusal(reason: &'static str) {
    crate::session_println!("sophia_live_input_lease schema=2 status=refused reason={reason}");
}

pub(super) fn application_lease_refusal_reason(
    error: sophia_engine::ApplicationRouteLeaseError,
) -> &'static str {
    use sophia_engine::ApplicationRouteLeaseError::*;
    match error {
        OutsideScope => "outside_scope",
        StalePresentation => "target_evidence",
        WrongDevice => "device",
        StaleControlEpoch => "control_epoch",
        StaleAuthoritySession => "authority_session",
        NotRoutable(_) | InvalidPhase => "readiness",
        _ => "identity",
    }
}
