// The release a departing source owes, finished by the terminal.
//
// A source that departs while holding an input has its grant revoked, and
// `revoke_grant` retires it and releases every input it held. Where that ends
// the aggregate, the ledger has decided a release and nobody has told the
// keyboard or the recipient: there is no request to carry it, no execution to
// decide it in, and no submitter to answer to. This visit is what finishes
// it, using the same storage, the same writer and the same settlement as a
// release a request asked for.

/// How many delivery turns pass between looks for a departed source's
/// release. Short enough that a recipient waits turns rather than anything a
/// client could feel; long enough that the look's one common acquisition is
/// not charged to every delivery.
#[cfg(unix)]
const PRIVATE_DEPARTED_RELEASE_INTERVAL: usize = 8;

#[cfg(unix)]
impl PrivateXServerFrontend {
    /// Finish one release the ledger made when a source departed.
    ///
    /// HOW THE WORK IS RECOGNISED. The ledger is asked for the next record
    /// whose holders have reached zero, and that incarnation is looked for
    /// among the holds this inventory still keeps. A release a request made
    /// has already left `holds` for `settling`, so a held incarnation with no
    /// holders is one the ledger ended without a request -- which is exactly
    /// this and nothing else. Finding nothing is the ordinary case and is not
    /// a refusal.
    ///
    /// IT DOES NOT WAIT FOR THE DEPARTED SOURCE TO BE COLLECTED. A departed
    /// connection's row stays open until the service stops, so a visit that
    /// required the source to be gone from the boundary would never run, and
    /// this and that collection would each be waiting for the other.
    ///
    /// One per visit, from a cursor that persists, like every other visit
    /// here: sweeping would make the work unbounded and make it incidental to
    /// whatever else the turn was doing.
    fn release_departed_one(
        &mut self,
        keyboards: &mut PrivateKeyboards,
    ) -> Result<bool, XServerFrontendRouteError> {
        let Some(owner) = self.native_owner.as_ref() else {
            return Ok(false);
        };
        if self.terminal.settling.len() >= PRIVATE_HOLD_RECORDS {
            return Ok(false);
        }
        let controller = self.terminal.controller.clone();
        let mut departed_cursor = self.terminal.departed_cursor;
        let Some(incarnation) = controller
            .under_common(|authority| {
                authority
                    .next_debt(&mut departed_cursor)
                    .map(|(incarnation, _)| incarnation)
            })
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
        else {
            return Ok(false);
        };
        self.terminal.departed_cursor = departed_cursor;
        let Some(index) = self
            .terminal
            .holds
            .iter()
            .position(|record| record.incarnation == incarnation)
        else {
            return Ok(false);
        };
        let Some(hold) = self.terminal.holds[index].native.as_ref() else {
            return Ok(false);
        };
        let (grant, connection) = (hold.grant(), hold.connection());
        if hold.key().is_none() {
            // Pointer holds depart through their own obligations; this visit
            // answers for keys, which is what the aggregate names here.
            return Ok(false);
        }

        // The destination is reserved before anything is taken from the hold,
        // so an emission never leaves its obligation with nowhere to be.
        let Some(next_order) = self.terminal.next_event_order.checked_add(1) else {
            return Ok(false);
        };
        // A CELL OF ITS OWN, since no admission minted one: the writer answers
        // into it, and that answer is what settles the recipient's half.
        let custody = PrivateDeliveryCustody::unadmitted(self.terminal.next_event_order);

        let applied = std::cell::Cell::new(false);
        let holds = &mut self.terminal.holds;
        let built = controller
            .under_common_as_origin(|authority, issuer| {
                let permit = authority
                    .native_reconciliation(issuer, Some(grant), incarnation)
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
                let hold = holds[index]
                    .native
                    .as_mut()
                    .and_then(PrivateNativeHold::key_mut)
                    .ok_or(XServerFrontendRouteError::RegistryPoisoned)?;
                owner
                    .lock_for_release(&connection)
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?
                    .retire_key_release(&permit, hold, keyboards, &applied)
                    .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)
            })
            .map_err(|_| XServerFrontendRouteError::RegistryPoisoned)?;
        let Ok(built) = built else {
            return Ok(false);
        };

        self.terminal.next_event_order = next_order;
        let reached = self.terminal.holds[index].reached;
        // WHETHER THE RECIPIENT MAY BE REACHED, asked with no delivery to
        // name, which the recovery ledger has always accepted: a release
        // nobody requested carries no identity and still has to know whether
        // its recipient is there.
        let binding = match self
            .broker
            .registry
            .input_recovery
            .bind(None, reached.client())
        {
            Ok(true) => PrivateReleaseBinding::Reached,
            Ok(false) => PrivateReleaseBinding::Ended,
            Err(_) => PrivateReleaseBinding::Unknown,
        };
        let removed = self.terminal.holds.remove(index);
        self.terminal.settling.push(PrivateSettlingRelease {
            incarnation,
            reached,
            custody,
            press_custody: Some(removed.custody),
            native: removed.native,
            unbuilt: built.as_ref().err().copied(),
            native_recorded: false,
            native_failure: None,
            native_attempts: 0,
            // The ledger's own decision, restated rather than remade: it
            // ended this aggregate inside the revocation and this visit asked
            // it for nothing.
            outcome: sophia_input_authority::ReleaseOutcome::DeliverTo(incarnation),
            event: built.ok().flatten().map(XAuthorityInputEvent::Key),
            binding,
            delivery: None,
        });
        self.terminal.shared_activation.invalidate();
        Ok(true)
    }
}
