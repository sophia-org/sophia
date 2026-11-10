//! Harness-owned per-facade cell; placement in the production facade remains unproved.

use super::super::{
    DiscardedWorkerLease, NativeGbmRendererWorker, WorkerCommand, WorkerOutcome, WorkerResult,
};
use std::sync::mpsc::TrySendError;

enum Cell {
    Free,
    Reserved,
    Retained(DiscardedWorkerLease),
}

struct CombinedRelease<'facade> {
    facade: &'facade mut NativeGbmRendererWorker,
    cell: Cell,
}

#[derive(Debug, Eq, PartialEq)]
enum Flush {
    NoRelease,
    Enqueued,
    Full,
    Disconnected,
}

impl<'facade> CombinedRelease<'facade> {
    fn new(facade: &'facade mut NativeGbmRendererWorker) -> Self {
        Self {
            facade,
            cell: Cell::Free,
        }
    }

    fn reserve(&mut self) -> bool {
        if self.facade.quarantined
            || self.facade.stalled.is_some()
            || self.facade.in_flight.is_some()
            || self.facade.claim.is_detached()
            || !matches!(self.cell, Cell::Free)
        {
            return false;
        }
        self.cell = Cell::Reserved;
        true
    }

    /// Return refused results intact; their frame or failure still belongs to the owner.
    fn retain(&mut self, result: WorkerResult) -> Option<WorkerResult> {
        if result.output != self.facade.output || !matches!(self.cell, Cell::Reserved) {
            return Some(result);
        }
        let WorkerOutcome::Exported {
            lease_id,
            slot_token,
            ..
        } = &result.outcome
        else {
            return Some(result);
        };
        self.cell = Cell::Retained(DiscardedWorkerLease {
            output: result.output,
            lease_id: *lease_id,
            slot_token: *slot_token,
        });
        None
    }

    fn flush(&mut self) -> Flush {
        let Cell::Retained(release) = &self.cell else {
            return Flush::NoRelease;
        };
        match self
            .facade
            .core
            .command_sender
            .try_send(WorkerCommand::Release {
                output: release.output,
                lease_id: release.lease_id,
                slot_token: release.slot_token,
            }) {
            Ok(()) => {
                self.cell = Cell::Free;
                Flush::Enqueued
            }
            Err(TrySendError::Full(_)) => Flush::Full,
            Err(TrySendError::Disconnected(_)) => Flush::Disconnected,
        }
    }
}

#[path = "combined_frame_release_tests.rs"]
mod tests;
