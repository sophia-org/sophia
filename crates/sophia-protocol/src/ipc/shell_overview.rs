use std::collections::BTreeSet;

use super::cursor::{Cursor, push_u16, push_u32, push_u64};
use super::{IpcCodecError, IpcMessageKind, decode_frame, encode_frame};
use crate::*;

fn invalid() -> IpcCodecError {
    IpcCodecError::InvalidRecord("shell_overview")
}

pub fn validate_shell_overview_catalog(
    catalog: &ShellOverviewCatalog,
) -> Result<(), IpcCodecError> {
    if catalog.connection_epoch == 0
        || catalog.generation == 0
        || catalog.workspaces.is_empty()
        || catalog.workspaces.len() > SOPHIA_SHELL_MAX_OVERVIEW_WORKSPACES
    {
        return Err(invalid());
    }
    let mut slots = BTreeSet::new();
    let mut outputs = BTreeSet::new();
    let mut active = BTreeSet::new();
    let mut count = 0usize;
    for workspace in &catalog.workspaces {
        if workspace.slot == 0
            || !slots.insert(workspace.slot)
            || !workspace.output.is_valid()
            || workspace.windows.len() > SOPHIA_SHELL_MAX_OVERVIEW_WINDOWS_PER_WORKSPACE
            || workspace.active && !active.insert(workspace.output)
        {
            return Err(invalid());
        }
        outputs.insert(workspace.output);
        let windows: BTreeSet<_> = workspace.windows.iter().copied().collect();
        if windows.len() != workspace.windows.len()
            || windows.contains(&0)
            || workspace.focused != 0 && !windows.contains(&workspace.focused)
        {
            return Err(invalid());
        }
        count += windows.len();
        if count > SOPHIA_SHELL_MAX_OVERVIEW_WINDOWS {
            return Err(invalid());
        }
    }
    if outputs != active {
        return Err(invalid());
    }
    Ok(())
}

fn framed(
    kind: IpcMessageKind,
    transaction: TransactionId,
    bytes: &[u8],
) -> Result<Vec<u8>, IpcCodecError> {
    if !transaction.is_valid() {
        return Err(invalid());
    }
    encode_frame(kind, transaction, bytes)
}

pub fn encode_shell_overview_catalog(
    transaction: TransactionId,
    catalog: &ShellOverviewCatalog,
) -> Result<Vec<Vec<u8>>, IpcCodecError> {
    validate_shell_overview_catalog(catalog)?;
    let mut prefix = Vec::new();
    push_u64(&mut prefix, catalog.connection_epoch);
    push_u64(&mut prefix, catalog.generation);
    let mut boundary = prefix.clone();
    push_u16(&mut boundary, catalog.workspaces.len() as u16);
    push_u16(&mut boundary, 0);
    push_u32(
        &mut boundary,
        catalog
            .workspaces
            .iter()
            .map(|w| w.windows.len() as u32)
            .sum(),
    );
    let mut frames = vec![framed(
        IpcMessageKind::ShellOverviewBegin,
        transaction,
        &boundary,
    )?];
    for workspace in &catalog.workspaces {
        let mut bytes = prefix.clone();
        push_u16(&mut bytes, workspace.slot);
        push_u16(&mut bytes, u16::from(workspace.active));
        push_u64(&mut bytes, workspace.output.raw());
        push_u16(&mut bytes, workspace.focused);
        push_u16(&mut bytes, workspace.windows.len() as u16);
        for window in &workspace.windows {
            push_u16(&mut bytes, *window);
        }
        frames.push(framed(
            IpcMessageKind::ShellOverviewWorkspace,
            transaction,
            &bytes,
        )?);
    }
    frames.push(framed(
        IpcMessageKind::ShellOverviewEnd,
        transaction,
        &boundary,
    )?);
    Ok(frames)
}

pub fn encode_shell_overview_request(
    transaction: TransactionId,
    request: ShellOverviewRequest,
) -> Result<Vec<u8>, IpcCodecError> {
    if request.connection_epoch == 0
        || request.catalog_generation == 0
        || request.request_generation == 0
        || !request.output.is_valid()
        || request.operation != ShellOverviewOperation::Pick
            && (request.workspace != 0 || request.window != 0)
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    for value in [
        request.connection_epoch,
        request.catalog_generation,
        request.request_generation,
        request.presentation_epoch,
        request.output.raw(),
    ] {
        push_u64(&mut bytes, value);
    }
    push_u16(&mut bytes, request.operation as u16);
    push_u16(&mut bytes, request.workspace);
    push_u16(&mut bytes, request.window);
    push_u16(&mut bytes, 0);
    framed(IpcMessageKind::ShellOverviewRequest, transaction, &bytes)
}

pub fn decode_shell_overview_candidate(
    frame: &[u8],
) -> Result<(TransactionId, ShellOverviewCandidate), IpcCodecError> {
    let (header, bytes) = decode_frame(frame)?;
    if header.message_kind != IpcMessageKind::ShellOverviewCandidate
        || !header.transaction.is_valid()
        || bytes.len() != 40
    {
        return Err(invalid());
    }
    let mut cursor = Cursor::new(bytes);
    let connection_epoch = cursor.u64()?;
    let catalog_generation = cursor.u64()?;
    let request_generation = cursor.u64()?;
    let candidate_generation = cursor.u64()?;
    let flags = cursor.u16()?;
    let workspace = cursor.u16()?;
    let window = cursor.u16()?;
    if cursor.u16()? != 0
        || flags > 2
        || connection_epoch == 0
        || catalog_generation == 0
        || request_generation == 0
        || candidate_generation == 0
        || workspace == 0
    {
        return Err(invalid());
    }
    Ok((
        header.transaction,
        ShellOverviewCandidate {
            connection_epoch,
            catalog_generation,
            request_generation,
            candidate_generation,
            visible: flags == 1,
            activate: flags == 2,
            workspace,
            window,
        },
    ))
}

pub fn encode_shell_overview_outcome(
    transaction: TransactionId,
    outcome: ShellV1CandidateOutcome,
) -> Result<Vec<u8>, IpcCodecError> {
    if outcome.connection_epoch == 0
        || outcome.candidate_generation == 0
        || outcome.kind == ShellV1CandidateOutcomeKind::Presented && outcome.presentation_epoch == 0
    {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    push_u64(&mut bytes, outcome.connection_epoch);
    push_u64(&mut bytes, outcome.candidate_generation);
    push_u64(&mut bytes, outcome.presentation_epoch);
    push_u16(&mut bytes, outcome.kind as u16);
    push_u16(&mut bytes, 0);
    framed(IpcMessageKind::ShellOverviewOutcome, transaction, &bytes)
}
