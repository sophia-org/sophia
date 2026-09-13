// Who may answer an obligation, and answering one.
//
// Split from both the owner and the handle by subject. Every settlement path
// asks the same question before it publishes -- whether anything else can
// still publish for this operation -- and the answer does not depend on which
// path is asking.

/// Whether this settlement may answer an obligation itself.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettlementOwnership {
    /// No live completion record can publish for it, so answering here gives
    /// it exactly one outcome.
    Ours,
    /// A record is still held by someone who can publish for it. Answering
    /// here would be the second owner.
    Elsewhere,
    /// The registry cannot say. Not an outcome, and not permission to act: a
    /// record nobody can read is the one case where both answering and
    /// discarding are guesses.
    Unprovable,
}

/// Establish who may answer an obligation, taking the record if it is free.
///
/// Asked before publication rather than after. A settlement that emits first
/// and tidies the record afterwards has already given the operation two
/// owners by the time it looks, and the outcome it sent cannot be recalled.
///
/// Only control carries a completion record. Everything else has a single
/// owner by construction.
#[cfg(unix)]
fn ownership_of(
    origin: &XServerFrontendRouteRegistry,
    operation: &PrivateOperation,
) -> SettlementOwnership {
    let PrivateOperation::Control(_, Some(token)) = operation else {
        return SettlementOwnership::Ours;
    };
    let Some(owner) = origin.control_completion() else {
        // No registry to ask. Retaining costs a held credit; assuming
        // ownership costs a duplicate outcome for a client.
        return SettlementOwnership::Unprovable;
    };
    match owner.state_of(*token) {
        // Issued here and no longer held: it was handed over or already
        // settled, so nothing else can publish for it.
        ControlRecordState::Retired => SettlementOwnership::Ours,
        // Still held. Taking it is the handover, and it succeeds only for a
        // record that has not begun applying and is owed no receipt.
        ControlRecordState::Outstanding => {
            if owner.discard(*token) {
                SettlementOwnership::Ours
            } else {
                SettlementOwnership::Elsewhere
            }
        }
        ControlRecordState::Unanswerable => SettlementOwnership::Unprovable,
    }
}

/// Answer one obligation, saying whether it was answered.
///
/// Borrowed rather than taken. This is where the acknowledgement is emitted,
/// so a caller that handed ownership in would lose the obligation to an unwind
/// at exactly the moment it stopped being able to say whether the outcome went
/// out. Borrowing leaves the caller holding it either way, free to keep it as
/// unsettled work or park it as an outcome nobody can prove.
///
/// Answers only. Nothing here decides what becomes of what it could not
/// answer: that belongs to the caller, which is the one that knows where the
/// obligation lives.
#[cfg(unix)]
fn settle_one(registry: &XServerFrontendRouteRegistry, operation: &PrivateOperation) -> bool {
    {
        match operation {
            PrivateOperation::RoutedInput(envelope) => {
                let client = registry
                    .surfaces
                    .lock()
                    .ok()
                    .and_then(|surfaces| {
                        surfaces
                            .get(&envelope.route.request.target_surface)
                            .map(|route| route.client)
                    });
                let Some(client) = client else {
                    // Ownership is retained rather than resolved by guessing.
                    // A receipt goes to the issuer's channel rather than to the
                    // named client, so the harm is not that another client
                    // receives it; it is a receipt attributed to a client
                    // nobody resolved, which correlates with nothing. Choosing
                    // a recipient here would also choose it at the wrong
                    // moment: final target resolution belongs at execution.
                    return false;
                };
                registry
                    .send_input_delivery(
                        client,
                        envelope.route.delivery,
                        XAuthorityInputDeliveryOutcome::RouteRejected,
                    )
                    .is_ok()
            }
            PrivateOperation::Control(control, _token) => {
                let acknowledgement = XAuthorityClientControlAck {
                    client: control.client,
                    acknowledgement: XAuthorityControlAck {
                        kind: control.command.kind(),
                        transaction: control.command.transaction(),
                        surface: control.command.surface(),
                        outcome: XAuthorityControlOutcome::AuthorityRejected,
                    },
                };
                // Nothing here executed the command, so what is retained on
                // failure is the command, not an outcome: the caller retries
                // this settlement, and a retry that only sends a rejection
                // replays nothing.
                //
                // No completion record is answered here. A command reaching
                // this point was handed on by an owner that gave up its record
                // as it did so, which is the one place that sees all of them
                // at once; answering again from here would give one operation
                // two owners able to publish for it.
                registry
                    .acknowledgement_sender
                    .try_send(acknowledgement)
                    .is_ok()
            }
            PrivateOperation::LeaseRelease(release) => {
                registry.release_route_lease(*release).is_ok()
            }
        }
    }
}
