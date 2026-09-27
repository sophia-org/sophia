//! The blocking connect+negotiate handshake, bounded by
//! `options.handshake_timeout`: Pipeline connect, attach, open the fixed
//! nodes, stage and submit the `Negotiate` candidate, then read `events`
//! until `Negotiated` or `Refused`. Mirrors
//! `tests/support/shell_file_peer.rs`'s working sequence exactly.
use std::path::Path;
use std::time::Instant;

use sophia_9p::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_9p::records::{Fid, Tag};

use sophia_protocol::shell_files::*;
use sophia_protocol::{
    ContentLimits, ShellContentRecord, ShellV1ClientHello, ShellV1ServerWelcome,
};

use super::{FileWire, O_RDONLY, O_RDWR, O_WRONLY};
use crate::wire::Inbound;
use crate::{ShellClientError, ShellClientOptions};
use std::collections::VecDeque;

/// `Pipeline::wait`, recovering a reply that already landed in its completed
/// queue even when the call itself returns an I/O error: `read_input`'s own
/// loop can drain and resolve a reply, then immediately hit a real EOF on a
/// following nonblocking read within that *same* call, and propagates that
/// EOF without ever re-checking what it just resolved. `take_reply` still
/// sees it (poisoning never clears the completed queue), so recover it here
/// before reporting the connection broken.
fn wait_ok(
    pipeline: &mut Pipeline,
    tag: Tag,
    deadline: Instant,
) -> Result<Reply, ShellClientError> {
    match pipeline.wait(tag, deadline) {
        Ok(reply) => Ok(reply),
        Err(error) => {
            while let Some((found, reply)) = pipeline.take_reply() {
                if found == tag {
                    return Ok(reply);
                }
            }
            Err(error.into())
        }
    }
}

/// Walks one fixed root name and opens it, blocking within `deadline`. Used
/// only during `connect`, before the pipeline switches to the nonblocking
/// per-round state machines `poll_io` drives.
fn open_fixed(
    pipeline: &mut Pipeline,
    root: Fid,
    name: &'static [u8],
    flags: u32,
    deadline: Instant,
) -> Result<Fid, ShellClientError> {
    let (tag, fid) = pipeline.walk(root, &[name])?;
    match wait_ok(pipeline, tag, deadline)? {
        Reply::Walk(qids) if qids.len() == 1 => {}
        _ => return Err(ShellClientError::Protocol("fixed node walk refused")),
    }
    let tag = pipeline.lopen(fid, flags)?;
    match wait_ok(pipeline, tag, deadline)? {
        Reply::Lopen { .. } => Ok(fid),
        _ => Err(ShellClientError::Protocol("fixed node open refused")),
    }
}

fn ack_now(
    pipeline: &mut Pipeline,
    ack_fid: Fid,
    connection_epoch: u64,
    sequence: u64,
    deadline: Instant,
) -> Result<(), ShellClientError> {
    let bytes = encode_shell_file_ack(ShellFileAck {
        connection_epoch,
        sequence,
    })?;
    let tag = pipeline.write(ack_fid, 0, &bytes)?;
    match wait_ok(pipeline, tag, deadline)? {
        Reply::Write(count) if count as usize == bytes.len() => Ok(()),
        _ => Err(ShellClientError::Protocol("ack write refused")),
    }
}

impl FileWire {
    /// Connects, attaches, opens the fixed nodes and negotiates, all bounded
    /// by `options.handshake_timeout`. The codec requires every record's
    /// header to carry the attach's exact `connection_epoch`, which the file
    /// contract gives no in-band way to learn before the first submission;
    /// `connection_epoch` is Session's own pre-assignment for this attach
    /// (see the final report).
    pub(crate) fn connect(
        path: &Path,
        connection_epoch: u64,
        options: &ShellClientOptions,
    ) -> Result<(Self, ShellV1ServerWelcome, VecDeque<Inbound>), ShellClientError> {
        if connection_epoch == 0 {
            return Err(ShellClientError::Environment(
                "file wire connection epoch must be nonzero",
            ));
        }
        let deadline = Instant::now()
            .checked_add(options.handshake_timeout)
            .ok_or(ShellClientError::Protocol("handshake deadline overflow"))?;
        let mut pipeline =
            Pipeline::connect(path, PipelineLimits::default(), options.handshake_timeout)?;
        let (tag, root) = pipeline.attach(&[], &[])?;
        match wait_ok(&mut pipeline, tag, deadline)? {
            Reply::Attach(_) => {}
            _ => return Err(ShellClientError::Protocol("attach refused")),
        }
        let events_fid = open_fixed(&mut pipeline, root, b"events", O_RDONLY, deadline)?;
        let submit_fid = open_fixed(&mut pipeline, root, b"submit", O_WRONLY, deadline)?;
        let ack_fid = open_fixed(&mut pipeline, root, b"ack", O_WRONLY, deadline)?;

        let submission_id = 1u64;
        let header = ShellFileHeader {
            kind: ShellFileKind::Negotiate,
            connection_epoch,
            submission_id,
            sequence: 0,
        };
        let hello = ShellV1ClientHello {
            minimum_revision: options.minimum_revision,
            maximum_revision: options.maximum_revision,
            required_capabilities: options.required_capabilities,
        };
        let record_bytes = encode_shell_file_negotiate(header, hello)?;

        let txn_fid = open_fixed(&mut pipeline, root, b"transaction", O_RDWR, deadline)?;
        let tag = pipeline.write(txn_fid, 0, &record_bytes)?;
        match wait_ok(&mut pipeline, tag, deadline)? {
            Reply::Write(count) if count as usize == record_bytes.len() => {}
            _ => return Err(ShellClientError::Protocol("negotiate record write refused")),
        }
        let submit_bytes = encode_shell_file_submit(ShellFileSubmit {
            connection_epoch,
            submission_id,
            candidate_bytes: record_bytes.len() as u32,
        })?;
        let tag = pipeline.write(submit_fid, 0, &submit_bytes)?;
        match wait_ok(&mut pipeline, tag, deadline)? {
            Reply::Write(count) if count as usize == submit_bytes.len() => {}
            _ => return Err(ShellClientError::Protocol("negotiate submit refused")),
        }
        // These clunks' own replies arrive only once `poll_io` starts
        // draining the pipeline, well after `connect` returns; the built
        // `FileWire` must already know to treat them as forgettable.
        let mut pending_forgettable = vec![pipeline.clunk(txn_fid)?];

        let mut read_offset = 0u64;
        let mut buffer: Vec<u8> = Vec::new();
        let (outcome, last_acked) = 'outer: loop {
            let tag = pipeline.read(events_fid, read_offset, u32::MAX)?;
            let data = match wait_ok(&mut pipeline, tag, deadline)? {
                Reply::Read(data) => data,
                _ => return Err(ShellClientError::Protocol("events read refused")),
            };
            read_offset += data.len() as u64;
            buffer.extend_from_slice(&data);
            loop {
                if buffer.len() < 4 {
                    break;
                }
                let size = u32::from_le_bytes(buffer[..4].try_into().unwrap()) as usize;
                if size < 4 || buffer.len() < size {
                    break;
                }
                let record: Vec<u8> = buffer.drain(..size).collect();
                let parsed = decode_shell_file_record(&record, ShellFileClass::Event)?;
                let sequence = parsed.header.sequence;
                match parsed.header.kind {
                    ShellFileKind::Submitted => {
                        let submitted = decode_shell_file_submitted(&record)?;
                        if submitted.submission_id != submission_id
                            || submitted.candidate_kind != ShellFileKind::Negotiate
                        {
                            return Err(ShellClientError::Protocol(
                                "unexpected Submitted before negotiation",
                            ));
                        }
                        ack_now(&mut pipeline, ack_fid, connection_epoch, sequence, deadline)?;
                    }
                    ShellFileKind::Negotiated => {
                        let negotiated = decode_shell_file_negotiated(&record)?;
                        ack_now(&mut pipeline, ack_fid, connection_epoch, sequence, deadline)?;
                        break 'outer (Ok(negotiated), sequence);
                    }
                    ShellFileKind::Refused => {
                        let refused = decode_shell_file_refused(&record)?;
                        ack_now(&mut pipeline, ack_fid, connection_epoch, sequence, deadline)?;
                        break 'outer (Err(refused), sequence);
                    }
                    _ => {
                        return Err(ShellClientError::Protocol(
                            "unexpected event before negotiation",
                        ));
                    }
                }
            }
        };

        let negotiated = match outcome {
            Ok(negotiated) => negotiated,
            Err(refused) => return Err(ShellClientError::AdmissionRefused(refused)),
        };
        let welcome = negotiated.welcome;
        if welcome.selected_revision < options.minimum_revision
            || welcome.selected_revision > options.maximum_revision
        {
            return Err(ShellClientError::UnsupportedRevision);
        }
        if welcome.capabilities & options.required_capabilities != options.required_capabilities {
            return Err(ShellClientError::MissingCapability);
        }

        let mut inbox = VecDeque::new();
        let mut upload_slots = 0u8;
        if negotiated.limits_published {
            let fid = open_fixed(&mut pipeline, root, b"limits", O_RDONLY, deadline)?;
            let tag = pipeline.read(fid, 0, u32::MAX)?;
            let data = match wait_ok(&mut pipeline, tag, deadline)? {
                Reply::Read(data) => data,
                _ => return Err(ShellClientError::Protocol("limits read refused")),
            };
            pending_forgettable.push(pipeline.clunk(fid)?);
            let limits: ContentLimits = decode_shell_file_limits(&data)?;
            upload_slots = limits
                .max_open_transfers
                .min(u32::from(SHELL_FILE_MAX_UPLOAD_SLOTS)) as u8;
            inbox.push_back(Inbound::Content(
                sophia_protocol::TransactionId::INVALID,
                ShellContentRecord::Limits(limits),
            ));
        }

        let wire = FileWire {
            pipeline,
            epoch: connection_epoch,
            root,
            events_fid,
            submit_fid,
            ack_fid,
            upload_slots,
            next_submission_id: 2,
            read_tag: None,
            read_offset,
            event_buf: buffer,
            ack_ready: last_acked,
            acked: last_acked,
            ack_tag: None,
            object_fetch: None,
            pending: VecDeque::new(),
            staged: None,
            staged_begin: None,
            current: None,
            uploads: Default::default(),
            forgettable: pending_forgettable.into_iter().collect(),
            peer_closed: false,
            fatal: None,
        };
        Ok((wire, welcome, inbox))
    }
}
