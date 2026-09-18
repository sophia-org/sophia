// A failed writer receipt is immutable. The original endpoint's established
// shutdown supplies a separate recipient fact. Pending capsules keep their
// original completion authority until it accepts their disposition.

#[cfg(unix)]
impl PrivateDeliveryCustody {
    fn owns_terminated_output(&self) -> bool {
        self.pending.is_some()
            || (self.completion.is_some() && self.dispatch == PrivateDispatchPhase::Untaken)
    }

    fn answer_terminated_output(
        &mut self,
        endpoint: &PrivateEndpointIdentity,
        recovery: &InputRecovery,
    ) {
        let Some(completion) = self.completion.as_ref() else {
            return;
        };
        let (delivery, client, original) = match self.pending.as_ref() {
            Some(PrivatePendingDelivery::Capsule(capsule)) => (
                Some(capsule.delivery()),
                capsule.client(),
                capsule.endpoint(),
            ),
            Some(PrivatePendingDelivery::Unwrapped { emission, .. }) => (
                emission.delivery(),
                XServerFrontendClientId::from_raw(emission.connection().recipient),
                emission.endpoint(),
            ),
            None => return,
        };
        let Some(delivery) = delivery else {
            return;
        };
        if !endpoint.matches(original) || !original.ordered_termination() {
            return;
        }
        let finalizer = finalizer_from_held(recovery, completion, delivery, client);
        if finalizer.finalize(XAuthorityInputDeliveryOutcome::ClientDisconnected)
            != PrivateAdjudication::Refused
        {
            // Deferred transfers responsibility to the same terminal authority;
            // live disposal still waits for this exact cell's actual answer.
            self.pending = None;
            self.dispatch = PrivateDispatchPhase::Unrepeatable;
        }
    }
}

#[cfg(unix)]
impl PrivateXServerFrontend {
    fn owes_terminated_recipient(&self) -> bool {
        self.terminal.settling.iter().any(|release| {
            release
                .native
                .as_ref()
                .is_some_and(|native| native.endpoint().ordered_termination())
                && (!release.custody.recipient_termination
                    || release.custody.owns_terminated_output()
                    || release
                        .press_custody
                        .as_ref()
                        .is_some_and(PrivateDeliveryCustody::owns_terminated_output))
        })
    }

    fn settle_one_terminated_recipient(&mut self) -> PrivateReceiptStep {
        let count = self.terminal.settling.len();
        if count == 0 {
            return PrivateReceiptStep::Unanswered;
        }
        let index = self.terminal.recipient_termination_cursor % count;
        self.terminal.recipient_termination_cursor = (index + 1) % count;
        let release = &mut self.terminal.settling[index];
        let Some(native) = release.native.as_mut() else {
            return PrivateReceiptStep::Unanswered;
        };
        if native.incarnation() != Some(release.incarnation)
            || !native.endpoint().ordered_termination()
        {
            return PrivateReceiptStep::Unanswered;
        }
        let endpoint = native.endpoint().clone();
        let grant = native.grant();
        let incarnation = release.incarnation;
        let attempt = release.custody.attempt;
        let mut step = PrivateReceiptStep::Unanswered;
        if !release.custody.recipient_termination {
            let answer = self.controller.under_common_as_origin(|authority, issuer| {
                if !authority.reconciliation_record_present(issuer, incarnation)? {
                    return Ok(true);
                }
                let _permission =
                    authority.native_reconciliation(issuer, Some(grant), incarnation)?;
                let bit = sophia_input_authority::SettlementBit {
                    native_reconciled: false,
                    recipient_settled: true,
                };
                if let Some(attempt) = attempt {
                    authority.finish_attempt(issuer, attempt, bit)?;
                }
                authority.settle(issuer, Some(grant), incarnation.input, incarnation, bit)?;
                authority
                    .reconciliation_record_present(issuer, incarnation)
                    .map(|present| !present)
            });
            let Ok(Ok(debt_settled)) = answer else {
                return PrivateReceiptStep::Unanswered;
            };
            release.custody.recipient_termination = true;
            if let Some(press) = &mut release.press_custody {
                press.recipient_termination = true;
            }
            release.custody.attempt = None;
            if self
                .terminal
                .attempt_custody
                .is_some_and(|held| Some(held.token) == attempt)
            {
                self.terminal.attempt_custody = None;
            }
            step = PrivateReceiptStep::Settled { debt_settled };
        }
        // Only locally owned output is adjudicated here. Enqueued or
        // indeterminate handovers still owe their original writer's answer.
        if release.custody.dispatch == PrivateDispatchPhase::Untaken
            && let Some(emission) = native.take_release_emission()
        {
            Self::stow_press_capsule(
                &mut release.custody,
                emission,
                &self.broker.registry.input_recovery,
                release.reached.client,
            );
        }
        release
            .custody
            .answer_terminated_output(&endpoint, &self.broker.registry.input_recovery);
        if let Some(press) = &mut release.press_custody {
            if press.dispatch == PrivateDispatchPhase::Untaken
                && let Some(emission) = native.take_press_emission()
            {
                Self::stow_press_capsule(
                    press,
                    emission,
                    &self.broker.registry.input_recovery,
                    release.reached.client,
                );
            }
            press.answer_terminated_output(&endpoint, &self.broker.registry.input_recovery);
        }
        step
    }
}
