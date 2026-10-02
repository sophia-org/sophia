use super::*;

#[derive(Default)]
pub(super) struct ExplicitPointerGrabQueue {
    applied: Option<TransactionId>,
    requests: VecDeque<sophia_x_authority::XAuthorityExplicitPointerGrabRequest>,
}

impl ExplicitPointerGrabQueue {
    pub(super) fn account(&mut self, batch: &XAuthorityObservedTransactionBatch) {
        // Only actual socket observations can certify a socket prerequisite.
        // WM coordinator ticks share TransactionId but are not this stream.
        if batch.client.is_some() {
            self.applied = Some(self.applied.map_or(batch.transaction, |prior| prior.max(batch.transaction)));
        }
    }

    fn prerequisite_applied(&self, prerequisite: Option<TransactionId>) -> bool {
        prerequisite.is_none_or(|required| self.applied.is_some_and(|applied| applied >= required))
    }
}


#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ExplicitPointerGrabControlReport {
    pub prepared: usize,
    pub activated: usize,
    pub released: usize,
    pub aborted: usize,
    pub rejected: usize,
    pub deferred: usize,
    pub cancelled: usize,
}

fn explicit_pointer_grab_rejection(
    error: sophia_engine::ApplicationRouteLeaseError,
) -> sophia_x_authority::XAuthorityExplicitPointerGrabRejection {
    match error {
        sophia_engine::ApplicationRouteLeaseError::SeatAlreadyOwned => {
            sophia_x_authority::XAuthorityExplicitPointerGrabRejection::AlreadyOwned
        }
        sophia_engine::ApplicationRouteLeaseError::NoLease
        | sophia_engine::ApplicationRouteLeaseError::IdentityMismatch
        | sophia_engine::ApplicationRouteLeaseError::StaleAuthoritySession
        | sophia_engine::ApplicationRouteLeaseError::StaleControlEpoch
        | sophia_engine::ApplicationRouteLeaseError::StalePresentation => {
            sophia_x_authority::XAuthorityExplicitPointerGrabRejection::Stale
        }
        _ => sophia_x_authority::XAuthorityExplicitPointerGrabRejection::Invalid,
    }
}

pub(super) fn drain_explicit_pointer_grab_controls(
    owner: &sophia_x_authority::XAuthorityExplicitPointerGrabOwner,
    state: &mut ApplicationRouteLeaseState,
    pending: &mut ExplicitPointerGrabQueue,
    layout: &PersistentLiveLayout,
    held_input: &mut PendingLeaseInput,
    release_sender: &dyn SessionSender<XAuthorityRouteLeaseRelease>,
    seat_owned: bool,
    focus: &InputFocusState,
    seat: SeatId,
    now_msec: u64,
) -> Result<ExplicitPointerGrabControlReport, Box<dyn std::error::Error>> {
    let mut report = ExplicitPointerGrabControlReport::default();
    let client_routes = &layout.client_routes;
    while let Ok(request) = owner.try_recv() {
        pending.requests.push_back(request);
    }
    // Visit each request once. An unmet prerequisite never blocks a release
    // behind it or prevents the owner from consuming the authority queue.
    for _ in 0..pending.requests.len() {
        let request = pending.requests.pop_front().expect("bounded pending request");
        if owner.is_cancelled(request.id)? || Instant::now() >= request.deadline {
            if let Some(identity) = explicit_request_identity(request.kind) {
                cancel_application_lease(state, client_routes, release_sender, held_input, identity, now_msec)?;
            }
            let _ = owner.respond(request.id, sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Rejected(
                sophia_x_authority::XAuthorityExplicitPointerGrabRejection::Stale,
            ))?;
            report.cancelled += 1;
            continue;
        }
        if let sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind::Prepare { after_observation, control_epoch, .. } = request.kind {
            if control_epoch != state.control_epoch() {
                let _ = owner.respond(request.id, sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Rejected(
                    sophia_x_authority::XAuthorityExplicitPointerGrabRejection::Stale,
                ))?;
                report.rejected += 1;
                continue;
            }
            if !pending.prerequisite_applied(after_observation) {
                pending.requests.push_back(request);
                report.deferred += 1;
                continue;
            }
        }
        let admission = request.admission;
        let response = match request.kind {
            sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind::Prepare {
                anchor,
                replaces,
                ..
            } => {
                if seat_owned {
                    let _ = owner.respond(request.id, sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Rejected(
                        sophia_x_authority::XAuthorityExplicitPointerGrabRejection::AlreadyOwned,
                    ))?;
                    report.rejected += 1;
                    continue;
                }
                let surface = match anchor {
                    sophia_x_authority::XAuthorityExplicitPointerGrabAnchor::Surface(surface) => {
                        (client_routes.admission_for_surface(surface) == Some(admission) && layout.input_eligible(surface))
                            .then_some(surface)
                    }
                    sophia_x_authority::XAuthorityExplicitPointerGrabAnchor::AdmissionDefault => {
                        focus
                            .focused_surface(seat)
                            .filter(|surface| {
                                client_routes.admission_for_surface(*surface) == Some(admission)
                                    && layout.input_eligible(*surface)
                            })
                            .or_else(|| {
                                client_routes
                                    .surfaces_for_admission(admission)
                                    .into_iter()
                                    .find(|surface| {
                                        layout.input_eligible(*surface)
                                    })
                            })
                    }
                };
                let Some(surface) = surface else {
                    let reason = match anchor {
                        sophia_x_authority::XAuthorityExplicitPointerGrabAnchor::Surface(surface)
                            if client_routes.admission_for_surface(surface) != Some(admission) => "anchor_admission",
                        sophia_x_authority::XAuthorityExplicitPointerGrabAnchor::Surface(surface)
                            if !layout.mapped_surfaces.contains(&surface) => "anchor_unmapped",
                        sophia_x_authority::XAuthorityExplicitPointerGrabAnchor::Surface(_) => "anchor_owner",
                        sophia_x_authority::XAuthorityExplicitPointerGrabAnchor::AdmissionDefault => "no_anchor",
                    };
                    crate::session_println!("sophia_live_explicit_pointer_grab schema=2 status=rejected reason={reason}");
                    report.rejected = report.rejected.saturating_add(1);
                    owner.respond(
                        request.id,
                        sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Rejected(
                            sophia_x_authority::XAuthorityExplicitPointerGrabRejection::NotViewable,
                        ),
                    )?;
                    continue;
                };
                let candidate = ApplicationRouteLeaseCandidate {
                    seat,
                    origin: sophia_engine::ApplicationRouteLeaseOrigin::ExplicitPointer,
                    target_surface: surface,
                    admission: admission.client_id,
                    scope: ApplicationRouteScope {
                        profile: admission.namespace.profile,
                        authority: admission.namespace.id,
                    },
                    authority_session_epoch: admission.auth_provenance.session_generation,
                    binding: sophia_engine::ApplicationRouteLeaseBinding::AwaitingPresentation {
                        pinned_output: None,
                        deadline_msec: now_msec.saturating_add(sophia_engine::POINTER_FOCUS_HANDOFF_TIMEOUT_MSEC),
                    },
                    initiating_device: None,
                    initiating_button: None,
                };
                let result = match replaces {
                    Some(identity) => state.replace_explicit_provisional(identity, candidate),
                    None => state.begin_provisional(candidate),
                };
                match result {
                    Ok(lease) => {
                        report.prepared = report.prepared.saturating_add(1);
                        sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Prepared(
                            lease.identity,
                        )
                    }
                    Err(error) => {
                        report.rejected = report.rejected.saturating_add(1);
                        sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Rejected(
                            explicit_pointer_grab_rejection(error),
                        )
                    }
                }
            }
            sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind::Activate {
                identity,
            } => {
                let result = state
                    .lease(identity.seat)
                    .filter(|lease| {
                        lease.identity == identity
                            && lease.origin
                                == sophia_engine::ApplicationRouteLeaseOrigin::ExplicitPointer
                            && lease.admission == admission.client_id
                    })
                    .ok_or(sophia_engine::ApplicationRouteLeaseError::IdentityMismatch)
                    .and_then(|lease| {
                        state.confirm(
                            identity,
                            lease.target_surface,
                            admission.client_id,
                            admission.auth_provenance.session_generation,
                        )
                    });
                match result {
                    Ok(_) => {
                        report.activated = report.activated.saturating_add(1);
                        sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Activated
                    }
                    Err(error) => {
                        report.rejected = report.rejected.saturating_add(1);
                        sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Rejected(
                            explicit_pointer_grab_rejection(error),
                        )
                    }
                }
            }
            sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind::BeginRelease {
                identity,
            } => match state.request_exact_release(identity, admission.client_id, now_msec) {
                Ok(_) => sophia_x_authority::XAuthorityExplicitPointerGrabResponse::ReleaseReady,
                Err(error) => {
                    report.rejected = report.rejected.saturating_add(1);
                    sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Rejected(
                        explicit_pointer_grab_rejection(error),
                    )
                }
            },
            sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind::FinishRelease {
                identity,
            } => match state.acknowledge_release(identity, admission.client_id) {
                Ok(_) => {
                    report.released = report.released.saturating_add(1);
                    sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Released
                }
                Err(error) => {
                    report.rejected = report.rejected.saturating_add(1);
                    sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Rejected(
                        explicit_pointer_grab_rejection(error),
                    )
                }
            },
            sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind::Abort { identity } => {
                if state.lease(identity.seat).is_some_and(|lease| {
                    lease.identity == identity && lease.admission == admission.client_id
                        && lease.authority_session_epoch == admission.auth_provenance.session_generation
                }) {
                    cancel_application_lease(state, client_routes, release_sender, held_input, identity, now_msec)?;
                    report.aborted += 1;
                    sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Aborted
                } else {
                    report.rejected += 1;
                    sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Rejected(
                        sophia_x_authority::XAuthorityExplicitPointerGrabRejection::Stale,
                    )
                }
            }
        };
        let identity = match response {
            sophia_x_authority::XAuthorityExplicitPointerGrabResponse::Prepared(identity) => Some(identity),
            _ => explicit_request_identity(request.kind),
        };
        if owner.respond(request.id, response)? == sophia_x_authority::XAuthorityExplicitPointerGrabResponseDisposition::Cancelled {
            if let Some(identity) = identity {
                cancel_application_lease(state, client_routes, release_sender, held_input, identity, now_msec)?;
            }
            report.cancelled += 1;
        }
    }
    Ok(report)
}

fn explicit_request_identity(kind: sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind) -> Option<sophia_protocol::ApplicationRouteLeaseIdentity> {
    use sophia_x_authority::XAuthorityExplicitPointerGrabRequestKind::*;
    match kind {
        Prepare { .. } => None,
        Activate { identity } | BeginRelease { identity } | FinishRelease { identity } | Abort { identity } => Some(identity),
    }
}
