//! The blocking connect+negotiate handshake, bounded by
//! `options.handshake_timeout`: Pipeline connect, attach, open the fixed
//! nodes, stage and submit the `Negotiate` candidate, then read `events`
//! until `Negotiated` or `Refused`. Mirrors
//! `tests/support/shell_file_peer.rs`'s working sequence exactly.
use std::path::Path;
use std::time::Instant;

use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_9p_records::{Fid, Tag};

use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::{
    ContentLimits, ShellContentRecord, ShellV1ClientHello, ShellV1ServerWelcome,
};

use super::{FileWire, O_RDONLY, O_RDWR, O_WRONLY};
use crate::wire::Inbound;
use crate::{ShellClientError, ShellClientOptions};
use std::collections::VecDeque;

/// `Pipeline::wait`, converted to `ShellClientError`. `Pipeline::wait` itself
/// already recovers a reply that arrived just before the peer closed (a
/// following nonblocking read hitting real EOF within the same internal
/// `poll` call no longer loses a reply `read_input` had already resolved);
/// this wrapper only exists for the error-type conversion at each call site.
fn wait_ok(
    pipeline: &mut Pipeline,
    tag: Tag,
    deadline: Instant,
) -> Result<Reply, ShellClientError> {
    Ok(pipeline.wait(tag, deadline)?)
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

/// Reads a whole immutable object through `fid`, blocking within
/// `deadline`: a positive short read is not the end, only a zero-length read
/// is, and holding exactly `cap` bytes takes a 1-byte read to prove it.
fn read_object(
    pipeline: &mut Pipeline,
    fid: Fid,
    iounit: u32,
    cap: usize,
    deadline: Instant,
) -> Result<Vec<u8>, ShellClientError> {
    let count = if iounit == 0 { u32::MAX } else { iounit };
    let mut bytes = Vec::new();
    loop {
        let want = if bytes.len() == cap { 1 } else { count };
        let tag = pipeline.read(fid, bytes.len() as u64, want)?;
        let data = match wait_ok(pipeline, tag, deadline)? {
            Reply::Read(data) => data,
            _ => return Err(ShellClientError::Protocol("object read refused")),
        };
        if data.is_empty() {
            return Ok(bytes);
        }
        if bytes.len() + data.len() > cap {
            return Err(ShellClientError::Protocol("object over its cap"));
        }
        bytes.extend_from_slice(&data);
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

/// Whether `error` is only the connection having closed. Tolerated
/// specifically for the ack of the handshake's *terminal* event
/// (`Negotiated`/`Refused`): the server's own revocation is gated on having
/// processed that ack (it journals nothing more once every retained record
/// is acked), so by the time this ack's own write reaches the wire the
/// server may revoke and close before its `Rwrite` reply gets back -- a
/// same-turn race between generating that reply and disconnecting (see the
/// final report). The client already has everything it needs from the
/// decoded event itself; the ack write having been *sent* is what matters,
/// not seeing its own reply.
fn is_peer_closed(error: &ShellClientError) -> bool {
    matches!(
        error,
        ShellClientError::Pipeline(sophia_9p_client::pipeline::PipelineError::Io(kind))
            if super::is_disconnect(*kind)
    )
}

impl FileWire {
    /// Connects, attaches, opens the fixed nodes and negotiates, all bounded
    /// by `options.handshake_timeout`. The codec requires every record's
    /// header to carry the attach's exact connection epoch; the client learns
    /// it from `api` (read right after attach, before any candidate is
    /// staged), which the export discloses precisely for this purpose.
    pub(crate) fn connect(
        path: &Path,
        options: &ShellClientOptions,
    ) -> Result<(Self, ShellV1ServerWelcome, VecDeque<Inbound>), ShellClientError> {
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

        let api_fid = open_fixed(&mut pipeline, root, b"api", O_RDONLY, deadline)?;
        let tag = pipeline.read(api_fid, 0, crate::SHELL_FILES_API_LINE_MAX_BYTES)?;
        let api_line = match wait_ok(&mut pipeline, tag, deadline)? {
            Reply::Read(data) => data,
            _ => return Err(ShellClientError::Protocol("api read refused")),
        };
        // These clunks' own replies arrive only once `poll_io` starts
        // draining the pipeline, well after `connect` returns; the built
        // `FileWire` must already know to treat them as forgettable.
        let mut pending_forgettable = vec![pipeline.clunk(api_fid)?];
        let connection_epoch = crate::parse_shell_files_api_line(&api_line)?;

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
        pending_forgettable.push(pipeline.clunk(txn_fid)?);

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
                        if let Err(error) =
                            ack_now(&mut pipeline, ack_fid, connection_epoch, sequence, deadline)
                            && !is_peer_closed(&error)
                        {
                            return Err(error);
                        }
                        break 'outer (Ok(negotiated), sequence);
                    }
                    ShellFileKind::Refused => {
                        let refused = decode_shell_file_refused(&record)?;
                        if let Err(error) =
                            ack_now(&mut pipeline, ack_fid, connection_epoch, sequence, deadline)
                            && !is_peer_closed(&error)
                        {
                            return Err(error);
                        }
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
            let (tag, fid) = pipeline.walk(root, &[b"limits"])?;
            match wait_ok(&mut pipeline, tag, deadline)? {
                Reply::Walk(qids) if qids.len() == 1 => {}
                _ => return Err(ShellClientError::Protocol("limits walk refused")),
            }
            let tag = pipeline.lopen(fid, O_RDONLY)?;
            let iounit = match wait_ok(&mut pipeline, tag, deadline)? {
                Reply::Lopen { iounit, .. } => iounit,
                _ => return Err(ShellClientError::Protocol("limits open refused")),
            };
            let data = read_object(
                &mut pipeline,
                fid,
                iounit,
                SHELL_FILE_MAX_OBJECT_BYTES,
                deadline,
            )?;
            pending_forgettable.push(pipeline.clunk(fid)?);
            let limits: ContentLimits = decode_shell_file_limits(&data)?;
            upload_slots = limits
                .max_open_transfers
                .min(u32::from(SHELL_FILE_MAX_UPLOAD_SLOTS)) as u8;
            inbox.push_back(Inbound::Content(
                sophia_shell_protocol::TransactionId::INVALID,
                ShellContentRecord::Limits(limits),
            ));
        }

        let wire = FileWire {
            pipeline,
            epoch: connection_epoch,
            capabilities: welcome.capabilities,
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
            holds: [None; 3],
            progress: 0,
            pass: 0,
            service_pending: true,
            early_submitted: None,
            submitted_sequence: 0,
            pending: VecDeque::new(),
            staged: Vec::new(),
            staged_begin: None,
            current: None,
            backoff: super::RETRY_FIRST,
            custody: Vec::new(),
            retire: 0,
            uploads: Default::default(),
            forgettable: pending_forgettable.into_iter().collect(),
            peer_closed: false,
            event_fault: false,
            fatal: None,
        };
        Ok((wire, welcome, inbox))
    }
}
