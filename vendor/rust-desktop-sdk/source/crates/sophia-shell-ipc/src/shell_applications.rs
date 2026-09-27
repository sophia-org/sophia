//! The revision-4 application catalog frames (Begin/Entry/End), which the
//! persistent catalog transaction carries inside it.
use super::cursor::{Cursor, push_u16, push_u64};
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
