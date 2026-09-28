//! Native descriptor object values from Sophia's proposed descriptor file
//! amendment (ADR 4oapm903). A file envelope and its domain transaction are
//! outside these values. This module does not enable a served descriptor role.
use super::{ValueError, Wire, put_text_padded, reserved, rows, table_count, take_text_padded};
use crate::byte_cursor::Cursor;
use crate::*;

mod candidates;
mod events;
pub use candidates::*;
pub use events::*;

// Every entry validates before an infallible writer, and after a decoder
// has consumed exactly one complete value. Envelopes and custody stay outside.
macro_rules! value_codec {
    ($encode:ident, $decode:ident, $ty:ty, $validate:expr) => {
        pub fn $encode(value: &$ty) -> Result<Vec<u8>, ValueError> {
            ($validate)(value)?;
            let mut bytes = Vec::new();
            value.put(&mut bytes);
            Ok(bytes)
        }
        pub fn $decode(bytes: &[u8]) -> Result<$ty, ValueError> {
            let mut cursor = Cursor::new(bytes);
            let value = <$ty>::take(&mut cursor)?;
            cursor.finish()?;
            ($validate)(&value)?;
            Ok(value)
        }
    };
}
pub(super) use value_codec;

const DESCRIPTOR_BYTES: usize = 196;
const GROUP_BYTES: usize = 24;
const SHORTCUT_BYTES: usize = 408;

fn boolean(cursor: &mut Cursor<'_>, field: &'static str) -> Result<bool, ValueError> {
    match u16::take(cursor)? {
        0 => Ok(false),
        1 => Ok(true),
        value => Err(ValueError::InvalidEnum {
            field,
            value: u32::from(value),
        }),
    }
}

fn optional_text(
    cursor: &mut Cursor<'_>,
    present: bool,
    max: usize,
) -> Result<Option<String>, ValueError> {
    let text = take_text_padded(cursor, max)?;
    if present {
        Ok(Some(text))
    } else if text.is_empty() {
        Ok(None)
    } else {
        Err(ValueError::InvalidRecord("absent descriptor text"))
    }
}

fn require_length(bytes: &[u8], expected: usize) -> Result<(), ValueError> {
    if bytes.len() < expected {
        Err(ValueError::Truncated)
    } else if bytes.len() > expected {
        Err(ValueError::TrailingBytes(bytes.len() - expected))
    } else {
        Ok(())
    }
}

impl Wire for ToplevelActionCapabilityRef {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.token.put(bytes);
        self.issuer_epoch.put(bytes);
        self.issuer_revocation_epoch.put(bytes);
        self.recipient_epoch.put(bytes);
        self.target_slot.put(bytes);
        0u16.put(bytes);
        self.target_generation.put(bytes);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let token = u64::take(cursor)?;
        let issuer_epoch = u64::take(cursor)?;
        let issuer_revocation_epoch = u64::take(cursor)?;
        let recipient_epoch = u64::take(cursor)?;
        let target_slot = u16::take(cursor)?;
        reserved::<u16>(cursor)?;
        Ok(Self {
            token,
            issuer_epoch,
            issuer_revocation_epoch,
            recipient_epoch,
            target_slot,
            target_generation: u64::take(cursor)?,
        })
    }
}

impl Wire for ShellV1Descriptor {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.slot.put(bytes);
        let trust: u16 = match self.trust_level {
            TrustLevel::Unknown => 0,
            TrustLevel::Trusted => 1,
            TrustLevel::Untrusted => 2,
            TrustLevel::Isolated => 3,
        };
        let attention: u16 = match self.attention {
            AttentionState::None => 0,
            AttentionState::Notice => 1,
            AttentionState::Critical => 2,
        };
        trust.put(bytes);
        attention.put(bytes);
        u16::from(self.label.is_some()).put(bytes);
        u16::from(self.label.as_ref().is_some_and(|l| l.redacted)).put(bytes);
        0u16.put(bytes);
        self.generation.put(bytes);
        self.action.put(bytes);
        put_text_padded(
            bytes,
            self.label.as_ref().map(|l| l.text.as_str()).unwrap_or(""),
            MAX_CHROME_LABEL_LEN,
        );
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let slot = u16::take(cursor)?;
        let trust_level = match u16::take(cursor)? {
            0 => TrustLevel::Unknown,
            1 => TrustLevel::Trusted,
            2 => TrustLevel::Untrusted,
            3 => TrustLevel::Isolated,
            value => {
                return Err(ValueError::InvalidEnum {
                    field: "descriptor trust",
                    value: u32::from(value),
                });
            }
        };
        let attention = match u16::take(cursor)? {
            0 => AttentionState::None,
            1 => AttentionState::Notice,
            2 => AttentionState::Critical,
            value => {
                return Err(ValueError::InvalidEnum {
                    field: "descriptor attention",
                    value: u32::from(value),
                });
            }
        };
        let present = boolean(cursor, "descriptor label presence")?;
        let redacted = boolean(cursor, "descriptor label redaction")?;
        reserved::<u16>(cursor)?;
        if !present && redacted {
            return Err(ValueError::InvalidRecord(
                "descriptor redaction without label",
            ));
        }
        let generation = u64::take(cursor)?;
        let action = ToplevelActionCapabilityRef::take(cursor)?;
        let label = optional_text(cursor, present, MAX_CHROME_LABEL_LEN)?
            .map(|text| DisplayLabel { text, redacted });
        Ok(Self {
            slot,
            generation,
            label,
            trust_level,
            attention,
            action,
        })
    }
}

/// The value after the domain transaction in a whole Descriptors body.
pub fn encode_shell_descriptors_value(
    value: &ShellV1DescriptorSnapshot,
) -> Result<Vec<u8>, ValueError> {
    validate_shell_descriptor_snapshot(value)?;
    let mut bytes = Vec::with_capacity(56 + value.descriptors.len() * DESCRIPTOR_BYTES);
    value.connection_epoch.put(&mut bytes);
    value.snapshot_generation.put(&mut bytes);
    value.output.put(&mut bytes);
    value.output_generation.put(&mut bytes);
    value.broker_epoch.put(&mut bytes);
    value.broker_revocation_epoch.put(&mut bytes);
    (value.descriptors.len() as u16).put(&mut bytes);
    0u16.put(&mut bytes);
    0u32.put(&mut bytes);
    for descriptor in &value.descriptors {
        descriptor.put(&mut bytes);
    }
    Ok(bytes)
}

pub fn decode_shell_descriptors_value(
    bytes: &[u8],
) -> Result<ShellV1DescriptorSnapshot, ValueError> {
    let mut cursor = Cursor::new(bytes);
    let connection_epoch = u64::take(&mut cursor)?;
    let snapshot_generation = u64::take(&mut cursor)?;
    let output = OutputId::take(&mut cursor)?;
    let output_generation = u64::take(&mut cursor)?;
    let broker_epoch = u64::take(&mut cursor)?;
    let broker_revocation_epoch = u64::take(&mut cursor)?;
    let count = table_count(&mut cursor, SOPHIA_SHELL_MAX_DESCRIPTORS)?;
    reserved::<u16>(&mut cursor)?;
    reserved::<u32>(&mut cursor)?;
    require_length(bytes, 56 + count * DESCRIPTOR_BYTES)?;
    let descriptors = rows(&mut cursor, count)?;
    cursor.finish()?;
    let value = ShellV1DescriptorSnapshot {
        connection_epoch,
        snapshot_generation,
        output,
        output_generation,
        broker_epoch,
        broker_revocation_epoch,
        descriptors,
    };
    validate_shell_descriptor_snapshot(&value)?;
    Ok(value)
}

/// All group headers precede the flattened descriptor table. Group counts
/// partition that table; rows do not repeat group or snapshot identities.
pub fn encode_shell_tabs_value(value: &ShellTabSnapshot) -> Result<Vec<u8>, ValueError> {
    validate_shell_tab_snapshot(value)?;
    let count: usize = value.groups.iter().map(|g| g.entries.len()).sum();
    let mut bytes =
        Vec::with_capacity(24 + value.groups.len() * GROUP_BYTES + count * DESCRIPTOR_BYTES);
    value.connection_epoch.put(&mut bytes);
    value.generation.put(&mut bytes);
    (value.groups.len() as u16).put(&mut bytes);
    (count as u16).put(&mut bytes);
    0u32.put(&mut bytes);
    for group in &value.groups {
        group.slot.put(&mut bytes);
        group.output.put(&mut bytes);
        group.selected_slot.unwrap_or(0).put(&mut bytes);
        u16::from(group.focused).put(&mut bytes);
        (group.entries.len() as u16).put(&mut bytes);
        0u16.put(&mut bytes);
    }
    for group in &value.groups {
        for descriptor in &group.entries {
            descriptor.put(&mut bytes);
        }
    }
    Ok(bytes)
}

pub fn decode_shell_tabs_value(bytes: &[u8]) -> Result<ShellTabSnapshot, ValueError> {
    let mut cursor = Cursor::new(bytes);
    let connection_epoch = u64::take(&mut cursor)?;
    let generation = u64::take(&mut cursor)?;
    let group_count = table_count(&mut cursor, SOPHIA_SHELL_MAX_TAB_GROUPS)?;
    let entry_count = table_count(&mut cursor, SOPHIA_SHELL_MAX_TAB_ENTRIES)?;
    reserved::<u32>(&mut cursor)?;
    require_length(
        bytes,
        24 + group_count * GROUP_BYTES + entry_count * DESCRIPTOR_BYTES,
    )?;
    let mut groups = Vec::with_capacity(group_count);
    let mut counts = Vec::with_capacity(group_count);
    let mut remaining = entry_count;
    for _ in 0..group_count {
        let slot = u64::take(&mut cursor)?;
        let output = OutputId::take(&mut cursor)?;
        let selected = u16::take(&mut cursor)?;
        let focused = boolean(&mut cursor, "tab focused")?;
        let count = table_count(&mut cursor, remaining)?;
        reserved::<u16>(&mut cursor)?;
        remaining -= count;
        counts.push(count);
        groups.push(ShellTabGroup {
            slot,
            output,
            focused,
            selected_slot: if selected == 0 { None } else { Some(selected) },
            entries: Vec::new(),
        });
    }
    if remaining != 0 {
        return Err(ValueError::InvalidRecord("tab group count partition"));
    }
    for (group, count) in groups.iter_mut().zip(counts) {
        group.entries = rows(&mut cursor, count)?;
    }
    cursor.finish()?;
    let value = ShellTabSnapshot {
        connection_epoch,
        generation,
        groups,
    };
    validate_shell_tab_snapshot(&value)?;
    Ok(value)
}

impl Wire for ShellShortcut {
    fn put(&self, bytes: &mut Vec<u8>) {
        self.slot.put(bytes);
        u16::from(self.label.is_some()).put(bytes);
        u16::from(self.group.is_some()).put(bytes);
        0u16.put(bytes);
        put_text_padded(bytes, &self.chord, 64);
        put_text_padded(bytes, &self.action, 128);
        put_text_padded(bytes, self.label.as_deref().unwrap_or(""), 128);
        put_text_padded(bytes, self.group.as_deref().unwrap_or(""), 64);
    }
    fn take(cursor: &mut Cursor<'_>) -> Result<Self, ValueError> {
        let slot = u16::take(cursor)?;
        let label_present = boolean(cursor, "shortcut label presence")?;
        let group_present = boolean(cursor, "shortcut group presence")?;
        reserved::<u16>(cursor)?;
        let chord = take_text_padded(cursor, 64)?;
        let action = take_text_padded(cursor, 128)?;
        let label = optional_text(cursor, label_present, 128)?;
        let group = optional_text(cursor, group_present, 64)?;
        Ok(Self {
            slot,
            chord,
            action,
            label,
            group,
        })
    }
}

pub fn encode_shell_shortcuts_value(value: &ShellShortcutCatalog) -> Result<Vec<u8>, ValueError> {
    validate_shell_shortcut_catalog(value)?;
    let mut bytes = Vec::with_capacity(24 + value.entries.len() * SHORTCUT_BYTES);
    value.connection_epoch.put(&mut bytes);
    value.generation.put(&mut bytes);
    (value.entries.len() as u16).put(&mut bytes);
    0u16.put(&mut bytes);
    0u32.put(&mut bytes);
    for entry in &value.entries {
        entry.put(&mut bytes);
    }
    Ok(bytes)
}

pub fn decode_shell_shortcuts_value(bytes: &[u8]) -> Result<ShellShortcutCatalog, ValueError> {
    let mut cursor = Cursor::new(bytes);
    let connection_epoch = u64::take(&mut cursor)?;
    let generation = u64::take(&mut cursor)?;
    let count = table_count(&mut cursor, SOPHIA_SHELL_MAX_SHORTCUTS)?;
    reserved::<u16>(&mut cursor)?;
    reserved::<u32>(&mut cursor)?;
    require_length(bytes, 24 + count * SHORTCUT_BYTES)?;
    let entries = rows(&mut cursor, count)?;
    cursor.finish()?;
    let value = ShellShortcutCatalog {
        connection_epoch,
        generation,
        entries,
    };
    validate_shell_shortcut_catalog(&value)?;
    Ok(value)
}
