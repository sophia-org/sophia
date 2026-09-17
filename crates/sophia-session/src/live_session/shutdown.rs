use super::{SessionQuiescence, XAuthorityObservedTransactionBatch, XServerFrontendServiceCommand};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, SendError, SyncSender, TryRecvError};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AuthorityIngressState {
    Open,
    Disconnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AuthorityWorkWait {
    Service,
    Receive,
}

/// Selects accepted work without polling the frontend or bypassing owner service.
pub(super) fn take_authority_work(
    initial: &mut Option<XAuthorityObservedTransactionBatch>,
    queued: &mut VecDeque<XAuthorityObservedTransactionBatch>,
    coordinator: Option<sophia_protocol::TransactionId>,
    ingress: AuthorityIngressState,
    owner_service_required: bool,
) -> Result<XAuthorityObservedTransactionBatch, AuthorityWorkWait> {
    if owner_service_required {
        return Err(AuthorityWorkWait::Service);
    }
    // EOF closes ingress, not the owner's accepted-work queues. In particular,
    // final removal batches must still retire routes, buffers, and layout state.
    initial
        .take()
        .or_else(|| queued.pop_front())
        .or_else(|| coordinator.map(super::wm_update_coordinator_batch))
        .ok_or(match ingress {
            AuthorityIngressState::Open => AuthorityWorkWait::Receive,
            AuthorityIngressState::Disconnected => AuthorityWorkWait::Service,
        })
}

/// Buffers immediately available batches without blocking.
///
/// A disconnected receiver is not itself a failure. During quiescence it is
/// the frontend's positive drain signal; before quiescence the owner still
/// treats it as a fatal authority loss. Keep that policy in
/// `observe_authority_ingress` so blocking and opportunistic receives cannot
/// classify the same channel state differently.
pub(super) fn drain_queued_authority_batches(
    receiver: &Receiver<XAuthorityObservedTransactionBatch>,
    queued: &mut VecDeque<XAuthorityObservedTransactionBatch>,
    capacity: usize,
    budget: Duration,
    ingress: AuthorityIngressState,
) -> AuthorityIngressState {
    if ingress == AuthorityIngressState::Disconnected {
        return ingress;
    }
    let started = Instant::now();
    while queued.len() < capacity && started.elapsed() < budget {
        match receiver.try_recv() {
            Ok(batch) => queued.push_back(batch),
            Err(TryRecvError::Empty) => return AuthorityIngressState::Open,
            Err(TryRecvError::Disconnected) => return AuthorityIngressState::Disconnected,
        }
    }
    AuthorityIngressState::Open
}

pub(super) fn observe_authority_ingress(
    state: AuthorityIngressState,
    quiescence: &mut Option<SessionQuiescence>,
    now: Instant,
) -> Result<(), &'static str> {
    if state == AuthorityIngressState::Open {
        return Ok(());
    }
    let Some(quiescence) = quiescence.as_mut() else {
        return Err("persistent X authority transaction channel disconnected");
    };
    if !quiescence.frontend_authority_drained {
        quiescence.mark_frontend_authority_drained();
        crate::session_println!(
            "sophia_live_session_quiescence schema=3 status=frontend_drained reason={} elapsed_msec={}",
            quiescence.reason,
            quiescence.elapsed(now).as_millis(),
        );
    }
    Ok(())
}

fn send_frontend_stop(
    sender: &SyncSender<XServerFrontendServiceCommand>,
    stopped: &mut bool,
    command: XServerFrontendServiceCommand,
) -> Result<(), SendError<XServerFrontendServiceCommand>> {
    if *stopped {
        return Ok(());
    }
    sender.send(command)?;
    *stopped = true;
    Ok(())
}

/// Stops frontend admission once. Fatal cleanup terminates clients separately.
pub(super) fn stop_frontend_intake(
    sender: &SyncSender<XServerFrontendServiceCommand>,
    stopped: &mut bool,
) -> Result<(), SendError<XServerFrontendServiceCommand>> {
    send_frontend_stop(
        sender,
        stopped,
        XServerFrontendServiceCommand::StopAccepting,
    )
}

/// Closes client streams without cancelling their accepted ordered work.
/// Merely closing admission leaves a live browser holding quiescence open.
pub(super) fn disconnect_frontend_for_drain(
    sender: &SyncSender<XServerFrontendServiceCommand>,
    stopped: &mut bool,
) -> Result<(), SendError<XServerFrontendServiceCommand>> {
    send_frontend_stop(
        sender,
        stopped,
        XServerFrontendServiceCommand::DrainAndDisconnect,
    )
}

/// Close optional publication before the frontend can consume its drain request.
/// A failed send remains fatal and never reopens the publisher.
pub(super) fn begin_frontend_quiescence(
    publisher: &mut super::window_allocation::LiveWindowAllocationPublisher,
    sender: &SyncSender<XServerFrontendServiceCommand>,
    stopped: &mut bool,
) -> Result<(), SendError<XServerFrontendServiceCommand>> {
    publisher.begin_quiescence();
    disconnect_frontend_for_drain(sender, stopped)
}

/// Recheck at the recovery boundary: draining the old owner can itself consume
/// the remaining runtime budget. Seat notifications never extend that budget.
pub(super) fn native_recovery_allowed(
    deadline: Option<Instant>,
    now: Instant,
    quiescing: bool,
    draining_keys: bool,
) -> bool {
    !quiescing && !draining_keys && deadline.is_none_or(|deadline| now < deadline)
}
