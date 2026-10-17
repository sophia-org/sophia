//! Bounded credit window over adjacent canonical upload units. Payloads and
//! tickets remain distinct; replies match by tag and settle in issue order.
use super::{
    Current, FileWire, O_WRONLY, Opening, QueuedKind, RETRY_FIRST, UploadState, WRITE_OVERHEAD,
    open_fid_for,
};
use crate::{ShellClientError, custody::Custody, outbox::ClientOutbox};
use sophia_9p_client::pipeline::{PipelineError, Reply};
use sophia_9p_records::{Errno, Fid, Tag};
use sophia_shell_protocol::{ContentResourceId, shell_files::SHELL_FILE_MAX_QUEUED_UPLOAD_WRITES};
use std::time::Instant;
const WIDTH: usize = SHELL_FILE_MAX_QUEUED_UPLOAD_WRITES;

pub(super) struct Unit {
    pub ticket: u64,
    offset: u64,
    bytes: Box<[u8]>,
    confirmed: usize,
    tag: Option<Tag>,
    issued: usize,
    reply: Option<Result<u32, Errno>>,
    invalidated: bool,
    outcome: Option<Custody>,
}
impl Unit {
    fn new(offset: u64, bytes: Box<[u8]>, ticket: u64) -> Self {
        Self {
            ticket,
            offset,
            bytes,
            confirmed: 0,
            tag: None,
            issued: 0,
            reply: None,
            invalidated: false,
            outcome: None,
        }
    }
    pub fn terminal_custody(&self) -> Custody {
        if let Some(outcome) = self.outcome {
            return outcome;
        }
        match self.reply {
            Some(Err(e)) if e == Errno::EAGAIN && self.confirmed == 0 => Custody::DroppedUnsent,
            Some(Ok(n))
                if n > 0
                    && n as usize <= self.issued
                    && self.confirmed + n as usize == self.bytes.len() =>
            {
                Custody::Stored
            }
            Some(Err(e)) if e == Errno::EINVAL && self.invalidated => Custody::DroppedUnsent,
            Some(Err(e)) if e != Errno::EAGAIN => Custody::Refused(e.0),
            _ if self.tag.is_some() || self.confirmed > 0 => Custody::Unknown,
            _ => Custody::DroppedUnsent,
        }
    }
}
pub(super) struct Window {
    resource: ContentResourceId,
    pub units: [Option<Unit>; WIDTH],
    pub not_before: Option<(Instant, u64)>,
    draining: bool,
    fenced: bool,
    clunk: Option<Fid>,
}
impl Window {
    pub fn new(resource: ContentResourceId, offset: u64, bytes: Box<[u8]>, ticket: u64) -> Self {
        let mut units = std::array::from_fn(|_| None);
        units[0] = Some(Unit::new(offset, bytes, ticket));
        Self {
            resource,
            units,
            not_before: None,
            draining: false,
            fenced: false,
            clunk: None,
        }
    }
    fn len(&self) -> usize {
        self.units.iter().flatten().count()
    }
    fn outstanding(&self) -> bool {
        self.units.iter().flatten().any(|u| u.tag.is_some())
    }
    fn pop(&mut self) -> Unit {
        let unit = self.units[0].take().expect("nonempty window");
        self.units.rotate_left(1);
        unit
    }
}
impl FileWire {
    pub(super) fn on_slot_open_reply(
        &mut self,
        tag: Tag,
        reply: &Reply,
    ) -> Result<bool, ShellClientError> {
        for slot in 0..self.upload_slots as usize {
            let Some(mut upload) = self.uploads[slot] else {
                continue;
            };
            let UploadState::Opening(mut open) = upload.state else {
                continue;
            };
            if open.open == Some(tag)
                && let Reply::Lopen { iounit, .. } = reply
            {
                upload.iounit = *iounit;
            }
            upload.state = match open.receive(&mut self.pipeline, tag, reply)? {
                Some(true) => UploadState::Open(open.fid),
                Some(false) => UploadState::Opening(open),
                None => continue,
            };
            self.uploads[slot] = Some(upload);
            return Ok(true);
        }
        Ok(false)
    }
    fn begin_slot_open(&mut self, slot: usize) -> Result<(), ShellClientError> {
        let name = slot.to_string();
        let opening = Opening::start(
            &mut self.pipeline,
            self.root,
            &[b"upload", name.as_bytes()],
            O_WRONLY,
        )?;
        if let Some(upload) = self.uploads[slot].as_mut() {
            upload.state = UploadState::Opening(opening);
        }
        Ok(())
    }
    fn settle_window_front(&mut self, window: &mut Window, custody: Custody) {
        self.custody.push((window.pop().ticket, custody));
        self.retire += 1;
    }
    fn retire_window(&mut self, w: &mut Window) -> Result<(), ShellClientError> {
        if w.draining {
            for unit in w.units.iter_mut().flatten() {
                let Some(reply) = unit.reply.take() else {
                    continue;
                };
                unit.reply = Some(reply);
                if !w.fenced {
                    match reply {
                        Err(e) if e == Errno::EINVAL => (),
                        Err(e) if e == Errno::ESTALE => w.fenced = true,
                        _ => {
                            return Err(ShellClientError::Protocol(
                                "upload tail accepted after failed head",
                            ));
                        }
                    }
                }
                if w.fenced {
                    unit.outcome = Some(unit.terminal_custody());
                }
                unit.reply = None;
                unit.tag = None;
                unit.issued = 0;
            }
            if w.outstanding() {
                return Ok(());
            }
            w.draining = false;
        }
        if w.fenced {
            // A permanent refusal ends this binding locally too. Later queued
            // chunks must not resume a resource whose window was abandoned.
            for upload in &mut self.uploads {
                if upload.as_ref().is_some_and(|u| u.resource == w.resource) {
                    if let Some(UploadState::Open(fid)) = upload.as_ref().map(|u| u.state) {
                        w.clunk = Some(fid);
                    }
                    *upload = None;
                }
            }
            // A fence forbids replay. Replies drain before the writer closes.
            if w.outstanding() {
                w.draining = true;
                return self.retire_window(w);
            }
            while let Some(unit) = &w.units[0] {
                self.settle_window_front(w, unit.terminal_custody());
            }
            w.not_before = None;
            if let Some(fid) = w.clunk.take() {
                self.forget(fid)?;
            }
            return Ok(());
        }
        while let Some(unit) = w.units[0].as_mut() {
            if let Some(Ok(count)) = unit.reply
                && (count == 0 || count as usize > unit.issued)
            {
                // The request left the client, but this reply establishes no
                // accepted prefix. Preserve that uncertainty through teardown.
                unit.outcome = Some(Custody::Unknown);
                return Err(ShellClientError::Protocol("unexpected slot write reply"));
            }
            let Some(reply) = unit.reply.take() else {
                break;
            };
            unit.tag = None;
            match reply {
                Ok(count) if count > 0 && count as usize <= unit.issued => {
                    unit.confirmed += count as usize;
                    let short = (count as usize) < unit.issued;
                    unit.issued = 0;
                    self.backoff = RETRY_FIRST;
                    if unit.confirmed == unit.bytes.len() {
                        self.settle_window_front(w, Custody::Stored);
                    }
                    if short {
                        w.draining = true;
                        break;
                    }
                }
                Err(e) if e == Errno::EAGAIN => {
                    unit.issued = 0;
                    w.not_before = Some((Instant::now() + self.next_backoff(), self.pass));
                    w.draining = true;
                    break;
                }
                Err(e) => {
                    self.settle_window_front(w, Custody::Refused(e.0));
                    w.fenced = true;
                    w.draining = true;
                    break;
                }
                _ => return Err(ShellClientError::Protocol("unexpected slot write reply")),
            }
        }
        if w.draining {
            for unit in w.units.iter_mut().flatten() {
                if unit.tag.is_some() {
                    unit.invalidated = true;
                }
            }
            self.retire_window(w)?;
        }
        Ok(())
    }
    pub(super) fn on_slot_write_reply(
        &mut self,
        tag: Tag,
        reply: &Reply,
    ) -> Result<bool, ShellClientError> {
        let Some(Current::SlotWrite(w)) = &mut self.current else {
            return Ok(false);
        };
        let Some(unit) = w.units.iter_mut().flatten().find(|u| u.tag == Some(tag)) else {
            return Ok(false);
        };
        unit.reply = Some(match reply {
            Reply::Write(n) => Ok(*n),
            Reply::Error(e) => Err(*e),
            _ => return Err(ShellClientError::Protocol("unexpected slot write reply")),
        });
        let Some(Current::SlotWrite(mut w)) = self.current.take() else {
            unreachable!()
        };
        let result = self.retire_window(&mut w);
        // Keep unresolved tickets available to terminal classification on error.
        if w.len() > 0 {
            self.current = Some(Current::SlotWrite(w));
        }
        result.map(|()| true)
    }
    pub(super) fn drive_slot_write(
        &mut self,
        output: &mut ClientOutbox,
    ) -> Result<bool, ShellClientError> {
        if !matches!(self.current, Some(Current::SlotWrite(_))) {
            return Ok(false);
        }
        let Some(Current::SlotWrite(mut w)) = self.current.take() else {
            unreachable!()
        };
        let result = self.fill_window(&mut w, output);
        if w.len() > 0 {
            self.current = Some(Current::SlotWrite(w));
        }
        result
    }
    fn fill_window(
        &mut self,
        w: &mut Window,
        output: &mut ClientOutbox,
    ) -> Result<bool, ShellClientError> {
        if w.draining
            || w.fenced
            || w.not_before
                .is_some_and(|(at, pass)| self.pass == pass || Instant::now() < at)
        {
            return Ok(false);
        }
        let Some(fid) = open_fid_for(&self.uploads, w.resource) else {
            if w.outstanding() {
                w.fenced = true;
                self.retire_window(w)?;
            } else {
                while w.len() > 0 {
                    self.settle_window_front(w, Custody::DroppedUnsent);
                }
            }
            return Ok(true);
        };
        let iounit = self
            .uploads
            .iter()
            .flatten()
            .find(|u| u.resource == w.resource)
            .unwrap()
            .iounit;
        let mut budget = self.pipeline.msize().saturating_sub(WRITE_OVERHEAD) as usize;
        if iounit > 0 {
            budget = budget.min(iounit as usize);
        }
        // Outbox units are canonical chunks; never cross records/resources/gaps.
        while w.len() < WIDTH {
            let Some(QueuedKind::SlotWrite { resource, offset }) = self.pending.front().copied()
            else {
                break;
            };
            let last = w.units.iter().flatten().last().unwrap();
            if resource != w.resource || offset != last.offset + last.bytes.len() as u64 {
                break;
            }
            let index = w.len();
            let ticket = output.ticket_at(index);
            let bytes = output.take_bytes(index);
            w.units[index] = Some(Unit::new(offset, bytes, ticket));
            self.pending.pop_front();
            self.custody.push((ticket, Custody::InFlight));
        }
        let mut progressed = false;
        for unit in w.units.iter_mut().flatten() {
            if let Some(reply) = unit.reply
                && !matches!(reply, Ok(n) if n as usize == unit.issued)
            {
                break;
            }
            if unit.tag.is_none() {
                if self.pipeline.outstanding() + 5
                    >= self.pipeline.limits().max_outstanding as usize
                {
                    break;
                }
                let count = (unit.bytes.len() - unit.confirmed).min(budget);
                let tag = match self.pipeline.write(
                    fid,
                    unit.offset + unit.confirmed as u64,
                    &unit.bytes[unit.confirmed..unit.confirmed + count],
                ) {
                    Ok(tag) => tag,
                    Err(PipelineError::Limit(_)) => break,
                    Err(e) => return Err(e.into()),
                };
                unit.tag = Some(tag);
                unit.invalidated = false;
                unit.issued = count;
                w.not_before = None;
                progressed = true;
            }
            // Confirm fragmented prefixes before another fragment. The last
            // fragment can share its window with following canonical chunks.
            if unit.confirmed + unit.issued < unit.bytes.len() {
                break;
            }
        }
        Ok(progressed)
    }
    pub(super) fn observe_resource_status(
        &mut self,
        resource: ContentResourceId,
        status: u16,
    ) -> Result<(), ShellClientError> {
        for slot in 0..self.upload_slots as usize {
            let Some(upload) = self.uploads[slot] else {
                continue;
            };
            if upload.resource != resource {
                continue;
            }
            if status == 1 {
                self.begin_slot_open(slot)?;
            } else {
                let fid = match upload.state {
                    UploadState::Open(fid) => Some(fid),
                    UploadState::Opening(o) => Some(o.fid),
                    UploadState::Pending => None,
                };
                if let UploadState::Opening(o) = upload.state {
                    self.forgettable.extend(o.walk);
                    self.forgettable.extend(o.open);
                }
                self.uploads[slot] = None;
                if let Some(Current::SlotWrite(w)) = &mut self.current
                    && w.resource == resource
                {
                    w.fenced = true;
                    w.clunk = fid;
                    let Some(Current::SlotWrite(mut w)) = self.current.take() else {
                        unreachable!()
                    };
                    let result = self.retire_window(&mut w);
                    if w.len() > 0 {
                        self.current = Some(Current::SlotWrite(w));
                    }
                    result?;
                } else if let Some(fid) = fid {
                    self.forget(fid)?;
                }
            }
        }
        Ok(())
    }
}
