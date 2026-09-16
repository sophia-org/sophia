// Where admission and revocation cross into the private boundary.
//
// Split by subject from the authority itself: the controller answers who may
// act on the instance, and this answers whether the client a request names is
// still admitted at the moment it would apply.

/// What the private boundary knows about one admitted client.
///
/// The admission identity is kept, not just the generation. A replacement
/// admission for the same client under the same session generation is a
/// different admission, and a grant issued under the old one must not become
/// current again because the numbers around it happen to match.
#[cfg(unix)]
#[derive(Debug, Clone)]
struct PrivateAdmissionBinding {
    lifecycle: Option<PrivateLifecycleGate>,
    admission: sophia_protocol::ClientAdmissionId,
    namespace: NamespaceId,
    generation: u64,
    /// Grants issued while this binding was current.
    ///
    /// Held so revocation can retire exactly what this admission authorised,
    /// rather than sweeping whatever the authority happens to hold. Storage is
    /// reserved when the binding is made, so recording a grant never allocates
    /// after the grant exists -- an allocation there could fail with the grant
    /// already issued and nothing yet recording it.
    grants: Vec<sophia_input_authority::GrantId>,
    /// Whether this binding has been closed.
    ///
    /// Marked before its grants are retired, and the entry stays until they
    /// are. Moving the binding out to retire from would put the remaining
    /// inventory in a local, where an unwind part-way through destroys it
    /// along with the record of what was still owed.
    closed: bool,
}

/// How many grant records one binding may hold.
///
/// A policy chosen here, set to the planned authority's grant count because
/// that is the shape the approved plan fixes -- **not** a reading of whatever
/// capacity the instance in hand was built with. The distinction is
/// observable: an authority built larger still gets this many records per
/// binding, and an authority built smaller refuses on its own capacity before
/// this is reached. Naming a number locally would have been a policy nobody
/// decided; deriving it from the supplied instance would be a different rule
/// again, and this is neither.
///
/// It bounds *records*, not concurrently live grants. A record leaves only
/// when its grant is retired, so a long-lived binding that churns grants
/// reaches this even though few are live at once. That is deliberate -- the
/// record is what revocation retires against, so a binding that has forgotten
/// which grants it authorised is the failure this bound prevents.
///
/// It is also per binding rather than per authority: a binding refused here
/// has not consumed the authority's remaining slots, which stay available to
/// another binding.
#[cfg(unix)]
const PRIVATE_BINDING_GRANTS: usize = sophia_input_authority::Capacity::PLANNED.grants;

/// What a revocation closed and retired.
///
/// Two counts, because they answer different questions. Closing a binding
/// always denies further work; retiring a grant is cleanup behind it. A
/// binding that never issued a grant, or whose grants were already retired,
/// closes with nothing to retire -- so a zero here is not evidence that
/// nothing was closed.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrivateRevocation {
    pub closed: usize,
    pub retired: usize,
}

#[cfg(unix)]
#[derive(Default)]
struct PrivateAdmissionBindings {
    bound: BTreeMap<XServerFrontendClientId, PrivateAdmissionBinding>,
}

/// The one way admission and revocation reach the private boundary.
///
/// Every path here takes common first and the bindings beneath it. That
/// ordering is the mechanism, not decoration: an execution already holding
/// common finishes before a revocation can cross, and once a revocation has
/// returned, no later execution can find the binding it removed. Nothing here
/// asks a caller to assert currency, and nothing caches an answer to be
/// checked later.
///
/// THE EDGE THIS EXISTS TO AVOID is taking common while the client table is
/// held. A caller holding the table and then reaching in here would let a
/// revocation wait behind a table that something else is waiting to cross this
/// boundary to release. The other nesting -- the table taken beneath common,
/// which is what this boundary itself does -- is the allowed one. That edge is
/// forbidden outright, and the ordered producers observe it: each releases the
/// client table before anything takes common.
///
/// NOT EVERY REGISTRY-OWNED LOCK IS THAT EDGE, and the blanket wording this
/// once carried no longer describes the callers. Promotion holds a
/// registration's endpoint gate and its payload storage and then reaches in
/// here, through the endpoint lookup; the whole order on that path is the
/// endpoint gate, then the payload storage, then common, then the bindings,
/// then the client table -- which is acyclic against the producers and the
/// teardown, because none of them holds a later lock while waiting for an
/// earlier one.
///
/// It is not free. A promotion waiting for common keeps that endpoint's gate
/// and payload occupied, so handovers and closing for THAT connection wait
/// behind it. Nothing here bounds how long any of that takes.
#[cfg(unix)]
#[derive(Clone)]
pub struct PrivateAdmissionParticipant {
    controller: PrivateAuthorityController,
    bindings: Arc<Mutex<PrivateAdmissionBindings>>,
    lifecycle: Arc<std::sync::OnceLock<std::sync::Weak<PrivateLifecycleCore>>>,
}

/// Why the participant refused.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateAdmissionRefusal {
    /// Nothing is bound for this client here. Not a lookup that failed: a
    /// client the private boundary never admitted, or one whose admission has
    /// been revoked.
    NotAdmitted,
    /// Bound, but to a different admission than the one being named. A new
    /// admission does not make an old grant current.
    DifferentAdmission,
    /// Already bound. Re-admitting would silently replace the identity live
    /// grants were issued under.
    AlreadyAdmitted,
    /// This binding already holds as many grant records as it may. Refused
    /// before a grant is issued, so nothing exists that the binding could not
    /// then account for.
    GrantRecordsExhausted,
    /// The configured frontend admission capacity still owns every lifecycle
    /// record, including closures whose cleanup remains unresolved.
    ClientRecordsExhausted,
    /// The authority refused, with its own reason kept.
    Authority(sophia_input_authority::RegistrationError),
    /// The boundary could not be reached.
    Unreachable,
}

#[cfg(unix)]
impl PrivateAdmissionParticipant {
    fn new(controller: PrivateAuthorityController) -> Self {
        Self {
            controller,
            bindings: Arc::new(Mutex::new(PrivateAdmissionBindings::default())),
            lifecycle: Arc::new(std::sync::OnceLock::new()),
        }
    }

    /// Take common, then the bindings beneath it.
    fn under_boundary<R>(
        &self,
        act: impl FnOnce(
            &mut sophia_input_authority::AuthorityInstance,
            &sophia_input_authority::IssuerHandle,
            &mut PrivateAdmissionBindings,
        ) -> R,
    ) -> Result<R, PrivateAdmissionRefusal> {
        self.controller
            .under_common_as_origin(|authority, issuer| {
                let mut bindings = self
                    .bindings
                    .lock()
                    .map_err(|_| PrivateAdmissionRefusal::Unreachable)?;
                Ok(act(authority, issuer, &mut bindings))
            })
            .map_err(|_| PrivateAdmissionRefusal::Unreachable)?
    }

    /// Admit one client to the private boundary.
    ///
    /// Called before the client's ingress is exposed, so there is no interval
    /// in which work could be accepted for a client this boundary has never
    /// heard of.
    pub fn admit(
        &self,
        client: XServerFrontendClientId,
        admission: sophia_protocol::ClientAdmissionContext,
    ) -> Result<(), PrivateAdmissionRefusal> {
        self.under_boundary(|_authority, _issuer, bindings| {
            if bindings.bound.contains_key(&client) {
                // Replacing in place would leave grants issued under the old
                // identity answering to the new one.
                return Err(PrivateAdmissionRefusal::AlreadyAdmitted);
            }
            let mut bound = PrivateAdmissionBinding {
                lifecycle: None,
                admission: admission.client_id,
                namespace: admission.namespace.id,
                generation: admission.auth_provenance.session_generation,
                grants: Vec::with_capacity(PRIVATE_BINDING_GRANTS),
                closed: false,
            };
            if let Some(owner) = self.lifecycle.get() {
                let owner = owner.upgrade().ok_or(PrivateAdmissionRefusal::Unreachable)?;
                // Refuse before publishing a binding. A full lifecycle table
                // must not accumulate unbounded, closed admission records.
                let gate = PrivateLifecycleOwner { inner: owner }
                    .register_held(client, &bound)
                    .map_err(|error| match error {
                        PrivateLifecycleRefusal::Capacity => PrivateAdmissionRefusal::ClientRecordsExhausted,
                        PrivateLifecycleRefusal::AlreadyOwned => PrivateAdmissionRefusal::AlreadyAdmitted,
                        PrivateLifecycleRefusal::NotAdmitted => PrivateAdmissionRefusal::NotAdmitted,
                        _ => PrivateAdmissionRefusal::Unreachable,
                    })?;
                bound.lifecycle = Some(gate);
            }
            bindings.bound.insert(client, bound);
            Ok(())
        })?
    }

    /// Revoke one exact admission.
    ///
    /// The binding goes first, so nothing can be admitted through it while the
    /// grants behind it are being retired. An interruption after that leaves
    /// the boundary closed with cleanup still owed, which costs capacity; the
    /// other order would leave it open with its grants already gone.
    pub fn revoke_admission(
        &self,
        client: XServerFrontendClientId,
        admission: sophia_protocol::ClientAdmissionId,
    ) -> Result<PrivateRevocation, PrivateAdmissionRefusal> {
        self.under_boundary(|authority, issuer, bindings| {
            let Some(bound) = bindings.bound.get_mut(&client) else {
                return Err(PrivateAdmissionRefusal::NotAdmitted);
            };
            if bound.admission != admission {
                // A delayed revoke naming an admission that has been replaced
                // must not close the replacement.
                return Err(PrivateAdmissionRefusal::DifferentAdmission);
            }
            Ok(close_and_retire(authority, issuer, bindings, client))
        })?
    }

    /// Revoke every admission in one namespace.
    /// Every binding in the namespace closes, whatever it holds.
    ///
    /// A binding that issued nothing, and one whose grants are already
    /// retired, close exactly like any other: closing is what denies further
    /// work, and having nothing left to clean up is not a reason to leave a
    /// namespace admitted.
    pub fn revoke_namespace(
        &self,
        namespace: NamespaceId,
    ) -> Result<PrivateRevocation, PrivateAdmissionRefusal> {
        self.under_boundary(|authority, issuer, bindings| {
            let mut total = PrivateRevocation::default();
            // Taken one at a time rather than listed first. Collecting the
            // matches allocates on a cleanup path, and the list would be a
            // local holding work the inventory no longer describes.
            while let Some(client) = bindings
                .bound
                .iter()
                .find(|(_, bound)| bound.namespace == namespace && !bound.closed)
                .map(|(client, _)| *client)
            {
                let one = close_and_retire(authority, issuer, bindings, client);
                total.closed = total.closed.saturating_add(one.closed);
                total.retired = total.retired.saturating_add(one.retired);
            }
            Ok(total)
        })?
    }

    /// Resume retirement for bindings that were closed with work unresolved.
    ///
    /// A revocation that could not retire everything leaves its binding closed
    /// with the remainder recorded, which denies further work but does not
    /// finish the cleanup. Nothing revisits those on its own: revoking the
    /// namespace again skips them, because they are already closed and closing
    /// is not what they are waiting for.
    ///
    /// So the origin drives this. It retires what it can and removes each
    /// record as its retirement returns, leaving anything still unresolved
    /// exactly where it was for the next attempt.
    pub fn resume_unresolved(&self) -> Result<PrivateRevocation, PrivateAdmissionRefusal> {
        self.under_boundary(|authority, issuer, bindings| {
            let mut total = PrivateRevocation::default();
            while let Some(client) = bindings
                .bound
                .iter()
                .find(|(_, bound)| bound.closed && !bound.grants.is_empty())
                .map(|(client, _)| *client)
            {
                let before = bindings
                    .bound
                    .get(&client)
                    .map_or(0, |bound| bound.grants.len());
                let one = close_and_retire(authority, issuer, bindings, client);
                total.retired = total.retired.saturating_add(one.retired);
                let after = bindings
                    .bound
                    .get(&client)
                    .map_or(0, |bound| bound.grants.len());
                if after >= before {
                    // Nothing moved, so trying again would loop on the same
                    // binding. Left for a later attempt rather than spun on.
                    break;
                }
            }
            Ok(total)
        })?
    }

    /// Issue a reservation role for an admitted client.
    ///
    /// The grant is issued and recorded on the binding in one pass under
    /// common, so there is no moment where a grant exists that revocation
    /// would not find.
    fn issue_role(
        &self,
        submit: sophia_input_authority::SubmitHandle,
        client: XServerFrontendClientId,
        device: sophia_protocol::DeviceId,
    ) -> Result<PrivateReservationRole, PrivateAdmissionRefusal> {
        self.under_boundary(|authority, issuer, bindings| {
            let Some(bound) = bindings.bound.get(&client).filter(|bound| !bound.closed && bound.lifecycle.as_ref().is_none_or(PrivateLifecycleGate::is_open)) else {
                return Err(PrivateAdmissionRefusal::NotAdmitted);
            };
            let connection = sophia_input_authority::ConnectionIdentity {
                recipient: client.raw(),
                connection_generation: bound.generation,
            };
            let admission = bound.admission;
            // Checked before the grant exists. Refusing afterwards would mean
            // a grant this binding could not record, and revocation retires
            // what a binding says it authorised -- so an unrecordable grant is
            // one nothing could ever retire.
            if bound.grants.len() >= PRIVATE_BINDING_GRANTS {
                return Err(PrivateAdmissionRefusal::GrantRecordsExhausted);
            }
            let (grant, generation) = authority
                .issue_grant(issuer, connection)
                .map_err(PrivateAdmissionRefusal::Authority)?;
            // Recorded before the next step, which can fail with the grant
            // already issued. Recording afterwards leaves a window where the
            // grant exists and revocation cannot find it, because revocation
            // retires what a binding says it authorised. The storage was
            // reserved when the binding was made, so this does not allocate.
            bindings
                .bound
                .get_mut(&client)
                .expect("just read")
                .grants
                .push(grant);
            let capability = match authority.allocate_device(issuer, grant, generation, device) {
                Ok(capability) => capability,
                Err(refused) => {
                    // Rolled back exactly: the grant this call issued, not
                    // whatever the binding currently holds. The record is
                    // removed only if the retirement actually happened --
                    // removing it on a failed rollback would forget a grant
                    // that still exists, which is the obligation this record
                    // is for.
                    let retired = matches!(
                        authority.revoke_grant(issuer, grant),
                        Ok(_) | Err(sophia_input_authority::RegistrationError::StaleGeneration)
                    );
                    if retired && let Some(bound) = bindings.bound.get_mut(&client) {
                        bound.grants.retain(|held| *held != grant);
                    }
                    return Err(PrivateAdmissionRefusal::Authority(refused));
                }
            };
            Ok(PrivateReservationRole::new(
                self.controller.clone(),
                submit,
                capability,
                generation,
                connection,
                admission,
                grant,
            ))
        })?
    }

    /// Execute one reserved request against a binding that is current now.
    ///
    /// The binding is read under common and the execution happens before it is
    /// released, so a revocation cannot cross in between. An execution already
    /// under way holds common, which is what makes revocation wait for it
    /// rather than racing it.
    fn execute_current(
        &self,
        outstanding: &PrivateOutstandingRequest,
        client: XServerFrontendClientId,
        act: impl FnOnce(
            &mut sophia_input_authority::ExecutionPermit<'_>,
            &PrivateAdmissionBindings,
        ) -> Result<(), sophia_input_authority::RegistrationError>,
    ) -> Result<
        Result<sophia_input_authority::RequestCompletion, PrivateAuthorityRefusal>,
        PrivateAdmissionRefusal,
    > {
        self.under_boundary(|authority, issuer, bindings| {
            let Some(bound) = bindings.bound.get(&client).filter(|bound| !bound.closed && bound.lifecycle.as_ref().is_none_or(PrivateLifecycleGate::is_open)) else {
                // Revoked, or never admitted here. Refused before any effect.
                return Err(PrivateAdmissionRefusal::NotAdmitted);
            };
            if bound.admission != outstanding.admission() {
                // Readmitted since this grant was issued. A replacement
                // admission does not make an old grant current, whatever the
                // generation says.
                return Err(PrivateAdmissionRefusal::DifferentAdmission);
            }
            let current = sophia_input_authority::ConnectionIdentity {
                recipient: client.raw(),
                connection_generation: bound.generation,
            };
            // The bindings are already held here, so the callback is handed
            // them rather than reaching for them again -- a recipient's own
            // admission generation is evidence it must read under this guard,
            // not evidence the submitter can supply about somebody else.
            let held: &PrivateAdmissionBindings = bindings;
            Ok(authority
                .execute_reserved(issuer, outstanding.token(), current, |permit| {
                    // Written before the caller's work can take effect or
                    // unwind.
                    outstanding.entering();
                    act(permit, held)
                })
                .inspect(|_| outstanding.settled())
                .map_err(PrivateAuthorityRefusal::Authority))
        })?
    }
}

#[cfg(unix)]
impl PrivateAdmissionBindings {
}

/// Close one binding and retire what its admission authorised.
///
/// Closed first, so nothing can be admitted through it while its grants are
/// still going. The binding stays in the inventory throughout and each grant
/// is removed only once its retirement has returned, so an unwind part-way
/// leaves the remainder recorded and closed rather than destroying it with a
/// local. The entry goes only when there is nothing left owed against it.
///
/// Debt is retained by the authority rather than discharged here: a revoked
/// grant's obligations outlive it, which is what makes cleanup possible after
/// the client is gone.
#[cfg(unix)]
fn close_and_retire(
    authority: &mut sophia_input_authority::AuthorityInstance,
    issuer: &sophia_input_authority::IssuerHandle,
    bindings: &mut PrivateAdmissionBindings,
    client: XServerFrontendClientId,
) -> PrivateRevocation {
    let Some(bound) = bindings.bound.get_mut(&client) else {
        return PrivateRevocation::default();
    };
    let closed = usize::from(!bound.closed);
    if let Some(gate) = &bound.lifecycle {
        gate.close();
    }
    bound.closed = true;
    let mut retired = 0usize;
    // Walked from the end so a grant that is removed does not shift the ones
    // still to be visited. Read by copy, never taken: a retirement that does
    // not return leaves its grant exactly where it was.
    let mut index = bound.grants.len();
    while index > 0 {
        index -= 1;
        let grant = bound.grants[index];
        match authority.revoke_grant(issuer, grant) {
            Ok(_debt) => {
                retired = retired.saturating_add(1);
                bound.grants.remove(index);
            }
            // Already gone: this authority issued it and no longer holds it,
            // so there is nothing left to retire and nothing owed for it.
            Err(sophia_input_authority::RegistrationError::StaleGeneration) => {
                bound.grants.remove(index);
            }
            // Anything else is unresolved rather than finished. The grant
            // stays recorded, so what is still owed can be found and retried;
            // dropping it here would buy a tidy count by forgetting an
            // obligation.
            Err(_unresolved) => {}
        }
    }
    if bound.grants.is_empty() {
        // Nothing left owed against it, so the record goes. A binding kept
        // past that point would deny work it no longer has any reason to.
        bindings.bound.remove(&client);
    }
    PrivateRevocation { closed, retired }
}
