use super::*;

fn invalid() -> ShellClientError {
    ShellClientError::Protocol("native observation correlation")
}

impl NativeLauncherLifecycle {
    pub(super) fn apply(
        &mut self,
        value: &NativeLauncherObservation,
    ) -> Result<(), ShellClientError> {
        match value {
            NativeLauncherObservation::Published(value) => {
                let generation = match value.object {
                    ShellFileKind::Catalog => &mut self.catalog_announced,
                    ShellFileKind::Outputs => &mut self.facts_announced,
                    _ => return Err(invalid()),
                };
                if value.generation <= *generation {
                    return Err(invalid());
                }
                *generation = value.generation;
            }
            NativeLauncherObservation::Catalog(_, value) => {
                if value.connection_epoch != self.limits.grant.connection_epoch
                    || value.generation == 0
                    || value.generation < self.catalog
                {
                    return Err(invalid());
                }
                self.catalog = value.generation;
            }
            NativeLauncherObservation::Content(transaction, record) => {
                self.content(*transaction, record)?
            }
            NativeLauncherObservation::Record(value) => self.native(value)?,
        }
        Ok(())
    }

    fn content(
        &mut self,
        tx: TransactionId,
        record: &ShellContentRecord,
    ) -> Result<(), ShellClientError> {
        let grant = match record {
            ShellContentRecord::Limits(v) => v.grant,
            ShellContentRecord::OutputFacts(v) => v.grant,
            ShellContentRecord::CandidateOutcome(v) => v.grant,
            ShellContentRecord::AllocationResult(v) => v.grant,
            ShellContentRecord::ResourceStatus(v) => v.grant,
            ShellContentRecord::ResourceReleased(v) => v.grant,
            ShellContentRecord::FramePermit(v) => v.grant,
            ShellContentRecord::Action(v) => v.grant,
            _ => return Err(invalid()),
        };
        self.grant(grant)?;
        match record {
            ShellContentRecord::Action(v) => self.observe_action(v),
            ShellContentRecord::AllocationResult(v) => self.allocation_result(v)?,
            ShellContentRecord::ResourceStatus(v) => self.resource_status(tx, v)?,
            ShellContentRecord::ResourceReleased(v) => self.free_resource(v.resource),
            ShellContentRecord::FramePermit(v) => self.frame_permit(v)?,
            ShellContentRecord::OutputFacts(v) => {
                if v.facts_generation == 0 || v.facts_generation < self.facts {
                    return Err(invalid());
                }
                self.facts = v.facts_generation;
            }
            ShellContentRecord::CandidateOutcome(v) => {
                let mut pending = self.pending.ok_or_else(invalid)?;
                if pending.transaction != tx
                    || pending.generation != v.candidate_generation
                    || pending.output != v.output
                    || !(1..=4).contains(&v.kind)
                    || (v.presentation_epoch != 0) != (v.kind == 2)
                    || (v.kind <= 2 && v.reason != 0)
                {
                    return Err(invalid());
                }
                if v.kind == 1 {
                    return Ok(());
                }
                if v.kind == 2
                    && self
                        .opening
                        .is_some_and(|opening| opening.opening == pending.opening)
                {
                    pending.presentation = v.presentation_epoch;
                    self.shown = Some(pending);
                }
                self.pending = None;
                self.pacing.consumed = None;
            }
            _ => {}
        }
        Ok(())
    }

    fn focus_matches(&self, b: &NativeLauncherBinding) -> bool {
        let (Some(opening), Some(shown)) = (self.opening, self.shown) else {
            return false;
        };
        !self.launched
            && b.opening == opening.opening
            && shown.opening == opening.opening
            && b.output == shown.output
            && b.allocation == shown.allocation
            && b.catalog_generation == shown.catalog
            && b.catalog_generation == opening.catalog_generation
            && b.candidate_generation == shown.generation
            && b.presentation_epoch == shown.presentation
            && b.interaction_generation == shown.interaction
            && b.state_revision == shown.revision
            && b.state_revision == self.revision
            && b.focus_lease != 0
    }

    fn native(&mut self, value: &ShellFileNativeLauncherRecord) -> Result<(), ShellClientError> {
        use ShellNativeLauncherRecord::*;
        let grant = match &value.record {
            Opening(v) => v.grant,
            Focus(v) => v.grant,
            FocusRevoked(v) => v.binding.grant,
            Input(v) => v.event.binding.grant,
            ActivationOutcome(v) => v.activation.event.binding.grant,
            Closed(v) => v.grant,
            _ => return Err(invalid()),
        };
        self.grant(grant)?;
        match &value.record {
            Opening(v) => {
                if self.opening.is_some() || v.opening <= self.last_opening || v.state_revision != 1
                {
                    return Err(invalid());
                }
                self.opening = Some(*v);
                self.last_opening = v.opening;
                self.revision = v.state_revision;
                self.focus = None;
                self.shown = None;
                self.launched = false;
            }
            Focus(v) => {
                if self
                    .opening
                    .is_some_and(|opening| opening.opening == v.opening)
                {
                    // No lease monotonicity is specified: an exact later binding supersedes.
                    self.focus = self.focus_matches(v).then_some(*v);
                }
            }
            FocusRevoked(v) => {
                if self.focus == Some(v.binding) {
                    self.focus = None;
                }
            }
            Input(v) => {
                let current = self
                    .opening
                    .is_some_and(|opening| opening.opening == v.event.binding.opening);
                let bound = current && self.focus == Some(v.event.binding);
                let mut status = NativeInputStatus {
                    current,
                    ack_due_ms: (v.issued_mono_usec / 1000)
                        .saturating_add(u64::from(self.limits.action_ack_timeout_ms)),
                    ..Default::default()
                };
                if v.kind != NativeLauncherInputKind::Accept {
                    if current && v.event.state_revision > self.revision {
                        self.revision = v.event.state_revision;
                        status.editable = bound;
                    }
                } else {
                    status.can_activate = bound
                        && !self.launched
                        && self.activation.is_none()
                        && self.catalog_current()
                        && v.event.state_revision == self.revision
                        && v.event.binding.state_revision == self.revision
                        && self.shown.is_some_and(|shown| {
                            shown.selected != 0
                                && shown.generation == v.event.binding.candidate_generation
                                && shown.presentation == v.event.binding.presentation_epoch
                        });
                }
                self.input = Some((v.clone(), status));
            }
            ActivationOutcome(v) => {
                let (tx, activation, _) = self.activation.ok_or_else(invalid)?;
                if value.transaction != tx || v.activation != activation {
                    return Err(invalid());
                }
                self.activation = None;
                if v.status == 1
                    && self
                        .opening
                        .is_some_and(|opening| opening.opening == activation.event.binding.opening)
                {
                    self.launched = true;
                    self.focus = None;
                }
            }
            Closed(v) => {
                if self
                    .opening
                    .is_none_or(|opening| opening.opening != v.opening)
                {
                    return Err(invalid());
                }
                self.opening = None;
                self.focus = None;
                self.shown = None;
                if self.allocation.granted.is_some() && self.allocation.opening == v.opening {
                    self.allocation.orphaned = true;
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
}
