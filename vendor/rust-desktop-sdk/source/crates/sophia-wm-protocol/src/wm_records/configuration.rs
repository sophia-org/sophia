//! Policy configuration rows: named action registrations and the actions whose
//! chords the WM follows. Chrome travels in the configuration metadata; each
//! transport chooses how to carry it.
use std::collections::BTreeSet;

use crate::byte_cursor::{Cursor, push_u32, push_u64};
use crate::wm_rows::{
    SNAPSHOT_ACTION_RECORD_KIND, WmV1SnapshotActionRecord, decode_wm_v1_snapshot_action_records,
    encode_wm_v1_snapshot_action_records,
};
use crate::{
    BinaryCodecError, POLICY_ACTION_LIFECYCLE_HELD_MS, PolicyActionLifecycleInterest,
    PolicyActionRegistration, PolicyConfiguration, WmActionId,
};

use super::values::{invalid, push_policy_section};
use super::{
    PolicyConfigurationMetadata, PolicyRecordContext, PolicyRecordSection, PolicyRecordSectionRef,
    validate_policy_record_sections,
};

/// `ConfigurationActionLifecycle`, gated on `action_lifecycle`: action u64,
/// held_ms u32, reserved u32.
pub const CONFIGURATION_ACTION_LIFECYCLE_RECORD_KIND: u16 = 0xff0e;
pub const CONFIGURATION_ACTION_LIFECYCLE_RECORD_LEN: usize = 16;
pub const CONFIGURATION_ACTION_LIFECYCLE_RECORD_MAX: usize = 256;

pub fn encode_policy_configuration_records(
    configuration: &PolicyConfiguration,
) -> Result<Vec<PolicyRecordSection>, BinaryCodecError> {
    if configuration.connection_epoch == 0 || configuration.generation == 0 {
        return Err(invalid("policy_configuration_identity", 0));
    }
    if configuration.actions.len() > crate::POLICY_MAX_BINDINGS {
        return Err(BinaryCodecError::CountTooLarge {
            count: configuration.actions.len(),
            max: crate::POLICY_MAX_BINDINGS,
        });
    }
    validate_policy_configuration(configuration)?;
    let records = configuration
        .actions
        .iter()
        .map(|action| {
            let (name_len, name) = encode_action_name(&action.name)?;
            Ok(WmV1SnapshotActionRecord {
                action: action.action.raw(),
                session_operation_slot: action.session_operation_slot.unwrap_or(0),
                name_len,
                name,
            })
        })
        .collect::<Result<Vec<_>, BinaryCodecError>>()?;
    let mut sections = Vec::new();
    push_policy_section(
        &mut sections,
        SNAPSHOT_ACTION_RECORD_KIND,
        records.len(),
        encode_wm_v1_snapshot_action_records(&records)?,
    )?;
    let mut rows = Vec::with_capacity(
        configuration.action_lifecycles.len() * CONFIGURATION_ACTION_LIFECYCLE_RECORD_LEN,
    );
    for interest in &configuration.action_lifecycles {
        push_u64(&mut rows, interest.action.raw());
        push_u32(&mut rows, interest.held_ms);
        push_u32(&mut rows, 0);
    }
    push_policy_section(
        &mut sections,
        CONFIGURATION_ACTION_LIFECYCLE_RECORD_KIND,
        configuration.action_lifecycles.len(),
        rows,
    )?;
    Ok(sections)
}

pub fn decode_policy_configuration_records(
    metadata: PolicyConfigurationMetadata,
    sections: &[PolicyRecordSectionRef<'_>],
) -> Result<PolicyConfiguration, BinaryCodecError> {
    if metadata.connection_epoch == 0 || metadata.generation == 0 {
        return Err(invalid("policy_configuration", 0));
    }
    validate_policy_record_sections(PolicyRecordContext::Configuration, sections)?;
    let mut records = Vec::new();
    let mut action_lifecycles = Vec::new();
    for section in sections {
        if section.kind == CONFIGURATION_ACTION_LIFECYCLE_RECORD_KIND {
            for row in section
                .bytes
                .chunks_exact(CONFIGURATION_ACTION_LIFECYCLE_RECORD_LEN)
            {
                let mut c = Cursor::new(row);
                let action = WmActionId::from_raw(c.u64()?);
                let held_ms = c.u32()?;
                if c.u32()? != 0 {
                    return Err(invalid("policy_configuration_action_lifecycle", 0));
                }
                action_lifecycles.push(PolicyActionLifecycleInterest { action, held_ms });
            }
            continue;
        }
        records.extend(decode_wm_v1_snapshot_action_records(
            section.bytes,
            section.count,
        )?);
    }
    let configuration = PolicyConfiguration {
        connection_epoch: metadata.connection_epoch,
        generation: metadata.generation,
        chrome: metadata.chrome,
        actions: decode_policy_action_rows(records)?,
        action_lifecycles,
    };
    validate_policy_configuration(&configuration)?;
    Ok(configuration)
}

pub(crate) fn decode_policy_action_rows(
    records: Vec<WmV1SnapshotActionRecord>,
) -> Result<Vec<PolicyActionRegistration>, BinaryCodecError> {
    records
        .into_iter()
        .map(|record| {
            Ok(PolicyActionRegistration {
                action: WmActionId::from_raw(record.action),
                name: decode_action_name(record.name_len, &record.name)?,
                session_operation_slot: (record.session_operation_slot != 0)
                    .then_some(record.session_operation_slot),
            })
        })
        .collect()
}

pub(crate) fn validate_policy_configuration(
    configuration: &PolicyConfiguration,
) -> Result<(), BinaryCodecError> {
    let valid_style = |enabled: bool, width: u32| {
        width <= 64 && ((enabled && width > 0) || (!enabled && width == 0))
    };
    if !valid_style(
        configuration.chrome.focus_ring.enabled,
        configuration.chrome.focus_ring.width,
    ) || !valid_style(
        configuration.chrome.frame.enabled,
        configuration.chrome.frame.width,
    ) {
        return Err(invalid("policy_configuration_chrome", 0));
    }

    let mut action_ids = BTreeSet::new();
    let mut action_names = BTreeSet::new();
    for action in &configuration.actions {
        if !action.action.is_valid()
            || encode_action_name(&action.name).is_err()
            || !action_ids.insert(action.action)
            || !action_names.insert(action.name.as_str())
        {
            return Err(invalid("policy_configuration_action", 0));
        }
    }

    // Each lifecycle row names a registered action that is not a session
    // operation, once, with Held off or within the contract's range.
    if configuration.action_lifecycles.len() > CONFIGURATION_ACTION_LIFECYCLE_RECORD_MAX {
        return Err(BinaryCodecError::CountTooLarge {
            count: configuration.action_lifecycles.len(),
            max: CONFIGURATION_ACTION_LIFECYCLE_RECORD_MAX,
        });
    }
    let mut lifecycle_ids = BTreeSet::new();
    for interest in &configuration.action_lifecycles {
        let policy_action = configuration.actions.iter().any(|action| {
            action.action == interest.action && action.session_operation_slot.is_none()
        });
        if !policy_action
            || (interest.held_ms != 0
                && !POLICY_ACTION_LIFECYCLE_HELD_MS.contains(&interest.held_ms))
            || !lifecycle_ids.insert(interest.action)
        {
            return Err(invalid("policy_configuration_action_lifecycle", 0));
        }
    }
    Ok(())
}

pub(super) fn encode_action_name(name: &str) -> Result<(u16, [u8; 128]), BinaryCodecError> {
    if name.is_empty()
        || name.len() > crate::POLICY_ACTION_NAME_MAX_BYTES
        || name.trim() != name
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b' ' | b'.'))
    {
        return Err(invalid("policy_action_name", 0));
    }
    let mut encoded = [0; 128];
    encoded[..name.len()].copy_from_slice(name.as_bytes());
    Ok((name.len() as u16, encoded))
}

fn decode_action_name(length: u16, encoded: &[u8; 128]) -> Result<String, BinaryCodecError> {
    let length = usize::from(length);
    if length == 0
        || length > crate::POLICY_ACTION_NAME_MAX_BYTES
        || encoded[length..].iter().any(|byte| *byte != 0)
    {
        return Err(invalid("policy_action_name", length as u32));
    }
    let name = core::str::from_utf8(&encoded[..length])
        .map_err(|_| invalid("policy_action_name", length as u32))?;
    encode_action_name(name)?;
    Ok(name.to_owned())
}
