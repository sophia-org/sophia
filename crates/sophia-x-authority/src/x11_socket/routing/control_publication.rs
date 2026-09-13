// Publishing an operation's outcome, and what that does to its record.
//
// Split from the records by subject: a record is what is owed, and this is the
// one way an outcome leaves. Kept apart because publishing is where the two
// rules that are easiest to conflate meet -- an acknowledgement goes out
// exactly once, and answering an operation is not everything it started being
// over.

#[cfg(unix)]
impl ControlCompletionRegistry {
    /// Publish an acknowledgement, under the hold that authorises it.
    ///
    /// The emission is passed in rather than done first and reported
    /// afterwards. A record can refuse an acknowledgement -- for naming
    /// another operation, for contradicting an established outcome, for no
    /// longer being held -- and a refusal that arrives after the send has
    /// already happened refuses nothing: the wrong or duplicate
    /// acknowledgement is outside and cannot be recalled. Authorising and
    /// emitting in one step is what makes the refusal mean anything.
    ///
    /// A delivered acknowledgement retires the record. A full channel keeps
    /// the exact acknowledgement to publish later. A gone receiver is neither:
    /// nothing was published, so the record stays owed rather than closed on
    /// the strength of a call that returned success.
    ///
    /// The first established outcome stands. Repeating it changes nothing;
    /// contradicting it is refused, because the effect that happened does not
    /// become a different effect later.
    pub fn publish_with(
        &self,
        token: ControlCompletionToken,
        acknowledgement: XAuthorityClientControlAck,
        publish: impl FnOnce(&XAuthorityClientControlAck) -> ControlPublication,
    ) -> Result<ControlPublication, ControlPublicationRefusal> {
        if token.origin != self.origin {
            return Err(ControlPublicationRefusal::Foreign);
        }
        let Ok(mut inner) = self.inner.lock() else {
            return Err(ControlPublicationRefusal::Unavailable);
        };
        let Some(position) = inner.records.iter().position(|held| held.token == token) else {
            return Err(ControlPublicationRefusal::NoLongerHeld);
        };
        if !inner.records[position].identity.answers(&acknowledgement) {
            return Err(ControlPublicationRefusal::NotThisOperation);
        }
        // Checked before the emission, not after it. A reservation belongs to
        // its producer, so there is no outcome of it to publish, and a
        // verdict reached after the send would refuse nothing.
        if matches!(inner.records[position].phase, ControlPhase::Reserved(_)) {
            return Err(ControlPublicationRefusal::NotAccepted);
        }
        // Nothing establishes what an abandoned operation did, so nothing may
        // publish an outcome for it. Cleanup is what it is owed.
        if matches!(inner.records[position].phase, ControlPhase::Abandoned(_)) {
            return Err(ControlPublicationRefusal::Abandoned);
        }
        // Its outcome has already gone out, and the record survives only for
        // work it queued elsewhere. Refused before the emitter, because
        // reaching it again is how a second or contradicting acknowledgement
        // gets to the receiver -- and a retained one here would turn a record
        // that has been published back into one that still owes publication.
        if matches!(inner.records[position].phase, ControlPhase::Settled(_)) {
            return Err(ControlPublicationRefusal::AlreadyPublished);
        }
        if matches!(&inner.records[position].phase, ControlPhase::Owed(established)
            if *established != acknowledgement)
        {
            return Err(ControlPublicationRefusal::OutcomeAlreadyEstablished);
        }
        let publication = publish(&acknowledgement);
        match publication {
            ControlPublication::Delivered => {
                // Published once. The record goes only when nothing it queued
                // elsewhere can still run: answering an operation is not the
                // same as everything it started being over, and freeing the
                // storage here would free a credit while an effect of it is
                // still queued somewhere.
                if inner.records[position].dependents == 0 {
                    inner.records.remove(position);
                } else {
                    let client = inner.records[position].phase.client();
                    inner.records[position].phase = ControlPhase::Settled(client);
                }
            }
            ControlPublication::Retained | ControlPublication::ReceiverGone => {
                if !matches!(inner.records[position].phase, ControlPhase::Owed(_)) {
                    inner.records[position].phase = ControlPhase::Owed(acknowledgement);
                }
            }
        }
        Ok(publication)
    }
}
