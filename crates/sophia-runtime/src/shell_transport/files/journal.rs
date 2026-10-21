//! The component's event journal: whole records, byte offsets, retention
//! until acknowledged. Credited responses own a terminal reserve that
//! unsolicited records can never consume.
use std::time::Instant;
mod owned;

use sophia_9p::journal::{Journal as Custody, JournalBounds as Limits};
use sophia_9p::{Errno, ReadOutcome};
use sophia_protocol::shell_files::{
    SHELL_FILE_MAX_JOURNAL_RECORDS, SHELL_FILE_TERMINAL_RESERVE_RECORDS, ShellFileAck,
    ShellFileHeader, ShellFileKind, encode_shell_file_record_at_revision,
};

/// Byte bounds derived per role from its largest Session-to-client record
/// (docs/sophia-shell-files.md, per-role disclosure bounds).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::shell_transport) struct JournalBounds {
    pub bytes: usize,
    pub reserve_bytes: usize,
}

/// An epoch-scoped reservation identity, never reused after release or spend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::shell_transport) struct JournalReservation {
    epoch: u64,
    id: u64,
}

// ChannelClosed plus two terminal events for each of four admitted offers.
const MAX_HELD_RECORDS: usize = 1 + 2 * super::export::image_offers::MAX_IMAGE_OFFERS;

pub(in crate::shell_transport) struct Journal {
    custody: Custody,
    bounds: JournalBounds,
    last_progress: Instant,
    revision: u16,
    /// Held future records and their total encoded byte sizes. Credited
    /// appends leave their record and byte capacity free; only
    /// [`Self::append_consuming_reservation`] may spend it. Uncredited
    /// appends need no subtraction: the terminal reserve they already
    /// cannot touch is where a credited held record lives, so subtracting
    /// again would fence the same room twice. This is accounting, not a
    /// named slot: holding reserves capacity, it does not promise any
    /// particular record a sequence.
    held: [Option<(JournalReservation, usize)>; MAX_HELD_RECORDS],
    next_reservation: u64,
}

impl Journal {
    pub(in crate::shell_transport) fn new(epoch: u64, bounds: JournalBounds, now: Instant) -> Self {
        Self {
            custody: Custody::new(epoch),
            bounds,
            last_progress: now,
            revision: 8,
            held: [None; MAX_HELD_RECORDS],
            next_reservation: 1,
        }
    }

    pub(super) fn enable_image_revision(&mut self) {
        self.revision = crate::owned_ring_protocol::REVISION;
    }

    /// Unsolicited records leave the terminal reserve (records and bytes)
    /// free for credited responses.
    fn limits(&self, credited: bool) -> Limits {
        if credited {
            Limits {
                records: usize::from(SHELL_FILE_MAX_JOURNAL_RECORDS),
                bytes: self.bounds.bytes,
            }
        } else {
            Limits {
                records: usize::from(
                    SHELL_FILE_MAX_JOURNAL_RECORDS - SHELL_FILE_TERMINAL_RESERVE_RECORDS,
                ),
                bytes: self.bounds.bytes.saturating_sub(self.bounds.reserve_bytes),
            }
        }
    }

    /// Whether a record of `size` bytes fits now.
    pub(in crate::shell_transport) fn fits(&self, size: usize, credited: bool) -> bool {
        self.custody.fits(size, self.append_limits(credited))
    }

    fn append_limits(&self, credited: bool) -> Limits {
        let mut limits = self.limits(credited);
        if credited {
            for (_, size) in self.held.iter().flatten() {
                limits.records = limits.records.saturating_sub(1);
                limits.bytes = limits.bytes.saturating_sub(*size);
            }
        }
        limits
    }

    /// Holds one future event's capacity. All holds together must fit the
    /// terminal reserve, which also protects them from uncredited appends.
    pub(in crate::shell_transport) fn hold_reservation(
        &mut self,
        size: usize,
    ) -> Option<JournalReservation> {
        let held_bytes: usize = self.held.iter().flatten().map(|(_, size)| *size).sum();
        if size < sophia_protocol::shell_files::SHELL_FILE_HEADER_BYTES
            || size > self.bounds.reserve_bytes.saturating_sub(held_bytes)
            || self.held.iter().flatten().count()
                >= usize::from(SHELL_FILE_TERMINAL_RESERVE_RECORDS)
            || !self.custody.fits(size, self.append_limits(true))
        {
            return None;
        }
        let next = self.next_reservation.checked_add(1)?;
        let slot = self.held.iter_mut().find(|slot| slot.is_none())?;
        let reservation = JournalReservation {
            epoch: self.custody.epoch(),
            id: self.next_reservation,
        };
        *slot = Some((reservation, size));
        self.next_reservation = next;
        Some(reservation)
    }

    /// Drops a held reservation without spending it.
    pub(in crate::shell_transport) fn release_reservation(
        &mut self,
        reservation: JournalReservation,
    ) {
        if let Some(slot) = self
            .held
            .iter_mut()
            .find(|slot| slot.is_some_and(|(held, _)| held == reservation))
        {
            *slot = None;
        }
    }

    /// Appends one complete event. Every fallible check precedes any change:
    /// a refusal spends no sequence, offset or byte.
    pub(in crate::shell_transport) fn append(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
        credited: bool,
    ) -> Result<u64, Errno> {
        Ok(self.prepare_append(kind, body, credited)?.commit())
    }

    /// Finish every fallible journal check before the owner reserves a resource
    /// and worker credit. The exclusive guard publishes nothing when dropped;
    /// once owner admission succeeds, commit cannot fail or lose its receipt.
    pub(in crate::shell_transport) fn prepare_append(
        &mut self,
        kind: ShellFileKind,
        body: &[u8],
        credited: bool,
    ) -> Result<sophia_9p::journal::PreparedRecord<'_>, Errno> {
        let bytes = encode_shell_file_record_at_revision(
            ShellFileHeader {
                kind,
                connection_epoch: self.custody.epoch(),
                submission_id: 0,
                sequence: self.custody.next_sequence(),
            },
            body,
            self.revision,
        )
        .map_err(|_| Errno::EINVAL)?;
        // A held reservation is fenced from ordinary credited appends.
        // Uncredited appends already cannot touch the terminal reserve
        // the held record will spend, so they are not subtracted twice.
        let limits = self.append_limits(credited);
        self.custody.prepare(bytes, limits)
    }

    /// Appends the record a reservation was held for, spending it. The
    /// encoded record must be exactly the held byte size; a mismatch keeps
    /// the reservation held and changes nothing.
    pub(in crate::shell_transport) fn append_consuming_reservation(
        &mut self,
        reservation: JournalReservation,
        kind: ShellFileKind,
        body: &[u8],
    ) -> Result<u64, Errno> {
        // Reservations exist for terminal events; spending one on any
        // other class is a programming error, refused before any change.
        if sophia_protocol::shell_files::shell_file_class(kind)
            != sophia_protocol::shell_files::ShellFileClass::Event
        {
            return Err(Errno::EINVAL);
        }
        let slot = self
            .held
            .iter()
            .position(|slot| slot.is_some_and(|(held, _)| held == reservation))
            .ok_or(Errno::EINVAL)?;
        let held = self.held[slot].expect("reservation found").1;
        let bytes = encode_shell_file_record_at_revision(
            ShellFileHeader {
                kind,
                connection_epoch: self.custody.epoch(),
                submission_id: 0,
                sequence: self.custody.next_sequence(),
            },
            body,
            self.revision,
        )
        .map_err(|_| Errno::EINVAL)?;
        if bytes.len() != held {
            return Err(Errno::EINVAL);
        }
        // Spend only this hold; every other offer keeps its own capacity.
        let mut limits = self.append_limits(true);
        limits.records += 1;
        limits.bytes += held;
        let prepared = self.custody.prepare(bytes, limits)?;
        self.held[slot] = None;
        Ok(prepared.commit())
    }

    pub(in crate::shell_transport) fn read(
        &self,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        self.custody.read(offset, count)
    }

    pub(in crate::shell_transport) fn read_buffered(
        &self,
        offset: u64,
        count: u32,
        scratch: &mut Vec<u8>,
    ) -> Result<ReadOutcome, Errno> {
        self.custody.read_buffered(offset, count, scratch)
    }

    /// Releases retention through `ack.sequence`. Acknowledgement releases
    /// transport retention only; it never confirms a semantic outcome.
    pub(in crate::shell_transport) fn ack(
        &mut self,
        ack: ShellFileAck,
        now: Instant,
    ) -> Result<(), Errno> {
        if self.custody.ack(ack.connection_epoch, ack.sequence)? {
            self.last_progress = now;
        }
        Ok(())
    }

    /// The last acknowledgement advance, or the journal's creation.
    pub(in crate::shell_transport) fn last_progress(&self) -> Instant {
        self.last_progress
    }

    pub(in crate::shell_transport) fn records(&self) -> usize {
        self.custody.position().records
    }

    pub(in crate::shell_transport) fn size(&self) -> u64 {
        self.custody.size()
    }
}

#[cfg(test)]
#[path = "../../../tests/support/shell_file_journal.rs"]
mod tests;

#[cfg(test)]
#[path = "../../../tests/support/shell_file_owned_journal.rs"]
mod owned_tests;

#[cfg(test)]
impl Journal {
    pub(in crate::shell_transport) fn prepare_owned_event(
        &mut self,
        event: crate::owned_ring_protocol::Event,
        credited: bool,
    ) -> Result<sophia_9p::journal::PreparedRecord<'_>, Errno> {
        let bytes = self.owned_event_bytes(event)?;
        let limits = self.append_limits(credited);
        self.custody.prepare(bytes, limits)
    }
}
