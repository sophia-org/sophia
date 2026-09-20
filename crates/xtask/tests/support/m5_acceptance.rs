#![cfg(test)]
//! Parser and identity evidence for the M5 gate, kept separate from Session
//! acceptance: none of this claims any XTEST behaviour.
use super::{catalog, identity, m4, m5, types::*};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const FROZEN: &str = include_str!("../../../../tools/probes/m4_acceptance/inventory.json");
const FROZEN_M5: &str = include_str!("../../../../tools/probes/m5_acceptance/inventory.json");

fn inventory() -> Inventory {
    serde_json::from_str(FROZEN_M5).unwrap()
}

fn temporary(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("m5-{name}-{}.json", std::process::id()))
}

fn execution() -> Execution {
    Execution {
        command: "synthetic M5 evidence parser fixture".into(),
        returncode: Some(0),
        timed_out: false,
        elapsed_millis: 1,
        collection: Collection {
            root_waited: true,
            descendants_found: 0,
            descendants_reaped: 0,
            remaining: vec![],
            error: None,
        },
    }
}

fn record() -> CaseEvidence {
    CaseEvidence {
        schema: 1,
        case: "M5.version_negotiation".into(),
        subcases: inventory().cases[1]
            .subcases
            .iter()
            .map(|name| (name.clone(), Verdict::Pass))
            .collect(),
        cleanup: ActorCollection {
            actors_started: 1,
            actors_collected: 1,
            pending_actors: 0,
            complete: true,
        },
        observations: BTreeMap::from([("parser_fixture".into(), serde_json::json!(true))]),
    }
}

fn output(record: &CaseEvidence) -> String {
    format!(
        "\nrunning 1 test\ntest version_negotiation ... ok\nsophia_m5_acceptance {}\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s\n",
        serde_json::to_string(record).unwrap()
    )
}

fn validate(run: &Execution, text: &str) -> Result<CaseEvidence, String> {
    m5::validate_case(&inventory().cases[1], "version_negotiation", run, text)
}

#[test]
fn m5_inventory_is_distinct_and_cannot_promote_m3_or_m4() {
    let value = inventory();
    assert!(catalog::validate(&value).is_err());
    let path = temporary("inventory");
    std::fs::write(&path, FROZEN_M5).unwrap();
    assert!(m5::inventory(&path).is_ok());
    assert!(m4::inventory(Gate::M4, &path).is_err());
    assert!(m4::inventory(Gate::M5, &path).is_ok());
    std::fs::write(&path, FROZEN).unwrap();
    assert!(m5::inventory(&path).is_err());
    std::fs::remove_file(&path).unwrap();
    let mut rows = catalog::initial(&value);
    assert_eq!(m5::overall(&rows).unwrap(), Verdict::NotRun);
    for row in &mut rows {
        row.status = Verdict::Pass;
    }
    assert_eq!(m5::overall(&rows).unwrap(), Verdict::Pass);
    rows[3].status = Verdict::Fail;
    assert_eq!(m5::overall(&rows).unwrap(), Verdict::Fail);
    rows.pop();
    assert!(m5::overall(&rows).is_err());
    rows.push(rows[0].clone());
    assert!(m5::overall(&rows).is_err());
}

#[test]
fn m5_inventory_pins_its_plan_revision_and_every_contract() {
    let frozen = inventory();
    assert_eq!(
        frozen.plan_source_commit,
        "ed0726c10258dad0b1fdde210f567fed0b510e62"
    );
    let path = temporary("contract");
    for change in [
        |value: &mut serde_json::Value| value["plan_source_commit"] = "0".repeat(40).into(),
        |value: &mut serde_json::Value| value["cases"][0]["required"] = false.into(),
        |value: &mut serde_json::Value| value["cases"][0]["gate"] = "M4".into(),
        |value: &mut serde_json::Value| value["cases"][2]["requirement"] = "weakened".into(),
        |value: &mut serde_json::Value| {
            value["cases"][5]["subcases"].as_array_mut().unwrap().pop();
        },
        |value: &mut serde_json::Value| {
            let cases = value["cases"].as_array_mut().unwrap();
            cases.swap(0, 1);
        },
    ] {
        let mut value: serde_json::Value = serde_json::from_str(FROZEN_M5).unwrap();
        change(&mut value);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(m5::inventory(&path).is_err());
    }
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn m5_bindings_name_exactly_the_dedicated_group_tests() {
    let path = temporary("bindings");
    let exact: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tools/probes/m5_acceptance/bindings.json"
    ))
    .unwrap();
    std::fs::write(&path, serde_json::to_vec(&exact).unwrap()).unwrap();
    let bindings = m5::bindings(&path).unwrap();
    assert_eq!(bindings.cases.len(), 8);
    assert_eq!(
        bindings
            .cases
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        m5::CASES.into_iter().collect::<BTreeSet<_>>()
    );
    for change in [
        |value: &mut serde_json::Value| {
            value["cases"]["M5.grab_control"] = "lifetime".into();
        },
        |value: &mut serde_json::Value| {
            value["cases"]["M4.lifetime"] = "lifetime".into();
        },
        |value: &mut serde_json::Value| value["schema"] = 2.into(),
    ] {
        let mut value = exact.clone();
        change(&mut value);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(m5::bindings(&path).is_err());
    }
    // A partial map is admitted: its unbound rows stay NOT_RUN.
    let mut partial = exact.clone();
    partial["cases"]
        .as_object_mut()
        .unwrap()
        .remove("M5.grab_control");
    std::fs::write(&path, serde_json::to_vec(&partial).unwrap()).unwrap();
    assert_eq!(m5::bindings(&path).unwrap().cases.len(), 7);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn m5_missing_duplicate_and_foreign_records_are_rejected() {
    let evidence = record();
    let positive = output(&evidence);
    validate(&execution(), &positive).unwrap();
    assert!(validate(&execution(), "").is_err());
    assert!(validate(&execution(), &format!("{positive}{positive}")).is_err());
    let mut foreign = evidence.clone();
    foreign.case = "M4.authorization".into();
    assert!(validate(&execution(), &output(&foreign)).is_err());
    // An M4 record cannot stand in for an M5 one, whatever it says.
    let borrowed = positive.replace("sophia_m5_acceptance", "sophia_m4_acceptance");
    assert!(validate(&execution(), &borrowed).is_err());
    let mut component_only = evidence;
    component_only
        .observations
        .insert("component_only".into(), serde_json::json!(true));
    assert!(validate(&execution(), &output(&component_only)).is_err());
}

#[test]
fn m5_each_subcase_and_each_actor_is_required() {
    let mut evidence = record();
    evidence.subcases.pop_first();
    assert!(validate(&execution(), &output(&evidence)).is_err());
    evidence = record();
    evidence
        .subcases
        .insert("requested_2".into(), Verdict::NotRun);
    assert!(validate(&execution(), &output(&evidence)).is_err());
    evidence = record();
    evidence
        .subcases
        .insert("requested_3".into(), Verdict::Pass);
    assert!(validate(&execution(), &output(&evidence)).is_err());
    evidence = record();
    evidence.cleanup.actors_collected = 0;
    assert!(validate(&execution(), &output(&evidence)).is_err());
    evidence = record();
    evidence.cleanup.pending_actors = 1;
    assert!(validate(&execution(), &output(&evidence)).is_err());
    evidence = record();
    evidence.observations.clear();
    assert!(validate(&execution(), &output(&evidence)).is_err());
}

#[test]
fn m5_zero_tests_timeouts_and_leaked_processes_cannot_pass() {
    let positive = output(&record());
    for changed in [
        positive.replace("running 1 test", "running 0 tests"),
        positive.replace(
            "version_negotiation ... ok",
            "version_negotiation ... ignored",
        ),
        positive.replace(
            "1 passed; 0 failed; 0 ignored",
            "0 passed; 0 failed; 1 ignored",
        ),
    ] {
        assert!(validate(&execution(), &changed).is_err());
    }
    let mut run = execution();
    run.timed_out = true;
    assert!(validate(&run, &positive).is_err());
    run = execution();
    run.returncode = Some(1);
    assert!(validate(&run, &positive).is_err());
    run = execution();
    run.collection.descendants_found = 1;
    run.collection.descendants_reaped = 1;
    assert!(validate(&run, &positive).is_err());
}

#[test]
fn m5_changed_test_binary_is_rejected() {
    let directory = std::env::temp_dir().join(format!("m5-binary-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("evidence")).unwrap();
    let test = directory.join("evidence/test-binary");
    std::fs::write(&test, b"original test executable").unwrap();
    let mut report: Report = serde_json::from_value(serde_json::json!({
        "schema":1,"run_id":"fixture","purpose":"m5_acceptance",
        "overall":"NOT_RUN","cases":[],
        "source":{"commit":"fixture","tree":"fixture","clean":true,
            "archive_sha256":"fixture","content_sha256":"fixture"},
        "build_target_namespace":"fixture","config_sha256":"fixture",
        "source_attested_inside":true,"source_unchanged_after":true,
        "binary":{"path":"evidence/test-binary","sha256":identity::digest(&test).unwrap()}
    }))
    .unwrap();
    m5::validate_binary(&report, &directory).unwrap();
    std::fs::write(&test, b"different executable").unwrap();
    assert!(
        m5::validate_binary(&report, &directory)
            .unwrap_err()
            .contains("binary identity")
    );
    std::fs::write(&test, b"original test executable").unwrap();
    report.binary.as_mut().unwrap()["path"] = serde_json::json!("../other/test-binary");
    assert!(m5::validate_binary(&report, &directory).is_err());
    report.binary = None;
    assert!(m5::validate_binary(&report, &directory).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn m5_gate_names_its_command_and_directory() {
    assert_eq!(Gate::M5.command(), "m5-acceptance");
    assert_eq!(Gate::M5.directory(), "tools/probes/m5_acceptance");
    let config: Config = serde_json::from_value(serde_json::json!({
        "schema":1,"gate":"m5","run_id":"fixture","self_test":false,
        "component_tests":[],"build_timeout":1,"case_timeout":1,
        "source":{"commit":"fixture","tree":"fixture","clean":true,
        "archive_sha256":"fixture","content_sha256":"a".repeat(64)},
        "build_target_namespace":format!("source-{}", "a".repeat(64)),
        "host_namespaces":{},"inventory_sha256":"fixture","bindings_sha256":"fixture",
        "xtask_sha256":"fixture","toolchain_sha256":{}
    }))
    .unwrap();
    assert_eq!(config.gate, Gate::M5);
    let path = temporary("config");
    identity::json(&path, &config).unwrap();
    let report = super::worker::initial(&config, &inventory(), &path).unwrap();
    assert_eq!(report.purpose, "m5_acceptance");
    assert_eq!(report.cases.len(), 8);
    assert!(report.cases.iter().all(|row| row.status == Verdict::NotRun));
    std::fs::remove_file(path).unwrap();
}
