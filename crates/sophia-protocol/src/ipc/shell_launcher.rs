use super::cursor::{Cursor, push_u16, push_u32, push_u64};
use crate::*;

fn bad() -> IpcCodecError {
    IpcCodecError::InvalidRecord("shell_launcher")
}
fn require(ok: bool) -> Result<(), IpcCodecError> {
    if ok { Ok(()) } else { Err(bad()) }
}
fn put_text(b: &mut Vec<u8>, s: &str, max: usize) -> Result<(), IpcCodecError> {
    require(shell_launcher_text_valid(s, max))?;
    push_u16(b, s.len() as u16);
    b.extend_from_slice(s.as_bytes());
    Ok(())
}
fn get_text(c: &mut Cursor<'_>, max: usize) -> Result<String, IpcCodecError> {
    let n = c.u16()? as usize;
    require(n <= max)?;
    let s = std::str::from_utf8(c.slice(n)?)
        .map_err(|_| bad())?
        .to_owned();
    require(shell_launcher_text_valid(&s, max))?;
    Ok(s)
}
fn prefix(epoch: u64, generation: u64) -> Result<Vec<u8>, IpcCodecError> {
    require(epoch > 0 && generation > 0)?;
    let mut b = Vec::new();
    push_u64(&mut b, epoch);
    push_u64(&mut b, generation);
    Ok(b)
}
fn frame(kind: IpcMessageKind, tx: TransactionId, b: Vec<u8>) -> Result<Vec<u8>, IpcCodecError> {
    require(tx.is_valid())?;
    encode_frame(kind, tx, &b)
}
fn payload(f: &[u8], kind: IpcMessageKind) -> Result<(TransactionId, Cursor<'_>), IpcCodecError> {
    let (h, b) = decode_frame(f)?;
    require(h.message_kind == kind && h.transaction.is_valid())?;
    Ok((h.transaction, Cursor::new(b)))
}
pub fn encode_shell_application_catalog(
    tx: TransactionId,
    s: &ShellApplicationCatalog,
) -> Result<Vec<Vec<u8>>, IpcCodecError> {
    validate_shell_application_catalog(s)?;
    let mut b = prefix(s.connection_epoch, s.generation)?;
    push_u16(&mut b, s.entries.len() as u16);
    push_u16(&mut b, 0);
    let mut frames = vec![frame(IpcMessageKind::ShellApplicationsBegin, tx, b)?];
    for e in &s.entries {
        let mut b = prefix(s.connection_epoch, s.generation)?;
        push_u16(&mut b, e.slot);
        push_u16(&mut b, u16::from(e.available));
        put_text(&mut b, &e.label, 128)?;
        put_text(&mut b, &e.keywords, 256)?;
        frames.push(frame(IpcMessageKind::ShellApplicationsEntry, tx, b)?);
    }
    frames.push(frame(
        IpcMessageKind::ShellApplicationsEnd,
        tx,
        prefix(s.connection_epoch, s.generation)?,
    )?);
    Ok(frames)
}
pub fn decode_shell_application_catalog(
    frames: &[Vec<u8>],
) -> Result<(TransactionId, ShellApplicationCatalog), IpcCodecError> {
    require((2..=SOPHIA_SHELL_MAX_APPLICATIONS + 2).contains(&frames.len()))?;
    let (tx, mut c) = payload(&frames[0], IpcMessageKind::ShellApplicationsBegin)?;
    let epoch = c.u64()?;
    let generation = c.u64()?;
    let count = c.u16()? as usize;
    require(c.u16()? == 0 && frames.len() == count + 2)?;
    c.finish()?;
    let mut s = ShellApplicationCatalog {
        connection_epoch: epoch,
        generation,
        entries: Vec::with_capacity(count),
    };
    for f in &frames[1..frames.len() - 1] {
        let (t, mut c) = payload(f, IpcMessageKind::ShellApplicationsEntry)?;
        require(t == tx && c.u64()? == epoch && c.u64()? == generation)?;
        let slot = c.u16()?;
        let available = c.u16()?;
        require(available <= 1)?;
        let label = get_text(&mut c, 128)?;
        let keywords = get_text(&mut c, 256)?;
        c.finish()?;
        s.entries.push(ShellApplicationDescriptor {
            slot,
            available: available == 1,
            label,
            keywords,
        });
    }
    let (t, mut c) = payload(frames.last().unwrap(), IpcMessageKind::ShellApplicationsEnd)?;
    require(t == tx && c.u64()? == epoch && c.u64()? == generation)?;
    c.finish()?;
    validate_shell_application_catalog(&s)?;
    Ok((tx, s))
}
pub fn encode_shell_launcher_request(
    tx: TransactionId,
    r: &ShellLauncherRequest,
) -> Result<Vec<u8>, IpcCodecError> {
    require(r.request_generation > 0 && r.output.is_valid() && r.output_generation > 0)?;
    let mut b = prefix(r.connection_epoch, r.catalog_generation)?;
    for v in [
        r.request_generation,
        r.output.raw(),
        r.output_generation,
        r.presentation_epoch,
    ] {
        push_u64(&mut b, v);
    }
    push_u16(&mut b, r.operation as u16);
    push_u16(&mut b, 0);
    put_text(&mut b, &r.query, SOPHIA_SHELL_MAX_QUERY_BYTES)?;
    frame(IpcMessageKind::ShellLauncherRequest, tx, b)
}
pub fn decode_shell_launcher_request(
    f: &[u8],
) -> Result<(TransactionId, ShellLauncherRequest), IpcCodecError> {
    let (tx, mut c) = payload(f, IpcMessageKind::ShellLauncherRequest)?;
    let connection_epoch = c.u64()?;
    let catalog_generation = c.u64()?;
    let request_generation = c.u64()?;
    let output = OutputId::from_raw(c.u64()?);
    let output_generation = c.u64()?;
    let presentation_epoch = c.u64()?;
    let operation = match c.u16()? {
        0 => ShellLauncherOperation::Open,
        1 => ShellLauncherOperation::Query,
        2 => ShellLauncherOperation::Next,
        3 => ShellLauncherOperation::Previous,
        4 => ShellLauncherOperation::Dismiss,
        _ => return Err(bad()),
    };
    require(c.u16()? == 0)?;
    let query = get_text(&mut c, SOPHIA_SHELL_MAX_QUERY_BYTES)?;
    c.finish()?;
    let r = ShellLauncherRequest {
        connection_epoch,
        catalog_generation,
        request_generation,
        output,
        output_generation,
        presentation_epoch,
        operation,
        query,
    };
    encode_shell_launcher_request(tx, &r)?;
    Ok((tx, r))
}
pub fn validate_shell_launcher_candidate(r: &ShellLauncherCandidate) -> Result<(), IpcCodecError> {
    require(
        r.connection_epoch > 0
            && r.catalog_generation > 0
            && r.request_generation > 0
            && r.candidate_generation > 0
            && r.output.is_valid()
            && r.entries.len() <= SOPHIA_SHELL_MAX_LAUNCHER_ROWS
            && (10..=32).contains(&r.font_size)
            && r.colors[1..].iter().all(|c| c >> 24 == 255),
    )?;
    let mut slots = std::collections::BTreeSet::new();
    for slot in &r.entries {
        require(
            *slot > 0 && usize::from(*slot) <= SOPHIA_SHELL_MAX_APPLICATIONS && slots.insert(*slot),
        )?;
    }
    require(r.selected == 0 || slots.contains(&r.selected))
}
pub fn encode_shell_launcher_candidate(
    tx: TransactionId,
    r: &ShellLauncherCandidate,
) -> Result<Vec<u8>, IpcCodecError> {
    validate_shell_launcher_candidate(r)?;
    let mut b = prefix(r.connection_epoch, r.catalog_generation)?;
    for v in [r.request_generation, r.candidate_generation, r.output.raw()] {
        push_u64(&mut b, v);
    }
    for v in [
        u16::from(r.visible),
        r.selected,
        r.entries.len() as u16,
        r.font_size,
    ] {
        push_u16(&mut b, v);
    }
    for color in r.colors {
        push_u32(&mut b, color);
    }
    for slot in &r.entries {
        push_u16(&mut b, *slot);
    }
    frame(IpcMessageKind::ShellLauncherCandidate, tx, b)
}
pub fn decode_shell_launcher_candidate(
    f: &[u8],
) -> Result<(TransactionId, ShellLauncherCandidate), IpcCodecError> {
    let (tx, mut c) = payload(f, IpcMessageKind::ShellLauncherCandidate)?;
    let connection_epoch = c.u64()?;
    let catalog_generation = c.u64()?;
    let request_generation = c.u64()?;
    let candidate_generation = c.u64()?;
    let output = OutputId::from_raw(c.u64()?);
    let visible = c.u16()?;
    require(visible <= 1)?;
    let selected = c.u16()?;
    let n = c.u16()? as usize;
    require(n <= SOPHIA_SHELL_MAX_LAUNCHER_ROWS)?;
    let font_size = c.u16()?;
    let mut colors = [0; 4];
    for color in &mut colors {
        *color = c.u32()?;
    }
    let mut entries = Vec::with_capacity(n);
    for _ in 0..n {
        entries.push(c.u16()?);
    }
    c.finish()?;
    let r = ShellLauncherCandidate {
        connection_epoch,
        catalog_generation,
        request_generation,
        candidate_generation,
        output,
        visible: visible == 1,
        selected,
        entries,
        font_size,
        colors,
    };
    validate_shell_launcher_candidate(&r)?;
    Ok((tx, r))
}
pub fn encode_shell_launcher_outcome(
    tx: TransactionId,
    r: ShellLauncherOutcome,
) -> Result<Vec<u8>, IpcCodecError> {
    require(
        r.candidate_generation > 0
            && (r.kind != ShellV1CandidateOutcomeKind::Presented || r.presentation_epoch > 0),
    )?;
    let mut b = prefix(r.connection_epoch, r.request_generation)?;
    push_u64(&mut b, r.candidate_generation);
    push_u64(&mut b, r.presentation_epoch);
    push_u16(
        &mut b,
        match r.kind {
            ShellV1CandidateOutcomeKind::Prepared => 1,
            ShellV1CandidateOutcomeKind::Presented => 2,
            ShellV1CandidateOutcomeKind::Rejected => 3,
            ShellV1CandidateOutcomeKind::Superseded => 4,
        },
    );
    push_u16(&mut b, 0);
    frame(IpcMessageKind::ShellLauncherOutcome, tx, b)
}
pub fn decode_shell_launcher_outcome(
    f: &[u8],
) -> Result<(TransactionId, ShellLauncherOutcome), IpcCodecError> {
    let (tx, mut c) = payload(f, IpcMessageKind::ShellLauncherOutcome)?;
    let connection_epoch = c.u64()?;
    let request_generation = c.u64()?;
    let candidate_generation = c.u64()?;
    let presentation_epoch = c.u64()?;
    let kind = match c.u16()? {
        1 => ShellV1CandidateOutcomeKind::Prepared,
        2 => ShellV1CandidateOutcomeKind::Presented,
        3 => ShellV1CandidateOutcomeKind::Rejected,
        4 => ShellV1CandidateOutcomeKind::Superseded,
        _ => return Err(bad()),
    };
    require(c.u16()? == 0)?;
    c.finish()?;
    let r = ShellLauncherOutcome {
        connection_epoch,
        request_generation,
        candidate_generation,
        presentation_epoch,
        kind,
    };
    encode_shell_launcher_outcome(tx, r)?;
    Ok((tx, r))
}
fn put_activation(a: ShellLauncherActivation) -> Result<Vec<u8>, IpcCodecError> {
    let mut b = Vec::new();
    for v in [
        a.connection_epoch,
        a.catalog_generation,
        a.request_generation,
        a.candidate_generation,
        a.presentation_epoch,
        a.activation,
    ] {
        require(v > 0)?;
        push_u64(&mut b, v);
    }
    require(a.slot > 0 && usize::from(a.slot) <= SOPHIA_SHELL_MAX_APPLICATIONS)?;
    push_u16(&mut b, a.slot);
    Ok(b)
}
fn get_activation(c: &mut Cursor<'_>) -> Result<ShellLauncherActivation, IpcCodecError> {
    let a = ShellLauncherActivation {
        connection_epoch: c.u64()?,
        catalog_generation: c.u64()?,
        request_generation: c.u64()?,
        candidate_generation: c.u64()?,
        presentation_epoch: c.u64()?,
        activation: c.u64()?,
        slot: c.u16()?,
    };
    put_activation(a)?;
    Ok(a)
}
pub fn encode_shell_launcher_activation(
    tx: TransactionId,
    a: ShellLauncherActivation,
) -> Result<Vec<u8>, IpcCodecError> {
    let mut b = put_activation(a)?;
    push_u16(&mut b, 0);
    frame(IpcMessageKind::ShellLauncherActivation, tx, b)
}
pub fn decode_shell_launcher_activation(
    f: &[u8],
) -> Result<(TransactionId, ShellLauncherActivation), IpcCodecError> {
    let (tx, mut c) = payload(f, IpcMessageKind::ShellLauncherActivation)?;
    let a = get_activation(&mut c)?;
    require(c.u16()? == 0)?;
    c.finish()?;
    Ok((tx, a))
}
pub fn encode_shell_launcher_activation_ack(
    tx: TransactionId,
    a: ShellLauncherActivationAck,
) -> Result<Vec<u8>, IpcCodecError> {
    let mut b = put_activation(a.activation)?;
    push_u16(&mut b, u16::from(a.consumed));
    frame(IpcMessageKind::ShellLauncherActivationAck, tx, b)
}
pub fn decode_shell_launcher_activation_ack(
    f: &[u8],
) -> Result<(TransactionId, ShellLauncherActivationAck), IpcCodecError> {
    let (tx, mut c) = payload(f, IpcMessageKind::ShellLauncherActivationAck)?;
    let activation = get_activation(&mut c)?;
    let consumed = c.u16()?;
    require(consumed <= 1)?;
    c.finish()?;
    Ok((
        tx,
        ShellLauncherActivationAck {
            activation,
            consumed: consumed == 1,
        },
    ))
}
pub fn encode_shell_launch_outcome(
    tx: TransactionId,
    a: ShellLaunchOutcome,
) -> Result<Vec<u8>, IpcCodecError> {
    let mut b = put_activation(a.activation)?;
    push_u16(&mut b, a.status as u16);
    frame(IpcMessageKind::ShellLaunchOutcome, tx, b)
}
pub fn decode_shell_launch_outcome(
    f: &[u8],
) -> Result<(TransactionId, ShellLaunchOutcome), IpcCodecError> {
    let (tx, mut c) = payload(f, IpcMessageKind::ShellLaunchOutcome)?;
    let activation = get_activation(&mut c)?;
    let status = match c.u16()? {
        1 => ShellLaunchStatus::Started,
        2 => ShellLaunchStatus::Rejected,
        3 => ShellLaunchStatus::Failed,
        _ => return Err(bad()),
    };
    c.finish()?;
    Ok((tx, ShellLaunchOutcome { activation, status }))
}
