fn configured_output_policy_keys(
    profile: &sophia_config::DesktopOutputCandidate,
) -> BTreeMap<String, u64> {
    profile
        .named
        .iter()
        .filter_map(|output| output.policy_key.map(|key| (output.connector.clone(), key)))
        .collect()
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
        if let Some(key) = keys.get(capability.connector_name()) {
            if found.is_some_and(|previous| previous != *key) {
                return Err("logical output has conflicting configured policy keys".into());
            }
            found = Some(*key);
        }
    }
    Ok(found)
}
