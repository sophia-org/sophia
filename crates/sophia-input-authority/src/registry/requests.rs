//! Retained completion cells. The notifier is only a wakeup: its capacity and
//! timing cannot discard a completed operation or allow a grant slot to recycle.
use super::*;

/// A queued operation's identity, unrelated to X sequence-number wraparound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestToken {
    grant: GrantId,
    incarnation: u64,
}

/// Internal processing completion, never an XTEST success reply or evidence
/// of recipient application processing. Refusal does not erase existing debt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestCompletion {
    Processed,
    Refused(RegistrationError),
    /// Authority application may have occurred; this is not a policy refusal.
    FailedAfterApplication(RegistrationError),
    Cancelled,
}

/// Whether a guarded adapter operation finished or remains effect-free and
/// ineligible. Deferral preserves the original reservation, never its credit
/// as a newly accepted request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionDisposition {
    Complete,
    Defer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestExecution {
    Completed(RequestCompletion),
    /// No permit operation was attempted and no completion was published.
    /// The caller must retain the same request custody until it is eligible
    /// or receives a real terminal disposition.
    Deferred,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RequestCell {
    token: RequestToken,
    capability: DeviceCapability,
    context: ExecutionContext,
    pub(super) completion: Option<RequestCompletion>,
}

/// A final-execution view. It exposes one input transition, not issuer control,
/// so a callback cannot revoke/reissue its own grant in the middle of execution.
/// The outer caller holds the common guard throughout this callback and may
/// acquire only later-ranked adapter guards. No waiting is permitted here.
pub struct ExecutionPermit<'a> {
    authority: &'a mut AuthorityInstance,
    token: RequestToken,
    capability: DeviceCapability,
    context: ExecutionContext,
    consumed: bool,
    applied: bool,
}

impl ExecutionPermit<'_> {
    /// The exact reservation that authorized this guarded operation. Naming
    /// it does not grant execution or completion-consumption rights.
    pub fn request_token(&self) -> RequestToken {
        self.token
    }

    /// The authority that validated this exact executing request. This read
    /// grants no mutation rights and lets an adapter reject foreign native
    /// projections before it applies a ledger or projection effect.
    pub fn identity(&self) -> super::AuthorityIdentity {
        super::AuthorityIdentity(self.authority.uid)
    }

    /// Before a non-ledger effect such as pointer movement, mark its commit
    /// boundary. An error after this point cannot be reported as effect-free.
    pub fn begin_external_effect(&mut self) -> Result<(), RegistrationError> {
        self.consume()?;
        self.applied = true;
        Ok(())
    }

    pub fn source(&self) -> SourceId {
        self.capability.source()
    }
    pub fn context(&self) -> ExecutionContext {
        self.context
    }

    fn consume(&mut self) -> Result<(), RegistrationError> {
        if self.consumed {
            return Err(RegistrationError::RequestConsumed);
        }
        self.consumed = true;
        Ok(())
    }

    pub fn press(&mut self, input: Input, to: Recipient) -> Result<Applied, RegistrationError> {
        self.consume()?;
        let result = self
            .authority
            .apply_press(self.capability.source(), input, to)?;
        self.applied = true;
        Ok(result)
    }

    pub fn release(&mut self, input: Input) -> Result<ReleaseOutcome, RegistrationError> {
        self.consume()?;
        self.authority.check_input(input)?;
        let result = self
            .authority
            .release_inner(self.capability.source(), input);
        self.applied = result != ReleaseOutcome::NotHeld;
        Ok(result)
    }
}

impl AuthorityInstance {
    /// Reserve before enqueue. Original context is retained through delay and
    /// thaw; final execution cannot substitute a newer generation/publication.
    pub fn reserve_request(
        &mut self,
        submit: &SubmitHandle,
        capability: DeviceCapability,
        context: ExecutionContext,
    ) -> Result<RequestToken, RegistrationError> {
        self.validate_execution(submit, capability, context)?;
        if self.grants[capability.grant.slot].request.is_some() {
            return Err(RegistrationError::Capacity(CapacityError::NoCompletionCell));
        }
        let token = RequestToken {
            grant: capability.grant,
            incarnation: self.identity()?,
        };
        self.grants[capability.grant.slot].request = Some(RequestCell {
            token,
            capability,
            context,
            completion: None,
        });
        Ok(token)
    }

    fn request_cell(&self, token: RequestToken) -> Result<RequestCell, RegistrationError> {
        self.grant(token.grant)?
            .request
            .filter(|cell| cell.token == token)
            .ok_or(RegistrationError::StaleRequest)
    }

    /// Final validation, native application and completion publication all occur
    /// inside the caller's common guard. The callback resolves adapter state at
    /// this boundary, not when the request was queued. Panicking is an executor
    /// failure; the private instance must stop rather than retry an uncertain
    /// operation. The watchdog, notifier and adapter outputs live outside here.
    pub fn execute_reserved<F>(
        &mut self,
        issuer: &IssuerHandle,
        token: RequestToken,
        current_connection: ConnectionIdentity,
        operation: F,
    ) -> Result<RequestCompletion, RegistrationError>
    where
        F: FnOnce(&mut ExecutionPermit<'_>) -> Result<(), RegistrationError>,
    {
        match self.execute_reserved_or_defer(issuer, token, current_connection, |permit| {
            operation(permit).map(|()| ExecutionDisposition::Complete)
        })? {
            RequestExecution::Completed(completion) => Ok(completion),
            RequestExecution::Deferred => unreachable!("this callback never defers"),
        }
    }

    /// Validate and optionally execute one retained request under the caller's
    /// common guard. The adapter must hold its native eligibility guards from
    /// the freeze decision through any effect; returning Defer is legal only
    /// before consuming the permit. Resumption validates the original cell's
    /// context again, without a replacement token or publication stamp.
    ///
    /// An interrupted callback remains the adapter owner's uncertain work;
    /// an absent completion alone never authorizes retry after interruption.
    pub fn execute_reserved_or_defer<F>(
        &mut self,
        issuer: &IssuerHandle,
        token: RequestToken,
        current_connection: ConnectionIdentity,
        operation: F,
    ) -> Result<RequestExecution, RegistrationError>
    where
        F: FnOnce(&mut ExecutionPermit<'_>) -> Result<ExecutionDisposition, RegistrationError>,
    {
        self.check_issuer(issuer)?;
        let cell = self.request_cell(token)?;
        if current_connection != cell.capability.connection {
            return Err(RegistrationError::WrongConnection);
        }
        if let Some(completed) = cell.completion {
            return Ok(RequestExecution::Completed(completed));
        }
        let submit = SubmitHandle::new(self.uid, self.binding);
        let mut applied = false;
        let result = self
            .validate_execution(&submit, cell.capability, cell.context)
            .and_then(|_| {
                let mut permit = ExecutionPermit {
                    authority: self,
                    token,
                    capability: cell.capability,
                    context: cell.context,
                    consumed: false,
                    applied: false,
                };
                let outcome = operation(&mut permit);
                applied = permit.applied;
                if outcome == Ok(ExecutionDisposition::Defer) && (permit.consumed || permit.applied)
                {
                    Err(RegistrationError::RequestConsumed)
                } else {
                    outcome
                }
            });
        let completion = match result {
            Ok(ExecutionDisposition::Defer) => return Ok(RequestExecution::Deferred),
            Ok(ExecutionDisposition::Complete) => RequestCompletion::Processed,
            Err(error) if applied => RequestCompletion::FailedAfterApplication(error),
            Err(error) => RequestCompletion::Refused(error),
        };
        // No allocation, channel send or other fallible publication after the
        // effect. Control operations cannot interleave while this guard is held.
        self.grants[token.grant.slot]
            .request
            .as_mut()
            .expect("reserved cell")
            .completion = Some(completion);
        Ok(RequestExecution::Completed(completion))
    }

    /// A revoked caller may still consume its own completion. Consumption does
    /// not authorize another request, and an old token cannot consume a new one.
    pub fn take_completion(
        &mut self,
        submit: &SubmitHandle,
        token: RequestToken,
        current_connection: ConnectionIdentity,
    ) -> Result<Option<RequestCompletion>, RegistrationError> {
        self.check_submit(submit)?;
        let cell = self.request_cell(token)?;
        if current_connection != cell.capability.connection {
            return Err(RegistrationError::WrongConnection);
        }
        if cell.completion.is_some() {
            self.grants[token.grant.slot].request = None;
        }
        Ok(cell.completion)
    }

    /// Issuer cleanup after the connection has departed. For pending work this
    /// invalidates the token before it can execute; completed effects and their
    /// debt are not undone. A late worker cannot replay an abandoned operation.
    pub fn abandon_request(
        &mut self,
        issuer: &IssuerHandle,
        token: RequestToken,
    ) -> Result<(), RegistrationError> {
        self.check_issuer(issuer)?;
        self.request_cell(token)?;
        self.grants[token.grant.slot].request = None;
        Ok(())
    }
}
