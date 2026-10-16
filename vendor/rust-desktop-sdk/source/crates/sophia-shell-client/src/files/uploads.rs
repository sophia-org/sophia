//! Upload-slot handling: once a bound resource's `ResourceStatus` admits it
//! (mirroring the export's own bookkeeping in `observe_resource_status`),
//! walk and open its `upload/<slot>` writer fid, then drive the current
//! slot-write unit's actual writes, splitting to fit `msize` and resuming
//! after a short write at the returned count. A terminal status frees the
//! slot and clunks whatever fid it held, at whatever phase it was in.
use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Tag};
use sophia_shell_protocol::ContentResourceId;

use super::{
    Current, FileWire, O_WRONLY, Opening, RETRY_FIRST, Upload, UploadState, WRITE_OVERHEAD,
    open_fid_for,
};
use crate::ShellClientError;
use crate::custody::Custody;

impl FileWire {
    /// Withholds payload until both replies for an admitted slot succeed.
    pub(super) fn on_slot_open_reply(
        &mut self,
        tag: Tag,
        reply: &Reply,
    ) -> Result<bool, ShellClientError> {
        for slot in 0..self.upload_slots as usize {
            let Some(upload) = self.uploads[slot] else {
                continue;
            };
            let mut open = match upload.state {
                UploadState::Opening(open) => open,
                _ => continue,
            };
            let new_state = match open.receive(&mut self.pipeline, tag, reply)? {
                Some(true) => UploadState::Open(open.fid),
                Some(false) => UploadState::Opening(open),
                None => continue,
            };
            self.uploads[slot] = Some(Upload {
                resource: upload.resource,
                state: new_state,
            });
            return Ok(true);
        }
        Ok(false)
    }

    /// Issues the walk that starts opening `upload/<slot>` for writing, right
    /// after `observe_resource_status` admits it.
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

    pub(super) fn on_slot_write_reply(
        &mut self,
        tag: Tag,
        reply: &Reply,
    ) -> Result<bool, ShellClientError> {
        let is_mine = matches!(
            &self.current,
            Some(Current::SlotWrite { tag: Some(t), .. }) if *t == tag
        );
        if !is_mine {
            return Ok(false);
        }
        let progress_backoff = match reply {
            Reply::Error(errno) if *errno == Errno::EAGAIN => {
                Some((std::time::Instant::now() + self.next_backoff(), self.pass))
            }
            _ => None,
        };
        let Some(Current::SlotWrite {
            offset,
            remaining,
            acknowledged,
            tag: current_tag,
            not_before,
            wrote_any,
            ..
        }) = &mut self.current
        else {
            unreachable!("checked above");
        };
        match reply {
            Reply::Write(count)
                if (*count as usize) <= remaining.len() - *acknowledged && *count > 0 =>
            {
                let count = *count as usize;
                *acknowledged += count;
                *offset += count as u64;
                *current_tag = None;
                *wrote_any = true;
                if *acknowledged == remaining.len() {
                    self.backoff = RETRY_FIRST;
                    self.settle_current(Custody::Stored);
                }
            }
            Reply::Error(errno) if *errno == Errno::EAGAIN => {
                // Nothing transferred; write again after a backoff, never in
                // the pass that saw the refusal.
                *current_tag = None;
                *not_before = progress_backoff;
            }
            Reply::Error(errno) => {
                // A node-specific refusal (`ESTALE` once the resource is
                // fenced): these bytes are not stored, and the connection
                // continues.
                let errno = errno.0;
                self.settle_current(Custody::Refused(errno));
            }
            _ => return Err(ShellClientError::Protocol("unexpected slot write reply")),
        }
        Ok(true)
    }

    /// Issues the next write for the current slot-write unit, splitting to
    /// fit `msize` and resuming after a short write at the returned count.
    pub(super) fn drive_slot_write(&mut self) -> Result<bool, ShellClientError> {
        let (resource, ready) = match &self.current {
            Some(Current::SlotWrite {
                resource,
                tag,
                remaining,
                acknowledged,
                not_before,
                ..
            }) => (
                *resource,
                tag.is_none()
                    && *acknowledged < remaining.len()
                    && not_before.is_none_or(|(at, pass)| {
                        self.pass != pass && std::time::Instant::now() >= at
                    }),
            ),
            _ => return Ok(false),
        };
        if !ready {
            return Ok(false);
        }
        let Some(fid) = open_fid_for(&self.uploads, resource) else {
            // The binding vanished (fenced/terminal) between queueing and
            // writing: these bytes never left the client.
            self.settle_current(Custody::DroppedUnsent);
            return Ok(true);
        };
        let budget = (self.pipeline.msize().saturating_sub(WRITE_OVERHEAD)) as usize;
        let Some(Current::SlotWrite {
            offset,
            remaining,
            acknowledged,
            tag,
            ..
        }) = &mut self.current
        else {
            unreachable!("checked above");
        };
        let take = (remaining.len() - *acknowledged).min(budget.max(1));
        let write_tag = self.pipeline.write(
            fid,
            *offset,
            &remaining[*acknowledged..*acknowledged + take],
        )?;
        *tag = Some(write_tag);
        if let Some(Current::SlotWrite { not_before, .. }) = &mut self.current {
            *not_before = None;
        }
        Ok(true)
    }

    /// Mirrors the export's own slot bookkeeping: `status == 1` admits the
    /// bound slot for opening; any other status is terminal and frees it.
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
                let fid_to_clunk = match upload.state {
                    UploadState::Open(fid) => Some(fid),
                    UploadState::Opening(opening) => Some(opening.fid),
                    UploadState::Pending => None,
                };
                if let UploadState::Opening(opening) = upload.state {
                    self.forgettable.extend(opening.walk);
                    self.forgettable.extend(opening.open);
                }
                self.uploads[slot] = None;
                if let Some(fid) = fid_to_clunk {
                    self.forget(fid)?;
                }
            }
        }
        Ok(())
    }
}
