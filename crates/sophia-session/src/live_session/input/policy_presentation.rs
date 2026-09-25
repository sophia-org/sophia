struct PolicyPresentedInputRouting<'a> {
    state: &'a sophia_engine::PresentedPolicyState,
    capture: &'a mut sophia_engine::PolicyInputCapture,
    protected_actions: Vec<WmActionId>,
}

fn discard_preempted_policy_release(
    policy: &mut Option<PolicyPresentedInputRouting<'_>>,
    seat: SeatId,
    device: sophia_protocol::DeviceId,
    kind: sophia_protocol::InputEventKind,
) {
    if let Some(policy) = policy.as_mut() {
        policy.capture.discard_release(seat, device, kind);
    }
}

fn record_policy_input(
    disposition: sophia_engine::PolicyInputDisposition,
    report: &mut PhysicalInputRouteReport,
) -> bool {
    match disposition {
        sophia_engine::PolicyInputDisposition::Pass => false,
        sophia_engine::PolicyInputDisposition::Consumed => true,
        sophia_engine::PolicyInputDisposition::Action(action) => {
            report
                .policy_inputs
                .push(PhysicalPolicyInput::PresentedAction(action));
            true
        }
        sophia_engine::PolicyInputDisposition::CapacityExceeded => {
            report.presentation_capacity_exceeded = true;
            true
        }
    }
}

impl PolicyPresentedInputRouting<'_> {
    fn keyboard_needs_shield(
        &self,
        projections: Option<&[sophia_backend_live::LivePresentedInputProjection]>,
    ) -> bool {
        projections
            .into_iter()
            .flatten()
            .filter(|projection| {
                projection.policy_visible || projection.policy_publication.is_some()
            })
            .any(|projection| {
                let Some(stamp) = &projection.policy_publication else {
                    return true;
                };
                let current = self
                    .state
                    .output_receipt(stamp.output)
                    .is_some_and(|receipt| {
                        receipt.connection_epoch == stamp.owner_epoch
                            && receipt.publication_generation == stamp.generation
                            && receipt.output_generation == stamp.output_generation
                    });
                !projection.frame_completed
                    || !current
                    || (self
                        .state
                        .publication()
                        .is_some_and(|(_, p)| p.keyboard_output.is_some())
                        && !self.state.modal_ready(false))
            })
    }
    fn pointer_hit(
        &self,
        output: Option<sophia_protocol::OutputId>,
        point: Option<Point>,
        projections: Option<&[sophia_backend_live::LivePresentedInputProjection]>,
    ) -> sophia_engine::PolicyPointerHit {
        let Some((output, point)) = output.zip(point) else {
            return sophia_engine::PolicyPointerHit::Pass;
        };
        let projection = projections
            .into_iter()
            .flatten()
            .find(|p| p.output == output);
        let Some(projection) = projection else {
            return sophia_engine::PolicyPointerHit::Pass;
        };
        let Some(stamp) = &projection.policy_publication else {
            if projection.policy_visible {
                return sophia_engine::PolicyPointerHit::Blocked;
            }
            return sophia_engine::PolicyPointerHit::Pass;
        };
        let current = self.state.output_receipt(output).is_some_and(|receipt| {
            receipt.connection_epoch == stamp.owner_epoch
                && receipt.publication_generation == stamp.generation
                && receipt.output_generation == stamp.output_generation
        });
        if projection.frame_completed && current {
            self.state.pointer_hit(output, point)
        } else {
            // A revoked/late frame can remain visible. It grants no action and
            // cannot expose an application under its untrusted old geometry.
            sophia_engine::PolicyPointerHit::Blocked
        }
    }
}
