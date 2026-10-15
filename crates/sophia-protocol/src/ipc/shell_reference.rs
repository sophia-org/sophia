use super::cursor::{Cursor, push_u16, push_u32, push_u64};
use super::{IpcCodecError, IpcMessageKind, decode_frame, encode_frame};
use crate::*;

fn bad() -> IpcCodecError {
    IpcCodecError::InvalidRecord("shell_reference")
}
fn require(ok: bool) -> Result<(), IpcCodecError> {
    if ok { Ok(()) } else { Err(bad()) }
}
fn text_valid(s: &str, max: usize) -> bool {
    shell_launcher_text_valid(s, max)
}
fn put_text(b: &mut Vec<u8>, s: &str, max: usize) -> Result<(), IpcCodecError> {
    require(text_valid(s, max))?;
    push_u16(b, s.len() as u16);
    b.extend_from_slice(s.as_bytes());
    Ok(())
}
fn get_text(c: &mut Cursor<'_>, max: usize) -> Result<String, IpcCodecError> {
    let len = c.u16()? as usize;
    require(len <= max)?;
    let s = std::str::from_utf8(c.slice(len)?)
        .map_err(|_| bad())?
        .to_owned();
    require(text_valid(&s, max))?;
    Ok(s)
}
fn prefix(epoch: u64, generation: u64) -> Result<Vec<u8>, IpcCodecError> {
    require(epoch > 0 && generation > 0)?;
    let mut b = Vec::new();
    push_u64(&mut b, epoch);
    push_u64(&mut b, generation);
    Ok(b)
}
fn framed(kind: IpcMessageKind, tx: TransactionId, b: Vec<u8>) -> Result<Vec<u8>, IpcCodecError> {
    require(tx.is_valid())?;
    encode_frame(kind, tx, &b)
}
fn payload(f: &[u8], kind: IpcMessageKind) -> Result<(TransactionId, Cursor<'_>), IpcCodecError> {
    let (h, b) = decode_frame(f)?;
    require(h.message_kind == kind && h.transaction.is_valid())?;
    Ok((h.transaction, Cursor::new(b)))
}

pub fn validate_shell_shortcut_catalog(s: &ShellShortcutCatalog) -> Result<(), IpcCodecError> {
    sophia_shell_protocol::shell::reference::validate_shell_shortcut_catalog(s).map_err(Into::into)
}
pub fn encode_shell_shortcut_catalog(
    tx: TransactionId,
    s: &ShellShortcutCatalog,
) -> Result<Vec<Vec<u8>>, IpcCodecError> {
    validate_shell_shortcut_catalog(s)?;
    let mut b = prefix(s.connection_epoch, s.generation)?;
    push_u16(&mut b, s.entries.len() as u16);
    push_u16(&mut b, 0);
    let mut frames = vec![framed(IpcMessageKind::ShellShortcutsBegin, tx, b)?];
    for e in &s.entries {
        let mut b = prefix(s.connection_epoch, s.generation)?;
        push_u16(&mut b, e.slot);
        push_u16(&mut b, 0);
        put_text(&mut b, &e.chord, 64)?;
        put_text(&mut b, &e.action, 128)?;
        put_text(&mut b, e.label.as_deref().unwrap_or(""), 128)?;
        put_text(&mut b, e.group.as_deref().unwrap_or(""), 64)?;
        frames.push(framed(IpcMessageKind::ShellShortcutsEntry, tx, b)?);
    }
    frames.push(framed(
        IpcMessageKind::ShellShortcutsEnd,
        tx,
        prefix(s.connection_epoch, s.generation)?,
    )?);
    Ok(frames)
}
pub fn decode_shell_shortcut_catalog(
    frames: &[Vec<u8>],
) -> Result<(TransactionId, ShellShortcutCatalog), IpcCodecError> {
    require((2..=SOPHIA_SHELL_MAX_SHORTCUTS + 2).contains(&frames.len()))?;
    let (tx, mut c) = payload(&frames[0], IpcMessageKind::ShellShortcutsBegin)?;
    let epoch = c.u64()?;
    let generation = c.u64()?;
    let count = c.u16()? as usize;
    require(c.u16()? == 0 && frames.len() == count + 2)?;
    c.finish()?;
    let mut s = ShellShortcutCatalog {
        connection_epoch: epoch,
        generation,
        entries: Vec::new(),
    };
    for f in &frames[1..frames.len() - 1] {
        let (t, mut c) = payload(f, IpcMessageKind::ShellShortcutsEntry)?;
        require(t == tx && c.u64()? == epoch && c.u64()? == generation)?;
        let slot = c.u16()?;
        require(c.u16()? == 0)?;
        let chord = get_text(&mut c, 64)?;
        let action = get_text(&mut c, 128)?;
        let label = get_text(&mut c, 128)?;
        let group = get_text(&mut c, 64)?;
        c.finish()?;
        s.entries.push(ShellShortcut {
            slot,
            chord,
            action,
            label: (!label.is_empty()).then_some(label),
            group: (!group.is_empty()).then_some(group),
        });
    }
    let (t, mut c) = payload(frames.last().unwrap(), IpcMessageKind::ShellShortcutsEnd)?;
    require(t == tx && c.u64()? == epoch && c.u64()? == generation)?;
    c.finish()?;
    validate_shell_shortcut_catalog(&s)?;
    Ok((tx, s))
}
pub fn encode_shell_reference_request(
    tx: TransactionId,
    r: ShellReferenceRequest,
) -> Result<Vec<u8>, IpcCodecError> {
    require(r.request_generation > 0 && r.output.is_valid() && r.output_generation > 0)?;
    let mut b = prefix(r.connection_epoch, r.catalog_generation)?;
    for n in [
        r.request_generation,
        r.output.raw(),
        r.output_generation,
        r.presentation_epoch,
    ] {
        push_u64(&mut b, n);
    }
    push_u16(&mut b, r.operation as u16);
    push_u16(&mut b, 0);
    framed(IpcMessageKind::ShellReferenceRequest, tx, b)
}
pub fn decode_shell_reference_request(
    f: &[u8],
) -> Result<(TransactionId, ShellReferenceRequest), IpcCodecError> {
    let (tx, mut c) = payload(f, IpcMessageKind::ShellReferenceRequest)?;
    let mut r = ShellReferenceRequest {
        connection_epoch: c.u64()?,
        catalog_generation: c.u64()?,
        request_generation: c.u64()?,
        output: OutputId::from_raw(c.u64()?),
        output_generation: c.u64()?,
        presentation_epoch: c.u64()?,
        operation: ShellReferenceOperation::Startup,
    };
    r.operation = match c.u16()? {
        0 => ShellReferenceOperation::Startup,
        1 => ShellReferenceOperation::Toggle,
        2 => ShellReferenceOperation::Next,
        3 => ShellReferenceOperation::Previous,
        4 => ShellReferenceOperation::Dismiss,
        _ => return Err(bad()),
    };
    require(c.u16()? == 0)?;
    c.finish()?;
    encode_shell_reference_request(tx, r)?;
    Ok((tx, r))
}
pub fn validate_shell_reference_candidate(
    r: &ShellReferenceCandidate,
) -> Result<(), IpcCodecError> {
    sophia_shell_protocol::shell::reference::validate_shell_reference_candidate(r)
        .map_err(Into::into)
}
pub fn encode_shell_reference_candidate(
    tx: TransactionId,
    r: &ShellReferenceCandidate,
) -> Result<Vec<u8>, IpcCodecError> {
    validate_shell_reference_candidate(r)?;
    let mut b = prefix(r.connection_epoch, r.catalog_generation)?;
    for n in [r.request_generation, r.candidate_generation, r.output.raw()] {
        push_u64(&mut b, n);
    }
    for n in [u16::from(r.visible), r.page, r.entries.len() as u16, 0] {
        push_u16(&mut b, n);
    }
    let s = &r.style;
    for n in [
        s.body_size,
        s.title_size,
        s.padding,
        s.row_gap,
        s.key_gap,
        s.column_gap,
        s.border,
        s.margin,
        s.columns,
    ] {
        push_u16(&mut b, n);
    }
    for n in s.colors {
        push_u32(&mut b, n);
    }
    put_text(&mut b, &s.title, 128)?;
    for entry in &r.entries {
        push_u16(&mut b, entry.slot);
        push_u16(&mut b, 0);
        put_text(&mut b, &entry.key, 64)?;
        put_text(&mut b, &entry.label, 128)?;
    }
    framed(IpcMessageKind::ShellReferenceCandidate, tx, b)
}
pub fn decode_shell_reference_candidate(
    f: &[u8],
) -> Result<(TransactionId, ShellReferenceCandidate), IpcCodecError> {
    let (tx, mut c) = payload(f, IpcMessageKind::ShellReferenceCandidate)?;
    let epoch = c.u64()?;
    let catalog = c.u64()?;
    let request = c.u64()?;
    let candidate = c.u64()?;
    let output = OutputId::from_raw(c.u64()?);
    let visible = c.u16()?;
    let page = c.u16()?;
    let count = c.u16()? as usize;
    require(visible <= 1 && count <= SOPHIA_SHELL_MAX_SHORTCUTS && c.u16()? == 0)?;
    let mut s = ShellReferenceStyle {
        body_size: c.u16()?,
        title_size: c.u16()?,
        padding: c.u16()?,
        row_gap: c.u16()?,
        key_gap: c.u16()?,
        column_gap: c.u16()?,
        border: c.u16()?,
        margin: c.u16()?,
        columns: c.u16()?,
        colors: [0; 6],
        title: String::new(),
    };
    for n in &mut s.colors {
        *n = c.u32()?;
    }
    s.title = get_text(&mut c, 128)?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let slot = c.u16()?;
        require(c.u16()? == 0)?;
        entries.push(ShellReferenceEntry {
            slot,
            key: get_text(&mut c, 64)?,
            label: get_text(&mut c, 128)?,
        });
    }
    c.finish()?;
    let r = ShellReferenceCandidate {
        connection_epoch: epoch,
        catalog_generation: catalog,
        request_generation: request,
        candidate_generation: candidate,
        output,
        visible: visible == 1,
        page,
        style: s,
        entries,
    };
    validate_shell_reference_candidate(&r)?;
    Ok((tx, r))
}
pub fn encode_shell_reference_outcome(
    tx: TransactionId,
    r: ShellReferenceOutcome,
) -> Result<Vec<u8>, IpcCodecError> {
    require(
        r.request_generation > 0
            && r.candidate_generation > 0
            && r.pages > 0
            && r.page < r.pages
            && (r.kind != ShellV1CandidateOutcomeKind::Presented || r.presentation_epoch > 0),
    )?;
    let mut b = prefix(r.connection_epoch, r.catalog_generation)?;
    for n in [
        r.request_generation,
        r.candidate_generation,
        r.presentation_epoch,
    ] {
        push_u64(&mut b, n);
    }
    for n in [r.page, r.pages, r.kind as u16 + 1, 0] {
        push_u16(&mut b, n);
    }
    framed(IpcMessageKind::ShellReferenceOutcome, tx, b)
}
pub fn decode_shell_reference_outcome(
    f: &[u8],
) -> Result<(TransactionId, ShellReferenceOutcome), IpcCodecError> {
    let (tx, mut c) = payload(f, IpcMessageKind::ShellReferenceOutcome)?;
    let mut r = ShellReferenceOutcome {
        connection_epoch: c.u64()?,
        catalog_generation: c.u64()?,
        request_generation: c.u64()?,
        candidate_generation: c.u64()?,
        presentation_epoch: c.u64()?,
        page: c.u16()?,
        pages: c.u16()?,
        kind: ShellV1CandidateOutcomeKind::Rejected,
    };
    r.kind = match c.u16()? {
        1 => ShellV1CandidateOutcomeKind::Prepared,
        2 => ShellV1CandidateOutcomeKind::Presented,
        3 => ShellV1CandidateOutcomeKind::Rejected,
        4 => ShellV1CandidateOutcomeKind::Superseded,
        _ => return Err(bad()),
    };
    require(c.u16()? == 0)?;
    c.finish()?;
    encode_shell_reference_outcome(tx, r)?;
    Ok((tx, r))
}
