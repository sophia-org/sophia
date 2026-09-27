//! Value encoding for the revision-6 shell indicator vocabulary
//! (`crate::shell::indicators`).
//!
//! [`ShellIndicatorActivation`] and [`ShellIndicatorActivationOutcome`] are
//! flat records with no transfer shape, so their `Wire` impls here are a
//! pure move of the byte layout `sophia_protocol::ipc::shell_indicators`
//! used to build inline: the IPC codec now calls these instead of pushing
//! bytes itself, and its frames are unchanged.
//!
//! [`ShellIndicatorSnapshot`] itself has no neutral value encoding to move:
//! the IPC codec only ever carried it as a four-frame Begin/OutputStatus/
//! Entry/End transfer, which stays exactly as it was (it is deleted at t255,
//! not reused). The whole-object encoding here
//! ([`encode_shell_indicator_snapshot`]/[`decode_shell_indicator_snapshot`])
//! is new: one object body, no chunk ordinals or repeated identities, per
//! docs/sophia-shell-files.md Amendment 1.
use super::{Wire, put_text_padded, reserved, rows, table_count, take_text_padded};
use crate::byte_cursor::Cursor;
use crate::shell::encoding::ValueError;
use crate::*;

impl Wire for ShellIndicatorActivation {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.connection_epoch.put(bytes);
        self.snapshot_generation.put(bytes);
        self.output.put(bytes);
        self.indicator.put(bytes);
        self.action.put(bytes);
        self.event_id.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        Ok(Self {
            connection_epoch: u64::take(cursor)?,
            snapshot_generation: u64::take(cursor)?,
            output: OutputId::take(cursor)?,
            indicator: u64::take(cursor)?,
            action: u64::take(cursor)?,
            event_id: u64::take(cursor)?,
        })
    }
}

impl TryFrom<u16> for ShellIndicatorActivationStatus {
    type Error = ValueError;
    fn try_from(raw: u16) -> Result<Self, Self::Error> {
        use ShellIndicatorActivationStatus::*;
        Ok(match raw {
            0 => Accepted,
            1 => Stale,
            2 => Unknown,
            3 => Unauthorized,
            other => {
                return Err(ValueError::InvalidEnum {
                    field: "shell indicator activation status",
                    value: u32::from(other),
                });
            }
        })
    }
}

impl Wire for ShellIndicatorActivationOutcome {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.connection_epoch.put(bytes);
        self.snapshot_generation.put(bytes);
        self.event_id.put(bytes);
        (self.status as u16).put(bytes);
        self.reason.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let connection_epoch = u64::take(cursor)?;
        let snapshot_generation = u64::take(cursor)?;
        let event_id = u64::take(cursor)?;
        let status = ShellIndicatorActivationStatus::try_from(u16::take(cursor)?)?;
        let reason = u16::take(cursor)?;
        Ok(Self {
            connection_epoch,
            snapshot_generation,
            event_id,
            status,
            reason,
        })
    }
}

/// Encodes one [`ShellIndicatorActivation`] value body. Validation is
/// structural only: there is no semantic validator for this record today
/// (`sophia_protocol::ipc::shell_indicators` had none either), so this
/// function does not invent one.
pub fn encode_shell_indicator_activation_value(
    value: &ShellIndicatorActivation,
) -> Result<Vec<u8>, ValueError> {
    let mut bytes = Vec::new();
    value.put(&mut bytes);
    Ok(bytes)
}

pub fn decode_shell_indicator_activation_value(
    payload: &[u8],
) -> Result<ShellIndicatorActivation, ValueError> {
    let mut cursor = Cursor::new(payload);
    let value = ShellIndicatorActivation::take(&mut cursor)?;
    cursor.finish()?;
    Ok(value)
}

pub fn encode_shell_indicator_activation_outcome_value(
    value: &ShellIndicatorActivationOutcome,
) -> Result<Vec<u8>, ValueError> {
    let mut bytes = Vec::new();
    value.put(&mut bytes);
    Ok(bytes)
}

pub fn decode_shell_indicator_activation_outcome_value(
    payload: &[u8],
) -> Result<ShellIndicatorActivationOutcome, ValueError> {
    let mut cursor = Cursor::new(payload);
    let value = ShellIndicatorActivationOutcome::take(&mut cursor)?;
    cursor.finish()?;
    Ok(value)
}

// ---------------------------------------------------------------------
// The whole `Indicators` object: new, native, no transfer shape.
// ---------------------------------------------------------------------

impl Wire for ShellOutputStatus {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.output.put(bytes);
        self.focus_bits.put(bytes);
        put_text_padded(bytes, &self.layout, SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let output = OutputId::take(cursor)?;
        let focus_bits = u16::take(cursor)?;
        let layout = take_text_padded(cursor, SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES)?;
        Ok(Self {
            output,
            focus_bits,
            layout,
        })
    }
}

impl Wire for ShellIndicator {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.output.put(bytes);
        self.indicator.put(bytes);
        self.action.put(bytes);
        self.slot.put(bytes);
        self.state_bits.put(bytes);
        put_text_padded(bytes, &self.label, SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let output = OutputId::take(cursor)?;
        let indicator = u64::take(cursor)?;
        let action = u64::take(cursor)?;
        let slot = u32::take(cursor)?;
        let state_bits = u16::take(cursor)?;
        let label = take_text_padded(cursor, SOPHIA_SHELL_MAX_INDICATOR_LABEL_BYTES)?;
        Ok(Self {
            output,
            indicator,
            action,
            slot,
            state_bits,
            label,
        })
    }
}

impl Wire for ShellIndicatorSnapshot {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.connection_epoch.put(bytes);
        self.generation.put(bytes);
        self.active_output.map_or(0u64, OutputId::raw).put(bytes);
        u16::from(self.active_output.is_some()).put(bytes);
        (self.statuses.len() as u16).put(bytes);
        (self.indicators.len() as u16).put(bytes);
        0u16.put(bytes);
        for row in &self.statuses {
            row.put(bytes);
        }
        for row in &self.indicators {
            row.put(bytes);
        }
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let connection_epoch = u64::take(cursor)?;
        let generation = u64::take(cursor)?;
        let active_raw = u64::take(cursor)?;
        let active_present = u16::take(cursor)?;
        let active_output = match active_present {
            0 => {
                if active_raw != 0 {
                    return Err(ValueError::ReservedNonZero(1));
                }
                None
            }
            1 => Some(OutputId::from_raw(active_raw)),
            other => {
                return Err(ValueError::InvalidEnum {
                    field: "shell indicator active output present",
                    value: u32::from(other),
                });
            }
        };
        let status_count = table_count(cursor, SOPHIA_SHELL_MAX_OUTPUT_STATUS)?;
        let indicator_count = table_count(cursor, SOPHIA_SHELL_MAX_INDICATORS)?;
        reserved::<u16>(cursor)?;
        Ok(Self {
            connection_epoch,
            generation,
            active_output,
            statuses: rows(cursor, status_count)?,
            indicators: rows(cursor, indicator_count)?,
        })
    }
}

/// Encodes the whole indicator snapshot object body (no transaction prefix;
/// the file wire adds that). Reuses
/// [`crate::shell::indicators::validate`] for the count bounds the IPC
/// transfer decoder used to check.
pub fn encode_shell_indicator_snapshot(
    snapshot: &ShellIndicatorSnapshot,
) -> Result<Vec<u8>, ValueError> {
    crate::shell::indicators::validate(snapshot)?;
    let mut bytes = Vec::new();
    snapshot.put(&mut bytes);
    Ok(bytes)
}

pub fn decode_shell_indicator_snapshot(
    payload: &[u8],
) -> Result<ShellIndicatorSnapshot, ValueError> {
    let mut cursor = Cursor::new(payload);
    let snapshot = ShellIndicatorSnapshot::take(&mut cursor)?;
    cursor.finish()?;
    crate::shell::indicators::validate(&snapshot)?;
    Ok(snapshot)
}
