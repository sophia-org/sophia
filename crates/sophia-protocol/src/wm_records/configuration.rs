//! Policy configuration rows: named action registrations. Chrome travels in
//! the configuration metadata; each transport chooses how to carry it.
use std::collections::BTreeSet;

use crate::{BinaryCodecError, PolicyActionRegistration, PolicyConfiguration, WmActionId};
// Raw generated rows: root-exported names until `crate::wm_rows` owns them.
use crate::{
    SNAPSHOT_ACTION_RECORD_KIND, WmV1SnapshotActionRecord, decode_wm_v1_snapshot_action_records,
    encode_wm_v1_snapshot_action_records,
};

use super::values::{invalid, push_policy_section};
use super::{
    PolicyConfigurationMetadata, PolicyRecordContext, PolicyRecordSection, PolicyRecordSectionRef,
    validate_policy_record_sections,
};

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
    for section in sections {
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
