#![cfg(test)]
//! This row checks the gate against evidence emitted by a real preceding
//! Session case. It makes no new claim about Session's service behavior.
use super::{identity, m4, types::*, worker};
use std::collections::BTreeMap;
use std::path::Path;

#[test]
#[ignore = "requires the current contained M4 run's actual Session evidence"]
fn evidence_integrity() {
    let config_path = Path::new("/work/config.json");
    let config: Config = identity::read_json(config_path).unwrap();
    assert_eq!(config.gate, Gate::M4);
    assert!(!config.self_test);
    let mut report: Report =
        identity::read_json(Path::new("/work/evidence/inner-report.json")).unwrap();
    assert!(report.source_attested_inside);
    assert_eq!(
        identity::contents(Path::new("/work/source")).unwrap(),
        config.source.content_sha256
    );
    worker::validate_identity(&report, &config, config_path).unwrap();
    m4::validate_binary(&report, Path::new("/work")).unwrap();
    let inventory = m4::inventory(
        Gate::M4,
        Path::new("/work/source/tools/probes/m4_acceptance/inventory.json"),
    )
    .unwrap();
    let construction = &report.cases[0];
    assert_eq!(
        construction.status,
        Verdict::Pass,
        "a real positive is required before tampering"
    );
    let exact = construction.test.as_deref().unwrap();
    let run = construction.execution.as_ref().unwrap();
    let text = std::fs::read_to_string("/work/evidence/M4.construction.log").unwrap();
    let original = m4::validate_case(Gate::M4, &inventory.cases[0], exact, run, &text).unwrap();
    let without = text
        .lines()
        .filter(|line| !line.starts_with("sophia_m4_acceptance "))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(m4::validate_case(Gate::M4, &inventory.cases[0], exact, run, &without).is_err());
    let marker = text
        .lines()
        .find(|line| line.starts_with("sophia_m4_acceptance "))
        .unwrap();
    assert!(
        m4::validate_case(
            Gate::M4,
            &inventory.cases[0],
            exact,
            run,
            &format!("{text}\n{marker}\n")
        )
        .is_err()
    );
    let skipped = text
        .replace("construction ... ok", "construction ... ignored")
        .replace(
            "1 passed; 0 failed; 0 ignored",
            "0 passed; 0 failed; 1 ignored",
        );
    assert!(m4::validate_case(Gate::M4, &inventory.cases[0], exact, run, &skipped).is_err());
    let mut timed_out = run.clone();
    timed_out.timed_out = true;
    assert!(m4::validate_case(Gate::M4, &inventory.cases[0], exact, &timed_out, &text).is_err());

    // The complete source identity is checked, not merely the content digest.
    let source = report.source.clone();
    report.source.commit.push_str("-wrong");
    assert!(worker::validate_identity(&report, &config, config_path).is_err());
    report.source = source.clone();
    report.source.tree.push_str("-wrong");
    assert!(worker::validate_identity(&report, &config, config_path).is_err());
    report.source = source;
    worker::validate_identity(&report, &config, config_path).unwrap();
    let binaries = report.binary.clone();
    for key in [
        None,
        Some("private_host"),
        Some("activation_probe"),
        Some("evidence-integrity-tests"),
    ] {
        report.binary = binaries.clone();
        let entry = report.binary.as_mut().unwrap();
        let entry = if let Some(key) = key {
            &mut entry[key]
        } else {
            entry
        };
        entry["sha256"] = serde_json::json!("0".repeat(64));
        assert!(m4::validate_binary(&report, Path::new("/work")).is_err());
    }
    report.binary = binaries;
    m4::validate_binary(&report, Path::new("/work")).unwrap();

    let evidence = CaseEvidence {
        schema: 1,
        case: "M4.evidence_integrity".into(),
        subcases: inventory.cases[7]
            .subcases
            .iter()
            .map(|name| (name.clone(), Verdict::Pass))
            .collect(),
        cleanup: original.cleanup,
        observations: BTreeMap::from([
            (
                "scope".into(),
                serde_json::json!("gate_validation_of_real_session_evidence"),
            ),
            ("source_case".into(), serde_json::json!("M4.construction")),
            (
                "source_commit".into(),
                serde_json::json!(config.source.commit),
            ),
            (
                "collection_from_preceding_case".into(),
                serde_json::json!(true),
            ),
        ]),
    };
    println!(
        "sophia_m4_acceptance {}",
        serde_json::to_string(&evidence).unwrap()
    );
}
