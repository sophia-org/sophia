use super::cursor::{Cursor, push_u16, push_u32, push_u64};
use super::{IpcCodecError, IpcMessageKind, decode_frame, encode_frame};
use crate::shell::encoding::ValueError;
use crate::*;

fn invalid(field: &'static str) -> IpcCodecError {
    IpcCodecError::InvalidEnum { field, value: 0 }
}

fn frame(kind: IpcMessageKind, tx: TransactionId, data: Vec<u8>) -> Result<Vec<u8>, IpcCodecError> {
    if !tx.is_valid() {
        return Err(invalid("shell_indicator_transaction"));
    }
    encode_frame(kind, tx, &data)
}

fn prefix(epoch: u64, generation: u64) -> Vec<u8> {
    let mut b = Vec::new();
    push_u64(&mut b, epoch);
    push_u64(&mut b, generation);
    b
}

/// Labels ride a fixed 32-byte field with an explicit length, matching the
/// projection record this republishes. A shorter label is zero-padded so every
/// record is the same size on the wire and a decoder never trusts a length to
/// find the next one.
fn push_fixed_label(out: &mut Vec<u8>, text: &str) -> Result<(), IpcCodecError> {
    let bytes = text.as_bytes();
    if bytes.len() > SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES {
        return Err(IpcCodecError::CountTooLarge {
            count: bytes.len(),
            max: SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES,
        });
    }
    push_u16(out, bytes.len() as u16);
    let mut padded = [0u8; SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES];
    padded[..bytes.len()].copy_from_slice(bytes);
    out.extend_from_slice(&padded);
    Ok(())
}

fn take_fixed_label(cursor: &mut Cursor<'_>, len: usize) -> Result<String, IpcCodecError> {
    if len > SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES {
        return Err(IpcCodecError::CountTooLarge {
            count: len,
            max: SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES,
        });
    }
    let bytes = cursor.slice(SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES)?;
    // Padding must be zero. A decoder that ignored it would let two different
    // byte strings mean the same label.
    if bytes[len..].iter().any(|b| *b != 0) {
        return Err(IpcCodecError::ReservedNonZero(1));
    }
    std::str::from_utf8(&bytes[..len])
        .map(str::to_owned)
        .map_err(|_| invalid("shell_indicator_label"))
}

pub fn validate_shell_indicator_snapshot(
    snapshot: &ShellIndicatorSnapshot,
) -> Result<(), IpcCodecError> {
    // Kept as the exact original count checks and error (`CountTooLarge`),
    // not a delegation to `shell::indicators::validate` (which the file
    // wire's own whole-object codec uses, and which reports a different,
    // file-contract-specific error): this function's public error is part
    // of the IPC codec's observable behaviour and must stay byte/value
    // identical to what it returned before the neutral split.
    if snapshot.indicators.len() > SOPHIA_SHELL_MAX_INDICATORS {
        return Err(IpcCodecError::CountTooLarge {
            count: snapshot.indicators.len(),
            max: SOPHIA_SHELL_MAX_INDICATORS,
        });
    }
    if snapshot.statuses.len() > SOPHIA_SHELL_MAX_OUTPUT_STATUS {
        return Err(IpcCodecError::CountTooLarge {
            count: snapshot.statuses.len(),
            max: SOPHIA_SHELL_MAX_OUTPUT_STATUS,
        });
    }
    Ok(())
}

pub fn encode_shell_indicator_snapshot(
    tx: TransactionId,
    snapshot: &ShellIndicatorSnapshot,
) -> Result<Vec<Vec<u8>>, IpcCodecError> {
    validate_shell_indicator_snapshot(snapshot)?;
    let mut b = prefix(snapshot.connection_epoch, snapshot.generation);
    // An absent active output is an explicit flag with a zeroed identity, not a
    // sentinel id. Nothing downstream has to agree on which number means none.
    push_u64(&mut b, snapshot.active_output.map_or(0, OutputId::raw));
    push_u16(&mut b, u16::from(snapshot.active_output.is_some()));
    push_u16(&mut b, snapshot.indicators.len() as u16);
    push_u16(&mut b, snapshot.statuses.len() as u16);
    push_u16(&mut b, 0);
    let mut frames = vec![frame(IpcMessageKind::ShellIndicatorsBegin, tx, b)?];

    for status in &snapshot.statuses {
        let mut b = prefix(snapshot.connection_epoch, snapshot.generation);
        push_u64(&mut b, status.output.raw());
        push_u16(&mut b, status.focus_bits);
        let layout = status.layout.as_bytes();
        if layout.len() > SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES {
            return Err(IpcCodecError::CountTooLarge {
                count: layout.len(),
                max: SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES,
            });
        }
        push_u16(&mut b, layout.len() as u16);
        push_u32(&mut b, 0);
        let mut padded = [0u8; SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES];
        padded[..layout.len()].copy_from_slice(layout);
        b.extend_from_slice(&padded);
        frames.push(frame(IpcMessageKind::ShellIndicatorsOutputStatus, tx, b)?);
    }

    for indicator in &snapshot.indicators {
        let mut b = prefix(snapshot.connection_epoch, snapshot.generation);
        push_u64(&mut b, indicator.output.raw());
        push_u64(&mut b, indicator.indicator);
        push_u64(&mut b, indicator.action);
        push_u32(&mut b, indicator.slot);
        push_u16(&mut b, indicator.state_bits);
        push_fixed_label(&mut b, &indicator.label)?;
        frames.push(frame(IpcMessageKind::ShellIndicatorsEntry, tx, b)?);
    }

    frames.push(frame(
        IpcMessageKind::ShellIndicatorsEnd,
        tx,
        prefix(snapshot.connection_epoch, snapshot.generation),
    )?);
    Ok(frames)
}

pub fn decode_shell_indicator_snapshot(
    frames: &[Vec<u8>],
) -> Result<(TransactionId, ShellIndicatorSnapshot), IpcCodecError> {
    let Some((first, rest)) = frames.split_first() else {
        return Err(IpcCodecError::Truncated);
    };
    let (header, payload) = decode_frame(first)?;
    let tx = header.transaction;
    if header.message_kind != IpcMessageKind::ShellIndicatorsBegin || !tx.is_valid() {
        return Err(invalid("shell_indicators_begin"));
    }
    let mut cursor = Cursor::new(payload);
    let connection_epoch = cursor.u64()?;
    let generation = cursor.u64()?;
    let active_raw = cursor.u64()?;
    let active_present = cursor.u16()?;
    let indicator_count = usize::from(cursor.u16()?);
    let status_count = usize::from(cursor.u16()?);
    let reserved = cursor.u16()?;
    if reserved != 0 {
        return Err(IpcCodecError::ReservedNonZero(u32::from(reserved)));
    }
    cursor.finish()?;
    let active_output = match active_present {
        0 => {
            // An absent output must carry a zeroed identity, so a stale id
            // cannot ride along unnoticed behind a false flag.
            if active_raw != 0 {
                return Err(IpcCodecError::ReservedNonZero(1));
            }
            None
        }
        1 => Some(OutputId::from_raw(active_raw)),
        _ => return Err(invalid("shell_indicator_active_output_present")),
    };
    if indicator_count > SOPHIA_SHELL_MAX_INDICATORS {
        return Err(IpcCodecError::CountTooLarge {
            count: indicator_count,
            max: SOPHIA_SHELL_MAX_INDICATORS,
        });
    }
    if status_count > SOPHIA_SHELL_MAX_OUTPUT_STATUS {
        return Err(IpcCodecError::CountTooLarge {
            count: status_count,
            max: SOPHIA_SHELL_MAX_OUTPUT_STATUS,
        });
    }
    if rest.len() != status_count + indicator_count + 1 {
        return Err(IpcCodecError::Truncated);
    }

    let mut statuses = Vec::with_capacity(status_count);
    for raw in &rest[..status_count] {
        let (header, payload) = decode_frame(raw)?;
        if header.message_kind != IpcMessageKind::ShellIndicatorsOutputStatus
            || header.transaction != tx
        {
            return Err(invalid("shell_indicators_output_status"));
        }
        let mut cursor = Cursor::new(payload);
        if cursor.u64()? != connection_epoch || cursor.u64()? != generation {
            return Err(invalid("shell_indicator_status_identity"));
        }
        let output = OutputId::from_raw(cursor.u64()?);
        let focus_bits = cursor.u16()?;
        let layout_len = usize::from(cursor.u16()?);
        let reserved = cursor.u32()?;
        if reserved != 0 {
            return Err(IpcCodecError::ReservedNonZero(reserved));
        }
        let layout = take_fixed_label(&mut cursor, layout_len)?;
        cursor.finish()?;
        statuses.push(ShellOutputStatus {
            output,
            focus_bits,
            layout,
        });
    }

    let mut indicators = Vec::with_capacity(indicator_count);
    for raw in &rest[status_count..status_count + indicator_count] {
        let (header, payload) = decode_frame(raw)?;
        if header.message_kind != IpcMessageKind::ShellIndicatorsEntry || header.transaction != tx {
            return Err(invalid("shell_indicators_entry"));
        }
        let mut cursor = Cursor::new(payload);
        if cursor.u64()? != connection_epoch || cursor.u64()? != generation {
            return Err(invalid("shell_indicator_entry_identity"));
        }
        let output = OutputId::from_raw(cursor.u64()?);
        let indicator = cursor.u64()?;
        let action = cursor.u64()?;
        let slot = cursor.u32()?;
        let state_bits = cursor.u16()?;
        let label_len = usize::from(cursor.u16()?);
        let label = take_fixed_label(&mut cursor, label_len)?;
        cursor.finish()?;
        indicators.push(ShellIndicator {
            output,
            indicator,
            action,
            slot,
            state_bits,
            label,
        });
    }

    let (header, payload) = decode_frame(&rest[status_count + indicator_count])?;
    if header.message_kind != IpcMessageKind::ShellIndicatorsEnd || header.transaction != tx {
        return Err(invalid("shell_indicators_end"));
    }
    let mut cursor = Cursor::new(payload);
    if cursor.u64()? != connection_epoch || cursor.u64()? != generation {
        return Err(invalid("shell_indicator_end_identity"));
    }
    cursor.finish()?;

    Ok((
        tx,
        ShellIndicatorSnapshot {
            connection_epoch,
            generation,
            active_output,
            statuses,
            indicators,
        },
    ))
}

pub fn encode_shell_indicator_activation(
    tx: TransactionId,
    activation: &ShellIndicatorActivation,
) -> Result<Vec<u8>, IpcCodecError> {
    let b =
        crate::shell::encoding::indicators::encode_shell_indicator_activation_value(activation)?;
    frame(IpcMessageKind::ShellIndicatorActivate, tx, b)
}

pub fn decode_shell_indicator_activation(
    bytes: &[u8],
) -> Result<(TransactionId, ShellIndicatorActivation), IpcCodecError> {
    let (header, payload) = decode_frame(bytes)?;
    let tx = header.transaction;
    if header.message_kind != IpcMessageKind::ShellIndicatorActivate || !tx.is_valid() {
        return Err(invalid("shell_indicator_activate"));
    }
    let activation =
        crate::shell::encoding::indicators::decode_shell_indicator_activation_value(payload)?;
    Ok((tx, activation))
}

pub fn encode_shell_indicator_activation_outcome(
    tx: TransactionId,
    outcome: &ShellIndicatorActivationOutcome,
) -> Result<Vec<u8>, IpcCodecError> {
    let b = crate::shell::encoding::indicators::encode_shell_indicator_activation_outcome_value(
        outcome,
    )?;
    frame(IpcMessageKind::ShellIndicatorActivateOutcome, tx, b)
}

pub fn decode_shell_indicator_activation_outcome(
    bytes: &[u8],
) -> Result<(TransactionId, ShellIndicatorActivationOutcome), IpcCodecError> {
    let (header, payload) = decode_frame(bytes)?;
    let tx = header.transaction;
    if header.message_kind != IpcMessageKind::ShellIndicatorActivateOutcome || !tx.is_valid() {
        return Err(invalid("shell_indicator_activate_outcome"));
    }
    let outcome =
        crate::shell::encoding::indicators::decode_shell_indicator_activation_outcome_value(
            payload,
        )
        .map_err(|err| match err {
            // The neutral decoder names this field with its own spelling
            // (spaces, for the file contract's own diagnostics); the IPC
            // codec's public error keeps the original identifier.
            ValueError::InvalidEnum { value, .. } => IpcCodecError::InvalidEnum {
                field: "shell_indicator_activation_status",
                value,
            },
            other => IpcCodecError::from(other),
        })?;
    Ok((tx, outcome))
}
