pub fn encode_policy_configuration_records(
    configuration: &PolicyConfiguration,
) -> Result<Vec<super::PolicyRecordSection>, IpcCodecError> {
    if configuration.connection_epoch == 0 || configuration.generation == 0 {
        return Err(invalid("policy_configuration_identity", 0));
    }
    if configuration.actions.len() > crate::POLICY_MAX_BINDINGS {
        return Err(IpcCodecError::CountTooLarge {
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
        .collect::<Result<Vec<_>, IpcCodecError>>()?;
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
    metadata: super::PolicyConfigurationMetadata,
    sections: &[super::PolicyRecordSectionRef<'_>],
) -> Result<PolicyConfiguration, IpcCodecError> {
    if metadata.connection_epoch == 0 || metadata.generation == 0 {
        return Err(invalid("policy_configuration", 0));
    }
    super::validate_policy_record_sections(super::PolicyRecordContext::Configuration, sections)?;
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

fn decode_policy_action_rows(
    records: Vec<WmV1SnapshotActionRecord>,
) -> Result<Vec<PolicyActionRegistration>, IpcCodecError> {
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

fn validate_policy_configuration(configuration: &PolicyConfiguration) -> Result<(), IpcCodecError> {
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

    let mut action_ids = std::collections::BTreeSet::new();
    let mut action_names = std::collections::BTreeSet::new();
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
