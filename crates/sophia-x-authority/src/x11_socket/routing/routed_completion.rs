// Completion for routed work that a submitter asked to be told about.
//
// Only an XTEST injector asks. Every route the session's own input phase
// sends carries no ticket and is answered to nobody, so nothing here runs for
// physical delivery. What this keeps is FakeInput's one promise to the
// request after it: not until the work was accepted, and not until it reached
// a socket, but until the registry has taken the effect. The ticket travels
// with the work, and the receiver that applies the route answers it after.

/// Answer a carried ticket, if there is one: store under no guard the
/// receiver still holds, then raise the wake. Nothing to do for the routes
/// the session's own input phase sends.
#[cfg(unix)]
fn report_completion(
    completion: Option<PrivateBarrierTicket>,
    outcome: sophia_input_authority::RequestCompletion,
) {
    if let Some(ticket) = completion {
        ticket.report(outcome);
        ticket.flush();
    }
}

#[cfg(unix)]
impl XAuthorityRoutedInputSender {
    /// Send, and be told when the registry has taken the effect.
    ///
    /// The ticket travels with the work rather than being looked up, exactly
    /// as a private reservation's does: the receiver that applies the route
    /// holds the only way to answer it, and answers it after the effect. A
    /// refusal here answers it too, so a waiter is never left parked on work
    /// that was never accepted.
    pub fn send_with_completion(
        &self,
        route: XAuthorityRoutedInput,
        ticket: PrivateBarrierTicket,
    ) -> Result<(), std::sync::mpsc::SendError<XAuthorityRoutedInput>> {
        let stamp = match self.stamp() {
            Ok(stamp) => stamp,
            Err(()) => {
                ticket.report(sophia_input_authority::RequestCompletion::Cancelled);
                ticket.flush();
                return Err(std::sync::mpsc::SendError(route));
            }
        };
        let envelope = XAuthorityEpochRoutedInput {
            control_epoch: stamp.control_epoch,
            publication: stamp.publication,
            route,
            reservation: None,
            completion: Some(ticket),
        };
        if !self.recovery.admit(&envelope.route, envelope.control_epoch, Instant::now()) {
            if let Some(ticket) = envelope.completion {
                ticket.report(sophia_input_authority::RequestCompletion::Cancelled);
                ticket.flush();
            }
            return Err(std::sync::mpsc::SendError(envelope.route));
        }
        self.sender.send(envelope).map_err(|error| {
            let envelope = error.0;
            self.recovery.abort_enqueue(envelope.route.delivery);
            if let Some(ticket) = envelope.completion {
                ticket.report(sophia_input_authority::RequestCompletion::Cancelled);
                ticket.flush();
            }
            std::sync::mpsc::SendError(envelope.route)
        })
    }
}
