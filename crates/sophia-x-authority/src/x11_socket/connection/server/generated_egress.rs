// Service-generated work owns its envelope until ordered egress takes it.
// One slot per producer prevents raster traffic and timed Presents from
// consuming each other's backpressure capacity. Admission order alternates;
// delivery always follows the allocated authority tickets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum XGeneratedEgressKind {
    Raster,
    Present,
}

impl XGeneratedEgressKind {
    fn index(self) -> usize {
        match self {
            Self::Raster => 0,
            Self::Present => 1,
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Raster => Self::Present,
            Self::Present => Self::Raster,
        }
    }
}

struct XGeneratedEgress {
    slots: [Option<XAuthorityBoundedEgressEnvelope>; 2],
    first: XGeneratedEgressKind,
    admission_passes: u64,
}

impl Default for XGeneratedEgress {
    fn default() -> Self {
        Self { slots: [None, None], first: XGeneratedEgressKind::Raster, admission_passes: 0 }
    }
}

impl XGeneratedEgress {
    fn admission_order(&mut self) -> [XGeneratedEgressKind; 2] {
        self.admission_passes = self.admission_passes.saturating_add(1);
        let order = [self.first, self.first.other()];
        self.first = self.first.other();
        order
    }

    fn vacant(&self, kind: XGeneratedEgressKind) -> bool {
        self.slots[kind.index()].is_none()
    }

    fn insert(&mut self, kind: XGeneratedEgressKind, envelope: XAuthorityBoundedEgressEnvelope) {
        assert!(self.vacant(kind), "generated authority egress slot overwritten");
        self.slots[kind.index()] = Some(envelope);
    }

    fn pending(&self) -> bool {
        self.slots.iter().flatten().any(|e| !e.cancelled)
    }

    fn try_submit(&mut self, egress: &XAuthorityOrderedEgress) -> Result<bool, X11SetupSocketError> {
        let mut order = [0, 1];
        order.sort_by_key(|&i| self.slots[i].as_ref().map(|e| e.transaction.raw()).unwrap_or(u64::MAX));
        let mut progressed = false;
        for i in order {
            if self.slots[i].as_ref().is_some_and(|e| !e.cancelled) {
                egress.try_submit(&mut self.slots[i])?;
                progressed |= self.slots[i].is_none();
            }
        }
        Ok(progressed)
    }

    fn cancel(&mut self, egress: &XAuthorityOrderedEgress) -> Vec<String> {
        let mut errors = Vec::new();
        for envelope in self.slots.iter_mut().flatten() {
            if let Err(error) = egress.cancel_envelope(envelope) {
                errors.push(format!("pending generated egress cancellation failed: {error}"));
            }
        }
        errors
    }

    fn take(&mut self) -> impl Iterator<Item = XAuthorityBoundedEgressEnvelope> + '_ {
        self.slots.iter_mut().filter_map(Option::take)
    }
}
