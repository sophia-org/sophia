//! Writes received in place: see the WRITES IN PLACE note on the parent
//! module.

use super::{Connection, ERROR_REPLY, Fatal, FidState, SMALL_REPLY};
use crate::export::{Access, Export, NodeKind, Operation};
use crate::records::{Errno, Fid, Reply, Tag};
use crate::wire;

/// A `Twrite` before its data: size[4] type[1] tag[2] fid[4] offset[8]
/// count[4].
pub(super) const WRITE_HEADER: usize = 23;
/// The larger of the two answers to a write, `Rwrite` and `Rlerror`.
pub(super) const WRITE_REPLY: usize = ERROR_REPLY;

/// A write being received in place.
pub(super) struct InPlace {
    pub(super) tag: Tag,
    pub(super) fid: Fid,
    pub(super) offset: u64,
    pub(super) len: u32,
    pub(super) received: u32,
    /// The owner's refusal: the rest of the data is read and discarded, and
    /// this is the answer.
    pub(super) refused: Option<Errno>,
}

/// Where the next bytes of a write received in place go.
pub enum InPlaceTarget<'a> {
    /// The owner's destination: exactly the rest of the write's data.
    Destination(&'a mut [u8]),
    /// The owner refused the write: read this many bytes, the rest of its
    /// data, and discard them.
    Discard(usize),
}

impl<E: Export> Connection<E> {
    /// Where the next bytes of the write being received in place go, if
    /// one is: the owner's destination for the rest of its data, asked
    /// again now, after [`Export::check`], or, once the owner has refused,
    /// how many bytes to discard. Read at most that many, then call
    /// [`Self::received_in_place`] with the count read, whatever it was.
    pub fn in_place_target<'a>(&'a mut self, export: &'a mut E) -> Option<InPlaceTarget<'a>> {
        let place = self.in_place.as_mut()?;
        let rest = (place.len - place.received) as usize;
        if place.refused.is_none() {
            let refusal = match self.fids.get_mut(&place.fid) {
                Some(FidState {
                    node,
                    epoch,
                    open: Some(opened),
                    ..
                }) => {
                    let access = Access {
                        connection: self.id,
                        epoch: *epoch,
                        node,
                        operation: Operation::Write,
                    };
                    match export.check(&access).map(|()| {
                        export.write_destination(
                            node,
                            &mut opened.handle,
                            place.offset,
                            place.len,
                            place.received,
                        )
                    }) {
                        Ok(Ok(Some(destination))) if destination.len() >= rest => {
                            return Some(InPlaceTarget::Destination(&mut destination[..rest]));
                        }
                        // An owner that accepted must keep taking the data
                        // or refuse it; it cannot hand it back to buffer.
                        Ok(Ok(Some(_) | None)) => Errno::EIO,
                        Ok(Err(errno)) | Err(errno) => errno,
                    }
                }
                _ => Errno::EBADF,
            };
            place.refused = Some(refusal);
        }
        Some(InPlaceTarget::Discard(rest))
    }

    /// Accounts for `count` bytes read for the write being received in
    /// place, into its destination or discarded, and answers the write once
    /// all of it has arrived.
    pub fn received_in_place(&mut self, export: &mut E, count: usize) -> Result<(), Fatal> {
        let Some(place) = self.in_place.as_mut() else {
            return Ok(());
        };
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        place.received += count.min(place.len - place.received);
        if place.received == place.len {
            self.finish_in_place(export)?;
        }
        Ok(())
    }

    /// Answers a write received whole: once, with the room its reply kept.
    pub(super) fn finish_in_place(&mut self, export: &mut E) -> Result<(), Fatal> {
        if self.output.len() + WRITE_REPLY > self.limits.max_unsent() {
            // Not reached while the room is kept; wait rather than overrun.
            self.stalled = true;
            return Ok(());
        }
        let Some(place) = self.in_place.take() else {
            return Ok(());
        };
        let reply = match place.refused {
            Some(errno) => Reply::Lerror(errno),
            None => match self.fids.get_mut(&place.fid) {
                Some(FidState {
                    node,
                    epoch,
                    open: Some(opened),
                    ..
                }) => {
                    let access = Access {
                        connection: self.id,
                        epoch: *epoch,
                        node,
                        operation: Operation::Write,
                    };
                    match export.check(&access).and_then(|()| {
                        export.write_received(node, &mut opened.handle, place.offset, place.len)
                    }) {
                        Ok(count) if count > place.len => Reply::Lerror(Errno::EIO),
                        Ok(count) => Reply::Write(count),
                        Err(errno) => Reply::Lerror(errno),
                    }
                }
                _ => Reply::Lerror(Errno::EBADF),
            },
        };
        self.send(place.tag, &reply)
    }

    /// Starts receiving in place the write `partial` begins, a frame of
    /// `length` bytes not yet whole, once its header is here and checked:
    /// the protocol's header rules, an exact count, room for the reply, a
    /// fid open for writing, and the export's check. Anything short of that
    /// leaves the request to be buffered and answered whole, as any other.
    /// True when started: what `partial` holds of the data has gone to the
    /// destination, or been discarded if the owner refused at once.
    pub(super) fn start_in_place(
        &mut self,
        export: &mut E,
        partial: &[u8],
        length: usize,
    ) -> Result<bool, Fatal> {
        let Some(header) = partial.first_chunk::<WRITE_HEADER>() else {
            return Ok(false);
        };
        let Some((kind, tag)) = wire::header(header) else {
            return Ok(false);
        };
        if kind != wire::kind::TWRITE {
            return Ok(false);
        }
        self.admit(kind, tag)?;
        let field = |at: usize, n: usize| {
            header[at..at + n]
                .iter()
                .rev()
                .fold(0u64, |value, byte| value << 8 | u64::from(*byte))
        };
        let (fid, offset, len) = (Fid(field(7, 4) as u32), field(11, 8), field(19, 4) as u32);
        if (WRITE_HEADER as u64).checked_add(u64::from(len)) != Some(length as u64)
            || !self.has_room(SMALL_REPLY)
        {
            return Ok(false);
        }
        let id = self.id;
        let Some(FidState {
            node,
            epoch,
            open: Some(opened),
            ..
        }) = self.fids.get_mut(&fid)
        else {
            return Ok(false);
        };
        if !opened.access.writes() || opened.kind != NodeKind::File {
            return Ok(false);
        }
        let access = Access {
            connection: id,
            epoch: *epoch,
            node,
            operation: Operation::Write,
        };
        if export.check(&access).is_err() {
            return Ok(false);
        }
        let data = &partial[WRITE_HEADER..];
        let refused = match export.write_destination(node, &mut opened.handle, offset, len, 0) {
            Ok(None) => return Ok(false),
            Ok(Some(destination)) if destination.len() >= len as usize => {
                destination[..data.len()].copy_from_slice(data);
                None
            }
            Ok(Some(_)) => Some(Errno::EIO),
            Err(errno) => Some(errno),
        };
        self.in_place = Some(InPlace {
            tag,
            fid,
            offset,
            len,
            received: data.len() as u32,
            refused,
        });
        Ok(true)
    }
}
