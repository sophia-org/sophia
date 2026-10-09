fn configured_output_policy_keys(
    profile: &sophia_config::DesktopOutputCandidate,
) -> BTreeMap<String, u64> {
    profile
        .named
        .iter()
        .filter_map(|output| output.policy_key.map(|key| (output.connector.clone(), key)))
        .collect()
}

/// A fallback takes the explicitly configured affinity for this session. Remove
/// the absent connector's claim so its later return cannot duplicate that key.
fn startup_output_policy_keys(
    profile: &sophia_config::DesktopOutputCandidate,
    fallback_connector: Option<&str>,
) -> Result<BTreeMap<String, u64>, Box<dyn std::error::Error>> {
    let mut keys = configured_output_policy_keys(profile);
    if let Some(connector) = fallback_connector {
        if profile.availability != sophia_config::DesktopOutputAvailability::Adaptive {
            return Err("strict output profile cannot bind a fallback connector".into());
        }
        if let Some(key) = profile.fallback_policy_key {
            keys.retain(|_, saved_key| *saved_key != key);
            keys.insert(connector.to_owned(), key);
        }
    }
    Ok(keys)
}

/// Only the locally resolved logical output crosses to policy. Mirrors must
/// have a single configured key; no primary/enumeration fallback picks one.
fn resolve_output_policy_key(
    output: sophia_protocol::OutputId,
    keys: &BTreeMap<String, u64>,
    capabilities: &[sophia_backend_live::LibdrmNativeOutputCapability],
) -> Result<Option<u64>, Box<dyn std::error::Error>> {
    let mut found = None;
    for capability in capabilities.iter().filter(|c| c.output() == output) {
        if let Some(key) = keys
            .get(capability.connector_key())
            .or_else(|| keys.get(capability.connector_name()))
        {
            if found.is_some_and(|previous| previous != *key) {
                return Err("logical output has conflicting configured policy keys".into());
            }
            found = Some(*key);
        }
    }
    Ok(found)
}
