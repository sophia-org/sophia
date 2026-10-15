use super::cursor::{Cursor, push_u16, push_u64};
use super::{IpcCodecError, IpcMessageKind, decode_frame, encode_frame};
use crate::*;

fn invalid() -> IpcCodecError {
    IpcCodecError::InvalidEnum {
        field: "shell_tabs",
        value: 0,
    }
}
fn frame(kind: IpcMessageKind, tx: TransactionId, data: Vec<u8>) -> Result<Vec<u8>, IpcCodecError> {
    if !tx.is_valid() {
        return Err(invalid());
    }
    encode_frame(kind, tx, &data)
}
fn prefix(epoch: u64, generation: u64) -> Vec<u8> {
    let mut b = Vec::new();
    push_u64(&mut b, epoch);
    push_u64(&mut b, generation);
    b
}

pub fn encode_shell_tab_snapshot(
    tx: TransactionId,
    s: &ShellTabSnapshot,
) -> Result<Vec<Vec<u8>>, IpcCodecError> {
    validate_shell_tab_snapshot(s)?;
    let mut b = prefix(s.connection_epoch, s.generation);
    push_u16(&mut b, s.groups.len() as u16);
    push_u16(
        &mut b,
        s.groups.iter().map(|g| g.entries.len()).sum::<usize>() as u16,
    );
    let mut frames = vec![frame(IpcMessageKind::ShellTabsBegin, tx, b)?];
    for g in &s.groups {
        let mut b = prefix(s.connection_epoch, s.generation);
        push_u64(&mut b, g.slot);
        push_u64(&mut b, g.output.raw());
        push_u16(&mut b, g.selected_slot.unwrap_or(0));
        push_u16(&mut b, u16::from(g.focused));
        push_u16(&mut b, g.entries.len() as u16);
        push_u16(&mut b, 0);
        frames.push(frame(IpcMessageKind::ShellTabsGroup, tx, b)?);
        // Each descriptor is one bounded record. Reuse the published descriptor
        // encoding inside the new transfer, without widening the r1 snapshot.
        for d in &g.entries {
            let one = ShellV1DescriptorSnapshot {
                connection_epoch: s.connection_epoch,
                snapshot_generation: s.generation,
                output: g.output,
                output_generation: 1,
                broker_epoch: d.action.issuer_epoch,
                broker_revocation_epoch: d.action.issuer_revocation_epoch,
                descriptors: vec![d.clone()],
            };
            let encoded = encode_shell_v1_descriptor_snapshot_frame(tx, &one)?;
            let mut b = prefix(s.connection_epoch, s.generation);
            push_u64(&mut b, g.slot);
            b.extend_from_slice(&encoded[SOPHIA_IPC_HEADER_LEN..]);
            frames.push(frame(IpcMessageKind::ShellTabsEntry, tx, b)?);
        }
    }
    frames.push(frame(
        IpcMessageKind::ShellTabsEnd,
        tx,
        prefix(s.connection_epoch, s.generation),
    )?);
    Ok(frames)
}

pub fn decode_shell_tab_snapshot(
    frames: &[Vec<u8>],
) -> Result<(TransactionId, ShellTabSnapshot), IpcCodecError> {
    if frames.len() < 2
        || frames.len() > 2 + SOPHIA_SHELL_MAX_TAB_GROUPS + SOPHIA_SHELL_MAX_TAB_ENTRIES
    {
        return Err(invalid());
    }
    let (h, b) = decode_frame(&frames[0])?;
    if h.message_kind != IpcMessageKind::ShellTabsBegin || !h.transaction.is_valid() {
        return Err(invalid());
    }
    let mut c = Cursor::new(b);
    let epoch = c.u64()?;
    let generation = c.u64()?;
    let groups = c.u16()? as usize;
    let entries = c.u16()? as usize;
    c.finish()?;
    if groups > SOPHIA_SHELL_MAX_TAB_GROUPS
        || entries > SOPHIA_SHELL_MAX_TAB_ENTRIES
        || frames.len() != groups + entries + 2
    {
        return Err(invalid());
    }
    let mut s = ShellTabSnapshot {
        connection_epoch: epoch,
        generation,
        groups: Vec::new(),
    };
    let mut expected = 0usize;
    for (index, f) in frames.iter().enumerate().skip(1) {
        let (head, p) = decode_frame(f)?;
        if head.transaction != h.transaction {
            return Err(invalid());
        }
        let mut c = Cursor::new(p);
        if c.u64()? != epoch || c.u64()? != generation {
            return Err(invalid());
        }
        match head.message_kind {
            IpcMessageKind::ShellTabsGroup if index < frames.len() - 1 && expected == 0 => {
                let slot = c.u64()?;
                let output = OutputId::from_raw(c.u64()?);
                let selected = c.u16()?;
                let focused = c.u16()?;
                expected = c.u16()? as usize;
                if c.u16()? != 0 || focused > 1 {
                    return Err(invalid());
                }
                c.finish()?;
                s.groups.push(ShellTabGroup {
                    slot,
                    output,
                    selected_slot: (selected != 0).then_some(selected),
                    focused: focused == 1,
                    entries: Vec::new(),
                });
            }
            IpcMessageKind::ShellTabsEntry if index < frames.len() - 1 && expected > 0 => {
                let slot = c.u64()?;
                let g = s.groups.last_mut().ok_or_else(invalid)?;
                if g.slot != slot {
                    return Err(invalid());
                }
                let wrapped = encode_frame(
                    IpcMessageKind::ShellV1DescriptorSnapshot,
                    h.transaction,
                    &p[24..],
                )?;
                let (_, one) = decode_shell_v1_descriptor_snapshot_frame(&wrapped)?;
                if one.connection_epoch != epoch
                    || one.snapshot_generation != generation
                    || one.output != g.output
                    || one.descriptors.len() != 1
                {
                    return Err(invalid());
                }
                g.entries.extend(one.descriptors);
                expected -= 1;
            }
            IpcMessageKind::ShellTabsEnd if index == frames.len() - 1 && expected == 0 => {
                c.finish()?;
            }
            _ => return Err(invalid()),
        }
    }
    if s.groups.len() != groups
        || s.groups.iter().map(|g| g.entries.len()).sum::<usize>() != entries
    {
        return Err(invalid());
    }
    validate_shell_tab_snapshot(&s)?;
    Ok((h.transaction, s))
}

pub fn validate_shell_tab_snapshot(s: &ShellTabSnapshot) -> Result<(), IpcCodecError> {
    sophia_shell_protocol::shell::tabs::validate_shell_tab_snapshot(s).map_err(Into::into)
}

pub fn encode_shell_tab_candidate(
    tx: TransactionId,
    c: &ShellTabCandidate,
) -> Result<Vec<u8>, IpcCodecError> {
    if c.connection_epoch == 0
        || c.snapshot_generation == 0
        || c.candidate_generation == 0
        || c.groups.len() > SOPHIA_SHELL_MAX_TAB_GROUPS
        || c.groups.contains(&0)
        || c.groups
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != c.groups.len()
    {
        return Err(invalid());
    }
    let mut b = prefix(c.connection_epoch, c.snapshot_generation);
    push_u64(&mut b, c.candidate_generation);
    push_u16(&mut b, c.groups.len() as u16);
    push_u16(&mut b, 0);
    for g in &c.groups {
        push_u64(&mut b, *g);
    }
    frame(IpcMessageKind::ShellTabsCandidate, tx, b)
}
pub fn decode_shell_tab_candidate(
    bytes: &[u8],
) -> Result<(TransactionId, ShellTabCandidate), IpcCodecError> {
    let (h, b) = decode_frame(bytes)?;
    if h.message_kind != IpcMessageKind::ShellTabsCandidate || !h.transaction.is_valid() {
        return Err(invalid());
    }
    let mut c = Cursor::new(b);
    let connection_epoch = c.u64()?;
    let snapshot_generation = c.u64()?;
    let candidate_generation = c.u64()?;
    let count = c.u16()? as usize;
    if count > SOPHIA_SHELL_MAX_TAB_GROUPS || c.u16()? != 0 {
        return Err(invalid());
    }
    let mut groups = Vec::with_capacity(count);
    for _ in 0..count {
        groups.push(c.u64()?);
    }
    c.finish()?;
    let candidate = ShellTabCandidate {
        connection_epoch,
        snapshot_generation,
        candidate_generation,
        groups,
    };
    encode_shell_tab_candidate(h.transaction, &candidate)?;
    Ok((h.transaction, candidate))
}
