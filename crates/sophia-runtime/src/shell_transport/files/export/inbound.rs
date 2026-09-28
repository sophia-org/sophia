//! Typed inbound retrieval: the peek/take accessors the owners poll instead
//! of scanning raw socket-frame kinds. A peek never disturbs order; a take
//! commits custody exactly once, matching whatever the immediately prior
//! peek reported. Explosion of a whole candidate into its wire-ordered parts
//! is not a loss: the parts stay owned, just relocated from `inbound` into
//! their own family buffer.
use super::*;

impl ShellFiles {
    pub(in crate::shell_transport) fn peek_descriptor(
        &self,
        kind: ShellFileKind,
    ) -> Option<&ShellFileDescriptorRecord> {
        self.inbound.iter().find_map(|v| match v {
            Inbound::Descriptor(value) if shell_file_descriptor_kind(&value.record) == kind => {
                Some(value.as_ref())
            }
            _ => None,
        })
    }

    pub(in crate::shell_transport) fn take_descriptor(
        &mut self,
        kind: ShellFileKind,
    ) -> Option<ShellFileDescriptorRecord> {
        self.take_descriptor_matching(kind, |_| true)
    }

    pub(in crate::shell_transport) fn take_descriptor_matching(
        &mut self,
        kind: ShellFileKind,
        select: impl Fn(&ShellFileDescriptorRecord) -> bool,
    ) -> Option<ShellFileDescriptorRecord> {
        let at = self.inbound.iter().position(|v| matches!(v, Inbound::Descriptor(value) if shell_file_descriptor_kind(&value.record) == kind && select(value)))?;
        match self.inbound.remove(at) {
            Some(Inbound::Descriptor(value)) => Some(*value),
            _ => unreachable!("selected descriptor input"),
        }
    }
    /// No accepted submission waits for the owners: neither a typed record
    /// nor the rest of a whole candidate already being handed over in parts.
    /// A candidate the client is still writing is not Session's input yet.
    pub(in crate::shell_transport) fn inbound_is_empty(&self) -> bool {
        self.inbound.is_empty()
            && self.candidate_parts.is_empty()
            && self.native_candidate_parts.is_empty()
            && self.catalog_candidate_parts.is_empty()
    }

    /// Which family the oldest still-whole queued candidate belongs to. A
    /// servicer expecting only its own family treats any other answer as a
    /// hard protocol violation, exactly as the socket wire's raw-kind scan
    /// does when a foreign candidate kind reaches the front of the inbox.
    pub(in crate::shell_transport) fn peek_candidate_family(&self) -> Option<CandidateFamily> {
        self.inbound.iter().find_map(|item| match item {
            Inbound::Candidate(_) => Some(CandidateFamily::Base),
            Inbound::NativeCandidate(_) => Some(CandidateFamily::Native),
            Inbound::CatalogCandidate(_) => Some(CandidateFamily::Catalog),
            _ => None,
        })
    }

    /// The first queued content record the predicate selects, with its
    /// transaction, left queued.
    pub(in crate::shell_transport) fn peek_content(
        &self,
        select: impl Fn(&ShellContentRecord) -> bool,
    ) -> Option<(TransactionId, &ShellContentRecord)> {
        self.inbound.iter().find_map(|item| match item {
            Inbound::Content(transaction, record) if select(record) => {
                Some((*transaction, record.as_ref()))
            }
            _ => None,
        })
    }

    pub(in crate::shell_transport) fn take_inbound(&mut self) -> Option<Inbound> {
        self.inbound.pop_front()
    }

    /// Removes the first queued content record the predicate selects,
    /// preserving the order of everything else.
    pub(in crate::shell_transport) fn take_content(
        &mut self,
        select: impl Fn(&ShellContentRecord) -> bool,
    ) -> Option<(TransactionId, ShellContentRecord)> {
        let at = self
            .inbound
            .iter()
            .position(|item| matches!(item, Inbound::Content(_, record) if select(record)))?;
        match self.inbound.remove(at) {
            Some(Inbound::Content(transaction, record)) => Some((transaction, *record)),
            _ => None,
        }
    }

    /// The first queued native allocation request, left queued.
    pub(in crate::shell_transport) fn peek_native_allocation(
        &self,
    ) -> Option<(TransactionId, NativeLauncherAllocationRequest)> {
        self.inbound.iter().find_map(|item| match item {
            Inbound::NativeAllocation(transaction, request) => Some((*transaction, **request)),
            _ => None,
        })
    }

    /// Removes the first queued native allocation request, preserving order.
    pub(in crate::shell_transport) fn take_native_allocation(
        &mut self,
    ) -> Option<(TransactionId, NativeLauncherAllocationRequest)> {
        let at = self
            .inbound
            .iter()
            .position(|item| matches!(item, Inbound::NativeAllocation(..)))?;
        match self.inbound.remove(at) {
            Some(Inbound::NativeAllocation(transaction, request)) => Some((transaction, *request)),
            _ => None,
        }
    }

    /// The first queued native input acknowledgement, left queued.
    pub(in crate::shell_transport) fn peek_native_input_ack(
        &self,
    ) -> Option<(TransactionId, NativeLauncherInputAck)> {
        self.inbound.iter().find_map(|item| match item {
            Inbound::NativeInputAck(transaction, ack) => Some((*transaction, **ack)),
            _ => None,
        })
    }

    /// Removes the first queued native input acknowledgement, preserving order.
    pub(in crate::shell_transport) fn take_native_input_ack(
        &mut self,
    ) -> Option<(TransactionId, NativeLauncherInputAck)> {
        let at = self
            .inbound
            .iter()
            .position(|item| matches!(item, Inbound::NativeInputAck(..)))?;
        match self.inbound.remove(at) {
            Some(Inbound::NativeInputAck(transaction, ack)) => Some((transaction, *ack)),
            _ => None,
        }
    }

    /// The first queued native activation, left queued.
    pub(in crate::shell_transport) fn peek_native_activate(
        &self,
    ) -> Option<(TransactionId, NativeLauncherActivation)> {
        self.inbound.iter().find_map(|item| match item {
            Inbound::NativeActivate(transaction, activation) => Some((*transaction, **activation)),
            _ => None,
        })
    }

    /// Removes the first queued native activation, preserving order.
    pub(in crate::shell_transport) fn take_native_activate(
        &mut self,
    ) -> Option<(TransactionId, NativeLauncherActivation)> {
        let at = self
            .inbound
            .iter()
            .position(|item| matches!(item, Inbound::NativeActivate(..)))?;
        match self.inbound.remove(at) {
            Some(Inbound::NativeActivate(transaction, activation)) => {
                Some((transaction, *activation))
            }
            _ => None,
        }
    }

    /// The first queued catalog activation, left queued.
    pub(in crate::shell_transport) fn peek_catalog_activate(
        &self,
    ) -> Option<(TransactionId, CatalogActivation)> {
        self.inbound.iter().find_map(|item| match item {
            Inbound::CatalogActivate(transaction, activation) => {
                Some((*transaction, (**activation).clone()))
            }
            _ => None,
        })
    }

    /// Removes the first queued catalog activation, preserving order.
    pub(in crate::shell_transport) fn take_catalog_activate(
        &mut self,
    ) -> Option<(TransactionId, CatalogActivation)> {
        let at = self
            .inbound
            .iter()
            .position(|item| matches!(item, Inbound::CatalogActivate(..)))?;
        match self.inbound.remove(at) {
            Some(Inbound::CatalogActivate(transaction, activation)) => {
                Some((transaction, *activation))
            }
            _ => None,
        }
    }

    /// The first queued indicator activation, left queued.
    pub(in crate::shell_transport) fn peek_indicator_activate(
        &self,
    ) -> Option<(TransactionId, ShellIndicatorActivation)> {
        self.inbound.iter().find_map(|item| match item {
            Inbound::IndicatorActivate(transaction, activation) => {
                Some((*transaction, **activation))
            }
            _ => None,
        })
    }

    /// Removes the first queued indicator activation, preserving order.
    pub(in crate::shell_transport) fn take_indicator_activate(
        &mut self,
    ) -> Option<(TransactionId, ShellIndicatorActivation)> {
        let at = self
            .inbound
            .iter()
            .position(|item| matches!(item, Inbound::IndicatorActivate(..)))?;
        match self.inbound.remove(at) {
            Some(Inbound::IndicatorActivate(transaction, activation)) => {
                Some((transaction, *activation))
            }
            _ => None,
        }
    }

    /// Drops the rest of the whole candidate whose earlier part the owner has
    /// just answered with a rejecting outcome. A whole candidate is one
    /// submission and gets one outcome; its later parts must not reach the
    /// owner as if a new candidate had started.
    pub(in crate::shell_transport) fn discard_candidate_parts(&mut self) {
        self.candidate_parts.clear();
    }

    /// As [`Self::discard_candidate_parts`], for a whole `NativeCandidate`.
    pub(in crate::shell_transport) fn discard_native_candidate_parts(&mut self) {
        self.native_candidate_parts.clear();
    }

    /// As [`Self::discard_candidate_parts`], for a whole `CatalogCandidate`.
    pub(in crate::shell_transport) fn discard_catalog_candidate_parts(&mut self) {
        self.catalog_candidate_parts.clear();
    }

    /// The next candidate part: the rest of the candidate already begun, or
    /// the first part of the oldest queued candidate. Other queued records
    /// keep their order.
    pub(in crate::shell_transport) fn take_candidate_part(
        &mut self,
    ) -> Option<(TransactionId, ShellContentRecord)> {
        if self.candidate_parts.is_empty() {
            let at = self
                .inbound
                .iter()
                .position(|item| matches!(item, Inbound::Candidate(_)))?;
            let Some(Inbound::Candidate(candidate)) = self.inbound.remove(at) else {
                return None;
            };
            let transaction = candidate.transaction;
            self.candidate_parts.extend(
                candidate
                    .candidate
                    .parts()
                    .into_iter()
                    .map(|record| (transaction, record)),
            );
        }
        self.candidate_parts.pop_front()
    }

    /// Explodes the oldest queued whole `NativeCandidate`, if any, into
    /// `native_candidate_parts`. A no-op once parts are already waiting there.
    /// Exploding is not a loss: the parts stay owned, just relocated.
    fn ensure_native_candidate_parts(&mut self) -> bool {
        if self.native_candidate_parts.is_empty() {
            let Some(at) = self
                .inbound
                .iter()
                .position(|item| matches!(item, Inbound::NativeCandidate(_)))
            else {
                return false;
            };
            let Some(Inbound::NativeCandidate(candidate)) = self.inbound.remove(at) else {
                return false;
            };
            let transaction = candidate.transaction;
            let (begin, chunk, end) = candidate.candidate.parts();
            let ShellNativeLauncherRecord::CandidateBegin(begin) = begin else {
                unreachable!("native candidate parts always begin with CandidateBegin");
            };
            let ShellNativeLauncherRecord::CandidateChunk(chunk) = chunk else {
                unreachable!("native candidate parts always continue with CandidateChunk");
            };
            let ShellContentRecord::CandidateEnd(end) = end else {
                unreachable!("native candidate parts always end with CandidateEnd");
            };
            self.native_candidate_parts
                .push_back((transaction, NativeCandidatePart::Begin(begin)));
            self.native_candidate_parts
                .push_back((transaction, NativeCandidatePart::Chunk(chunk)));
            self.native_candidate_parts
                .push_back((transaction, NativeCandidatePart::End(end)));
        }
        !self.native_candidate_parts.is_empty()
    }

    /// The next native candidate part, exploding the oldest whole candidate
    /// first if none is waiting yet. Left queued.
    pub(in crate::shell_transport) fn peek_native_candidate_part(
        &mut self,
    ) -> Option<(TransactionId, NativeCandidatePart)> {
        self.ensure_native_candidate_parts();
        self.native_candidate_parts.front().cloned()
    }

    /// As `take_candidate_part`, for a whole `NativeCandidate`: its Begin,
    /// Chunk and End reach the owner in wire order.
    pub(in crate::shell_transport) fn take_native_candidate_part(
        &mut self,
    ) -> Option<(TransactionId, NativeCandidatePart)> {
        self.ensure_native_candidate_parts();
        self.native_candidate_parts.pop_front()
    }

    /// As `ensure_native_candidate_parts`, for a whole `CatalogCandidate`.
    fn ensure_catalog_candidate_parts(&mut self) -> bool {
        if self.catalog_candidate_parts.is_empty() {
            let Some(at) = self
                .inbound
                .iter()
                .position(|item| matches!(item, Inbound::CatalogCandidate(_)))
            else {
                return false;
            };
            let Some(Inbound::CatalogCandidate(candidate)) = self.inbound.remove(at) else {
                return false;
            };
            let transaction = candidate.transaction;
            let (begin, chunk, end) = candidate.candidate.parts();
            let ShellCatalogActionRecord::CandidateBegin(begin) = begin else {
                unreachable!("catalog candidate parts always begin with CandidateBegin");
            };
            let ShellCatalogActionRecord::CandidateChunk(chunk) = chunk else {
                unreachable!("catalog candidate parts always continue with CandidateChunk");
            };
            let ShellContentRecord::CandidateEnd(end) = end else {
                unreachable!("catalog candidate parts always end with CandidateEnd");
            };
            self.catalog_candidate_parts
                .push_back((transaction, CatalogCandidatePart::Begin(begin)));
            self.catalog_candidate_parts
                .push_back((transaction, CatalogCandidatePart::Chunk(chunk)));
            self.catalog_candidate_parts
                .push_back((transaction, CatalogCandidatePart::End(end)));
        }
        !self.catalog_candidate_parts.is_empty()
    }

    /// The next catalog candidate part, exploding the oldest whole candidate
    /// first if none is waiting yet. Left queued.
    pub(in crate::shell_transport) fn peek_catalog_candidate_part(
        &mut self,
    ) -> Option<(TransactionId, CatalogCandidatePart)> {
        self.ensure_catalog_candidate_parts();
        self.catalog_candidate_parts.front().cloned()
    }

    /// As `take_candidate_part`, for a whole `CatalogCandidate`: its Begin,
    /// Chunk and End reach the owner in wire order.
    pub(in crate::shell_transport) fn take_catalog_candidate_part(
        &mut self,
    ) -> Option<(TransactionId, CatalogCandidatePart)> {
        self.ensure_catalog_candidate_parts();
        self.catalog_candidate_parts.pop_front()
    }
}
