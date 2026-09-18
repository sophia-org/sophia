//! Three independent native shell components. This checks transcript evidence,
//! not visual placement, input feel, driver behavior or a latency workload.
use std::collections::{BTreeMap, BTreeSet};

/// Explicit probe overrides; WM policy and shortcut bindings are never replaced.
pub fn profile(paths: &[String]) -> Result<String, String> {
    if paths.len() != 5 {
        return Err("dock profile needs LOM LOM_CONFIG BEMENU PROVLITA DOCK_CONFIG".into());
    }
    let quoted = paths
        .iter()
        .map(|p| {
            if !std::path::Path::new(p).is_absolute() || p.chars().any(char::is_control) {
                return Err(
                    "component paths must be absolute without control characters".to_owned(),
                );
            }
            Ok(format!(
                "\"{}\"",
                p.replace('\\', "\\\\").replace('"', "\\\"")
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let [lom, config, menu, dock, dock_config] = quoted.as_slice() else {
        unreachable!()
    };
    Ok(format!(
        r#"schema 1
shell {{ enabled #true; content #true; content-input #true; panel 24; gpu "denied"; }}
session {{
    shell-component "panel" "bar" {{ executable {lom}; config {config}; gpu "direct"; reservation "top" 24; }}
    shell-component "menu" "application-launcher" {{ executable {menu}; gpu "denied"; }}
    shell-component "dock" "dock" {{ executable {dock}; config {dock_config}; gpu "direct"; reservation "bottom" 64; }}
    application-catalog "native-launcher-gate"
    startup
}}
input {{ inherit-sophia #true; }}
output {{ inherit-sophia #true; }}
broker {{ enabled #false; }}
"#
    ))
}

type Grant = (u64, u64);
type Fields<'a> = BTreeMap<&'a str, &'a str>;
fn number(fields: &Fields<'_>, key: &str, min: u64, max: u64) -> Result<u64, String> {
    let text = fields.get(key).ok_or_else(|| format!("missing {key}"))?;
    if text.is_empty() || text.len() > 20 || !text.bytes().all(|c| c.is_ascii_digit()) {
        return Err(format!("invalid {key}"));
    }
    let value = text.parse::<u64>().map_err(|_| format!("overflow {key}"))?;
    if !(min..=max).contains(&value) {
        return Err(format!("out of range {key}"));
    }
    Ok(value)
}
fn grant(f: &Fields<'_>) -> Result<Grant, String> {
    Ok((
        number(f, "connection_epoch", 1, u64::MAX)?,
        number(f, "content_grant_epoch", 1, u64::MAX)?,
    ))
}

/// Host-only smoke evidence. Exact origin records are emitted when the actual
/// child enters Session custody; a client ACK is not a process-start witness.
pub fn verify(text: &str) -> Result<String, String> {
    if text.len() > 64 * 1024 * 1024 {
        return Err("dock log exceeds bound".into());
    }
    let mut roles = BTreeMap::<String, (Grant, u64)>::new();
    let mut facts = BTreeSet::new();
    let mut presented = BTreeMap::<Grant, BTreeMap<u64, BTreeSet<u64>>>::new();
    let mut launched = BTreeMap::<Grant, BTreeSet<u64>>::new();
    let mut transactions = BTreeMap::new();
    let mut exited = BTreeSet::new();
    let mut protocol_tally = false;
    let mut retired = BTreeSet::new();
    let (mut committed, mut catalog, mut shutdown) = (0, 0, 0);
    for (index, raw) in text.lines().enumerate() {
        if index >= 100_000 || raw.len() > 64 * 1024 {
            return Err("record bound exceeded".into());
        }
        let line = if raw.contains('\t') {
            let parts = raw.split('\t').collect::<Vec<_>>();
            if parts.len() != 4 {
                return Err("invalid structured record".into());
            }
            parts[3]
        } else {
            raw
        };
        // Native logs may carry tracing prefixes as well as recorder columns.
        let Some(record) = crate::direct_scanout::record_after_marker(line, "sophia_") else {
            continue;
        };
        let record = format!("sophia_{record}");
        let mut words = record.split_whitespace();
        let Some(name) = words.next() else { continue };
        let mut f = Fields::new();
        for word in words {
            if let Some((key, value)) = word.split_once('=')
                && f.insert(key, value).is_some()
            {
                return Err("duplicate evidence field".into());
            }
        }
        let status = f.get("status").copied().unwrap_or("");
        if name.contains("runtime_fatal")
            || f.contains_key("failure_code")
            || matches!(
                status,
                "transport_failed" | "presentation_failed" | "deadline_exceeded" | "retained"
            )
        {
            return Err("runtime failure or retained ownership".into());
        }
        if shutdown != 0
            && matches!(
                name,
                "sophia_shell_component"
                    | "sophia_shell_component_catalog"
                    | "sophia_shell_components_shutdown"
                    | "sophia_live_shell_content"
                    | "sophia_catalog_launch"
            )
        {
            return Err("component work after shutdown".into());
        }
        match name {
            "sophia_live_session_protocol_error_tally" => {
                number(&f, "total", 0, 0)?;
                if protocol_tally {
                    return Err("duplicate protocol tally".into());
                }
                protocol_tally = true;
            }
            "sophia_catalog_launch" if status == "process_exited" => {
                number(&f, "schema", 1, 1)?;
                let transaction = number(&f, "transaction", 1, u64::MAX)?;
                if f.get("success") != Some(&"true")
                    || transactions.get(&transaction) != Some(&grant(&f)?)
                    || !exited.insert(transaction)
                {
                    return Err("failed/unknown/duplicate catalog child exit".into());
                }
            }
            "sophia_shell_component" => {
                number(&f, "schema", 1, 1)?;
                let g = grant(&f)?;
                let slot = number(&f, "slot", 0, 2)?;
                if status == "process_retired" {
                    if f.get("endpoint_released") != Some(&"true")
                        || !roles.values().any(|v| *v == (g, slot))
                        || !retired.insert(g)
                    {
                        return Err("unknown/duplicate/unreleased retirement".into());
                    }
                    continue;
                }
                if status != "negotiated" {
                    return Err("component failure".into());
                }
                let role = *f.get("role").ok_or("missing role")?;
                let revision = match role {
                    "bar" => 6,
                    "application_launcher" => 7,
                    "dock" => 8,
                    _ => return Err("unknown role".into()),
                };
                number(&f, "revision", revision, revision)?;
                if roles.contains_key(role)
                    || roles
                        .values()
                        .any(|(old, s)| old.0 == g.0 || old.1 == g.1 || *s == slot)
                {
                    return Err("restarted or aliased component".into());
                }
                let gpu = number(&f, "gpu_grant_epoch", 0, u64::MAX)?;
                let major = number(&f, "device_major", 0, u32::MAX.into())?;
                let minor = number(&f, "device_minor", 0, u32::MAX.into())?;
                if role == "application_launcher" {
                    if f.get("gpu_mode") != Some(&"denied") || (gpu, major, minor) != (0, 0, 0) {
                        return Err("launcher GPU authority".into());
                    }
                } else if f.get("gpu_mode") != Some(&"direct") || gpu != g.0 || major == 0 {
                    return Err("persistent component missing exact GPU grant".into());
                }
                roles.insert(role.into(), (g, slot));
            }
            "sophia_live_wm_configuration" => {
                number(&f, "schema", 2, 2)?;
                if status != "committed" {
                    return Err("WM rejected configuration".into());
                }
                committed += 1;
            }
            "sophia_shell_component_catalog" => {
                number(&f, "schema", 1, 1)?;
                number(&f, "generation", 1, 1)?;
                number(&f, "entries", 1, 4096)?;
                if status != "built" {
                    return Err("catalog failure".into());
                }
                catalog += 1;
            }
            "sophia_live_shell_content" => {
                number(&f, "schema", 1, 1)?;
                if !matches!(status, "outputs" | "prepared" | "presented") {
                    return Err("content failure".into());
                }
                if status == "prepared" {
                    continue;
                }
                let g = grant(&f)?;
                if !roles.values().any(|(known, _)| *known == g) {
                    return Err("unknown content grant".into());
                }
                if status == "outputs" {
                    number(&f, "facts_generation", 1, u64::MAX)?;
                    number(&f, "outputs", 2, 2)?;
                    facts.insert(g);
                } else {
                    let output = number(&f, "output", 1, u64::MAX)?;
                    let candidate = number(&f, "candidate_generation", 1, u64::MAX)?;
                    number(&f, "presentation_epoch", 1, u64::MAX)?;
                    if !presented
                        .entry(g)
                        .or_default()
                        .entry(output)
                        .or_default()
                        .insert(candidate)
                    {
                        return Err("duplicate presentation".into());
                    }
                }
            }
            "sophia_catalog_launch" => {
                number(&f, "schema", 1, 1)?;
                if status != "process_started" {
                    return Err("launch failed".into());
                }
                let g = grant(&f)?;
                let role = if f.get("cause") == Some(&"persistent") {
                    "dock"
                } else if f.get("cause") == Some(&"transient") {
                    "application_launcher"
                } else {
                    return Err("unknown launch cause".into());
                };
                if roles.get(role).map(|v| v.0) != Some(g) {
                    return Err("launch grant mismatch".into());
                }
                let output = number(&f, "output", 1, u64::MAX)?;
                number(&f, "event_id", 1, u64::MAX)?;
                if !presented
                    .get(&g)
                    .is_some_and(|outputs| outputs.contains_key(&output))
                {
                    return Err("launch before presentation".into());
                }
                if transactions
                    .insert(number(&f, "transaction", 1, u64::MAX)?, g)
                    .is_some()
                {
                    return Err("duplicate launch".into());
                }
                launched.entry(g).or_default().insert(output);
            }
            "sophia_native_launcher" if status != "process_started" => {
                return Err("launcher failed/expired".into());
            }
            "sophia_shell_components_shutdown" => {
                number(&f, "schema", 1, 1)?;
                if status != "quiescent" || roles.len() != 3 {
                    return Err("unresolved/early shutdown".into());
                }
                shutdown += 1;
            }
            _ => {}
        }
    }
    if roles.len() != 3 || committed != 1 || catalog != 1 || shutdown != 1 {
        return Err("missing/repeated lifecycle evidence".into());
    }
    if !protocol_tally || exited.len() != transactions.len() {
        return Err("missing clean child exits or protocol tally".into());
    }
    let mut coverage = None;
    for (role, (g, _)) in &roles {
        let outputs = presented.get(g).ok_or("missing presentations")?;
        let keys = outputs.keys().copied().collect::<BTreeSet<_>>();
        if !facts.contains(g)
            || keys.len() != 2
            || coverage.as_ref().is_some_and(|old| old != &keys)
        {
            return Err("three components need matching two-output coverage".into());
        }
        if role != "dock" && outputs.values().any(|v| v.len() < 2) {
            return Err("bar/menu needs repeated presentation".into());
        }
        if role != "bar" && launched.get(g) != Some(&keys) {
            return Err("each output needs an actual launch from menu and dock".into());
        }
        coverage = Some(keys);
    }
    Ok("sophia_dock_smoke schema=1 status=pass components=3 outputs=2 scope=host_transcript visual_acceptance=OPERATOR_REQUIRED latency_acceptance=NOT_RUN restart_acceptance=NOT_RUN".into())
}
