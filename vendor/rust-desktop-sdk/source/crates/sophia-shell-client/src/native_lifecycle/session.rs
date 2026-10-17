//! Owning event-loop facade. It alone consumes observations and issues transactions.
use super::*;
use crate::{CustodyLookupError, IoInterest};
use std::os::fd::{AsFd, BorrowedFd};
use std::time::Instant;

/// Scheduling hints, never proof that a server deadline has not elapsed.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeObligations {
    pub input_due_ms: Option<u64>,
    /// Action has no issuance timestamp; receipt plus timeout is only a hint.
    pub action_due_ms: Option<u64>,
    pub permit_expires_ms: Option<u64>,
    pub allocation_due_ms: Option<u64>,
    pub candidate_due_ms: Option<u64>,
    pub next_ms: Option<u64>,
    pub needs_catalog: bool,
    pub needs_facts: bool,
    pub needs_candidate: bool,
    pub awaiting_invalidation: bool,
    pub lost_acks: u8,
}

/// Owns a fresh negotiated launcher connection. No other consumer may have
/// drained its observations or issued transactions. Snapshot installation is
/// automatic when `next_event` returns the fetched catalog/facts; renderers must
/// install that value before presenting. No reconnect or implicit I/O on receive.
pub struct NativeLauncherSession {
    connection: ShellConnection,
    lifecycle: NativeLauncherLifecycle,
    head: Option<NativeLauncherObservation>,
    head_ms: u64,
    candidate_ms: u64,
    next_transaction: u64,
    last_ms: Option<u64>,
}

enum Response<'a> {
    Input(NativeInputReservation<'a>),
    Action(NativeActionReservation<'a>),
}
/// Drop cancels preparation and preserves the event and transaction range.
/// Commit consumes the head and advances IDs without allocation or I/O.
pub struct NativeSessionResponse<'a> {
    response: Response<'a>,
    head: &'a mut Option<NativeLauncherObservation>,
    next: &'a mut u64,
    count: u64,
}
impl NativeSessionResponse<'_> {
    pub fn commit(self) -> Admission {
        let admission = match self.response {
            Response::Input(value) => value.commit(),
            Response::Action(value) => value.commit(),
        };
        *self.next += self.count;
        *self.head = None;
        admission
    }
}

impl NativeLauncherSession {
    pub fn new(
        connection: ShellConnection,
        first_transaction: u64,
    ) -> Result<Self, ShellClientError> {
        connection.wire.require_native_launcher()?;
        connection.io_interest()?;
        if first_transaction == 0 || first_transaction == u64::MAX {
            return Err(ShellClientError::InvalidArgument(
                "native transaction range",
            ));
        }
        let limits = connection
            .inbox
            .iter()
            .find_map(|v| match v {
                crate::wire::Inbound::Content(_, ShellContentRecord::Limits(limits)) => {
                    Some(limits.clone())
                }
                _ => None,
            })
            .ok_or(ShellClientError::InvalidArgument(
                "native Limits not available",
            ))?;
        Ok(Self {
            connection,
            lifecycle: NativeLauncherLifecycle::new(limits)?,
            head: None,
            head_ms: 0,
            candidate_ms: 0,
            next_transaction: first_transaction,
            last_ms: None,
        })
    }
    pub fn lifecycle(&self) -> &NativeLauncherLifecycle {
        &self.lifecycle
    }
    /// End locally and disarm lifecycle authority. Custody remains readable.
    pub fn close(&mut self) {
        self.connection.close();
        if self.lifecycle.state == NativeLifecycleState::Live {
            self.lifecycle.end(NativeLifecycleState::Ended);
        }
        self.head = None;
    }
    pub fn custody(&self, ticket: Ticket) -> Result<Custody, CustodyLookupError> {
        self.connection.custody_result(ticket)
    }
    pub fn io_interest(&self) -> Result<IoInterest, ShellClientError> {
        self.connection.io_interest()
    }
    pub fn wake_deadline(&self) -> Option<Instant> {
        self.connection.wake_deadline()
    }
    fn clock(&mut self, now_ms: u64) -> Result<(), ShellClientError> {
        if self.last_ms.is_some_and(|last| now_ms < last) {
            return Err(ShellClientError::InvalidArgument(
                "native clock moved backwards",
            ));
        }
        self.last_ms = Some(now_ms);
        Ok(())
    }
    /// `now` drives transport retries; `now_ms` is host CLOCK_MONOTONIC time
    /// for protocol scheduling. Neither domain may move backwards. A zero byte
    /// budget does no transport I/O but still reconciles local lifecycle state.
    pub fn dispatch(
        &mut self,
        now: Instant,
        now_ms: u64,
        byte_budget: usize,
    ) -> Result<(), ShellClientError> {
        self.clock(now_ms)?;
        let result = self.connection.dispatch(now, byte_budget);
        let service = self.lifecycle.service_at(&self.connection, now_ms);
        result.and(service)
    }
    /// Return the same head until `done` or response commit; apply it once.
    pub fn next_event(
        &mut self,
        now_ms: u64,
    ) -> Result<Option<&NativeLauncherObservation>, ShellClientError> {
        self.clock(now_ms)?;
        self.lifecycle.service_at(&self.connection, now_ms)?;
        if self.head.is_none()
            && let Some(value) = self.connection.take_native_launcher_observation()?
        {
            self.lifecycle.observe(&value)?;
            self.head = Some(value);
            self.head_ms = now_ms;
        }
        Ok(self.head.as_ref())
    }
    pub fn done(&mut self) -> Result<(), ShellClientError> {
        self.lifecycle.live()?;
        if self.head.is_none()
            || self.lifecycle.input.is_some()
            || self
                .lifecycle
                .action
                .as_ref()
                .is_some_and(|(_, s)| s.ack_owed)
        {
            return Err(ShellClientError::InvalidArgument(
                "native head requires response",
            ));
        }
        self.head = None;
        self.lifecycle.action = None;
        Ok(())
    }
    fn transaction(&self, count: u64) -> Result<TransactionId, ShellClientError> {
        self.lifecycle.live()?;
        self.next_transaction
            .checked_add(count)
            .ok_or(ShellClientError::InvalidArgument(
                "native transaction exhausted",
            ))?;
        Ok(TransactionId::from_raw(self.next_transaction))
    }
    /// Reserve capacity before invoking an editable input's callback, then
    /// commit consumed/stale according to its result. Panic fails closed.
    pub fn apply_input(
        &mut self,
        activate: bool,
        edit: impl FnOnce(&NativeLauncherInput) -> bool,
    ) -> Result<Admission, ShellClientError> {
        let count = if activate { 2 } else { 1 };
        let first = self.transaction(count)?;
        let ack = TransactionId::from_raw(self.next_transaction + count - 1);
        let a = self.connection.apply_native_input(
            &mut self.lifecycle,
            ack,
            activate.then_some(first),
            edit,
        )?;
        self.next_transaction += count;
        self.head = None;
        Ok(a)
    }
    pub fn prepare_input(
        &mut self,
        disposition: u16,
        activate: bool,
    ) -> Result<NativeSessionResponse<'_>, ShellClientError> {
        let count = if activate { 2 } else { 1 };
        let first = self.transaction(count)?;
        let ack = TransactionId::from_raw(self.next_transaction + count - 1);
        let response = self.connection.prepare_native_input(
            &mut self.lifecycle,
            ack,
            disposition,
            activate.then_some(first),
        )?;
        Ok(NativeSessionResponse {
            response: Response::Input(response),
            head: &mut self.head,
            next: &mut self.next_transaction,
            count,
        })
    }
    pub fn prepare_action(
        &mut self,
        disposition: u16,
        activate: bool,
    ) -> Result<NativeSessionResponse<'_>, ShellClientError> {
        let count = if activate { 2 } else { 1 };
        let first = self.transaction(count)?;
        let ack = TransactionId::from_raw(self.next_transaction + count - 1);
        let response = self.connection.prepare_native_action(
            &mut self.lifecycle,
            ack,
            disposition,
            activate.then_some(first),
        )?;
        Ok(NativeSessionResponse {
            response: Response::Action(response),
            head: &mut self.head,
            next: &mut self.next_transaction,
            count,
        })
    }
    pub fn allocate(
        &mut self,
        params: NativeAllocationParams,
        now_ms: u64,
    ) -> Result<Admission, ShellClientError> {
        self.clock(now_ms)?;
        let tx = self.transaction(1)?;
        let a = self
            .connection
            .allocate_native(&mut self.lifecycle, tx, params, now_ms)?;
        self.next_transaction += 1;
        Ok(a)
    }
    pub fn release_allocation(&mut self, now_ms: u64) -> Result<Admission, ShellClientError> {
        self.clock(now_ms)?;
        let tx = self.transaction(1)?;
        let a = self
            .connection
            .release_native_allocation(&mut self.lifecycle, tx, now_ms)?;
        self.next_transaction += 1;
        Ok(a)
    }
    pub fn begin_resource(
        &mut self,
        begin: ContentResourceBegin,
    ) -> Result<Admission, ShellClientError> {
        let tx = self.transaction(1)?;
        let a = self
            .connection
            .begin_native_resource(&mut self.lifecycle, tx, begin)?;
        self.next_transaction += 1;
        Ok(a)
    }
    pub fn upload_chunk(&mut self, bytes: &[u8]) -> Result<Admission, ShellClientError> {
        self.connection
            .upload_native_chunk(&mut self.lifecycle, bytes)
    }
    pub fn end_resource(&mut self) -> Result<Admission, ShellClientError> {
        let tx = self.transaction(1)?;
        let a = self
            .connection
            .end_native_resource(&mut self.lifecycle, tx)?;
        self.next_transaction += 1;
        Ok(a)
    }
    pub fn cancel_resource(
        &mut self,
        resource: ContentResourceId,
    ) -> Result<Admission, ShellClientError> {
        let tx = self.transaction(1)?;
        let a = self
            .connection
            .cancel_native_resource(&mut self.lifecycle, tx, resource)?;
        self.next_transaction += 1;
        Ok(a)
    }
    pub fn retire_resource(
        &mut self,
        resource: ContentResourceId,
    ) -> Result<Admission, ShellClientError> {
        let tx = self.transaction(1)?;
        let a = self
            .connection
            .retire_native_resource(&mut self.lifecycle, tx, resource)?;
        self.next_transaction += 1;
        Ok(a)
    }
    pub fn demand_frame(
        &mut self,
        reason: u16,
        now_ms: u64,
    ) -> Result<Admission, ShellClientError> {
        self.clock(now_ms)?;
        let tx = self.transaction(1)?;
        let a = self
            .connection
            .demand_native_frame(&mut self.lifecycle, tx, reason, now_ms)?;
        self.next_transaction += 1;
        Ok(a)
    }
    pub fn cancel_demand(
        &mut self,
        now_ms: u64,
        margin_ms: u32,
    ) -> Result<Admission, ShellClientError> {
        self.clock(now_ms)?;
        let tx = self.transaction(1)?;
        let a = self
            .connection
            .cancel_native_demand(&mut self.lifecycle, tx, now_ms, margin_ms)?;
        self.next_transaction += 1;
        Ok(a)
    }
    pub fn present(
        &mut self,
        scene: &NativeScene,
        now_ms: u64,
        margin_ms: u32,
    ) -> Result<Admission, ShellClientError> {
        self.clock(now_ms)?;
        let tx = self.transaction(1)?;
        let a = self.connection.present_native_scene(
            &mut self.lifecycle,
            tx,
            scene,
            now_ms,
            margin_ms,
        )?;
        self.candidate_ms = now_ms;
        self.next_transaction += 1;
        Ok(a)
    }
    pub fn obligations(&self) -> NativeObligations {
        let l = &self.lifecycle;
        if l.state != NativeLifecycleState::Live {
            return NativeObligations::default();
        }
        let needs_catalog = l.opening.is_some() && !l.catalog_current();
        let needs_facts = l.opening.is_some() && (l.facts == 0 || l.facts != l.facts_announced);
        let needs_candidate = l.opening.is_some()
            && !needs_catalog
            && !needs_facts
            && l.allocation().is_some_and(|allocation| {
                l.pending.is_none()
                    && l.shown.is_none_or(|shown| {
                        shown.revision != l.revision || shown.allocation != allocation.allocation
                    })
            });
        let mut o = NativeObligations {
            input_due_ms: l.input.as_ref().map(|(_, s)| s.ack_due_ms),
            action_due_ms: l.action.as_ref().filter(|(_, s)| s.ack_owed).map(|_| {
                self.head_ms
                    .saturating_add(u64::from(l.limits.action_ack_timeout_ms))
            }),
            permit_expires_ms: l.permit_deadline_ms(),
            allocation_due_ms: l.allocation_deadline_ms(),
            candidate_due_ms: l.pending.map(|_| {
                self.candidate_ms.saturating_add(
                    u64::from(l.limits.candidate_timeout_ms)
                        + u64::from(l.limits.preparation_timeout_ms)
                        + u64::from(l.limits.presentation_timeout_ms),
                )
            }),
            needs_catalog,
            needs_facts,
            needs_candidate,
            awaiting_invalidation: l.awaiting_invalidation(),
            lost_acks: l.lost_acks,
            next_ms: None,
        };
        o.next_ms = [
            o.input_due_ms,
            o.action_due_ms,
            o.permit_expires_ms,
            o.allocation_due_ms,
            o.candidate_due_ms,
        ]
        .into_iter()
        .flatten()
        .min();
        o
    }
}
impl AsFd for NativeLauncherSession {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.connection.as_fd()
    }
}
