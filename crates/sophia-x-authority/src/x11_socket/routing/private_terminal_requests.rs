// One retained original request per charged maintenance visit. This only
// disposes accepted item storage; native, recipient and wire debts stay in
// their independently reserved custody.

#[cfg(unix)]
impl PrivateOrderedItem {
    fn retire_request(&mut self) -> Result<bool, PrivateAuthorityRefusal> {
        use sophia_input_authority::RequestCompletion as Completion;
        let (ran, custody) = match self {
            Self::Ran { custody, .. } => (true, custody),
            Self::Refused { custody, .. } => (false, custody),
            Self::Parked { .. } => return Ok(false),
        };
        // Revocation can publish Cancelled after an interrupted source call.
        // That is not evidence about effects which the lost call may have
        // begun. Keep both its item and its original charge.
        if custody.phase.get() == PrivateRequestPhase::Entered {
            return Ok(false);
        }
        let completion = match custody.observed_outcome.get() {
            Some(completion) => Some(completion),
            None => custody.observe()?,
        };
        let disposable = match completion {
            Some(Completion::Cancelled | Completion::Refused(_)) => true,
            // Ran recorded the transfer into pre-reserved native/output
            // custody. A refused adapter result does not establish that.
            Some(Completion::Processed) => ran && custody.phase.get() == PrivateRequestPhase::Settled,
            Some(Completion::FailedAfterApplication(_)) | None => false,
        };
        Ok(disposable && custody.finish_item())
    }
}

#[cfg(unix)]
impl PrivateTerminalInventory {
    fn retire_request_one(
        &mut self,
        cursor: &mut usize,
    ) -> Result<PrivateTerminalVisit, PrivateTerminalDriveRefusal> {
        let current = usize::from(self.current.is_some());
        let count = current + self.turn.len() + self.delivering.len() + self.undelivered.len();
        if count == 0 {
            return Ok(PrivateTerminalVisit::Request { disposed: false });
        }
        let index = *cursor % count;
        *cursor = (index + 1) % count;
        let disposed = if index < current {
            let disposed = self.current.as_mut().expect("counted above").retire_request();
            if matches!(disposed, Ok(true)) {
                self.current = None;
            }
            disposed
        } else if index < current + self.turn.len() {
            let index = index - current;
            let disposed = self.turn[index].retire_request();
            if matches!(disposed, Ok(true)) {
                self.turn.remove(index);
            }
            disposed
        } else if index < current + self.turn.len() + self.delivering.len() {
            let index = index - current - self.turn.len();
            let disposed = self.delivering[index].retire_request();
            if matches!(disposed, Ok(true)) {
                self.delivering.remove(index);
            }
            disposed
        } else {
            let index = index - current - self.turn.len() - self.delivering.len();
            let disposed = self.undelivered[index].item.retire_request();
            if matches!(disposed, Ok(true)) {
                self.undelivered.remove(index);
            }
            disposed
        }.map_err(PrivateTerminalDriveRefusal::Common)?;
        Ok(PrivateTerminalVisit::Request { disposed })
    }
}
