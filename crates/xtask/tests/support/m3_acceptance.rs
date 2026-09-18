#![cfg(test)]
//! Private gate invariants; synthetic evidence never becomes an acceptance row.
use super::{catalog, evidence, host, identity, process, types::*};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static PROCESS_TEST: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn inventory() -> Inventory {
    serde_json::from_str(include_str!(
        "../../../../tools/probes/m3_acceptance/inventory.json"
    ))
    .unwrap()
}

fn execution() -> Execution {
    Execution {
        command: "synthetic parser fixture".into(),
        returncode: Some(0),
        timed_out: false,
        elapsed_millis: 1,
        collection: Collection {
            root_waited: true,
            descendants_found: 0,
            descendants_reaped: 0,
            remaining: Vec::new(),
            error: None,
        },
    }
}

fn evidence(row: &Case) -> CaseEvidence {
    CaseEvidence {
        schema: 1,
        case: row.case.clone(),
        subcases: row
            .subcases
            .iter()
            .map(|name| (name.clone(), Verdict::Pass))
            .collect(),
        cleanup: ActorCollection {
            actors_started: 2,
            actors_collected: 2,
            pending_actors: 0,
            complete: true,
        },
        observations: [("fixture_only".into(), serde_json::json!(true))]
            .into_iter()
            .collect(),
    }
}

fn output(exact: &str, record: &CaseEvidence) -> String {
    format!(
        "\nrunning 1 test\ntest {exact} ... ok\nsophia_m3_acceptance {}\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 4 filtered out; finished in 0.1s\n",
        serde_json::to_string(record).unwrap()
    )
}

#[test]
fn exact_twenty_cases_and_gate_counts_are_fixed() {
    let value = inventory();
    catalog::validate(&value).unwrap();
    assert_eq!(value.cases.len(), 20);
    for (gate, expected) in [("A", 3), ("B", 4), ("C", 6), ("D", 7)] {
        assert_eq!(
            value.cases.iter().filter(|row| row.gate == gate).count(),
            expected
        );
    }
    assert_eq!(
        value
            .cases
            .iter()
            .find(|row| row.case == "C.control_cleanup")
            .unwrap()
            .subcases
            .len(),
        9
    );
}

#[test]
fn missing_duplicate_and_empty_inventory_cannot_pass() {
    let mut value = inventory();
    value.cases.pop();
    assert!(catalog::validate(&value).is_err());
    let mut value = inventory();
    value.cases[1] = value.cases[0].clone();
    assert!(catalog::validate(&value).is_err());
    let mut value = inventory();
    value.cases.clear();
    assert!(catalog::validate(&value).is_err());
    assert!(catalog::overall(&[]).is_err());
}

#[test]
fn absent_implementation_keeps_all_twenty_not_run() {
    let value = inventory();
    let results = catalog::initial(&value);
    assert_eq!(catalog::overall(&results).unwrap(), Verdict::NotRun);
    assert!(results.iter().all(|row| row.status == Verdict::NotRun));
}

#[test]
fn one_missing_or_failed_case_prevents_aggregate_pass() {
    let mut results = catalog::initial(&inventory());
    for row in &mut results {
        row.status = Verdict::Pass;
    }
    results[13].status = Verdict::NotRun;
    assert_eq!(catalog::overall(&results).unwrap(), Verdict::NotRun);
    results[13].status = Verdict::Fail;
    assert_eq!(catalog::overall(&results).unwrap(), Verdict::Fail);
}

#[test]
fn complete_synthetic_record_validates_only_the_parser() {
    let row = &inventory().cases[0];
    let exact = format!("{}fixture", catalog::PREFIX);
    evidence::validate_case(row, &exact, &execution(), &output(&exact, &evidence(row))).unwrap();
}

#[test]
fn zero_filtered_ignored_or_wrong_test_is_rejected() {
    let row = &inventory().cases[0];
    let exact = format!("{}fixture", catalog::PREFIX);
    let valid = output(&exact, &evidence(row));
    for bad in [
        String::new(),
        valid.replace("running 1 test", "running 0 tests"),
        valid.replace(
            "1 passed; 0 failed; 0 ignored",
            "0 passed; 0 failed; 1 ignored",
        ),
        valid.replace(&format!("test {exact}"), "test another::test"),
    ] {
        assert!(evidence::validate_case(row, &exact, &execution(), &bad).is_err());
    }
}

#[test]
fn absent_extra_wrong_case_and_missing_subcase_evidence_is_rejected() {
    let row = &inventory().cases[0];
    let exact = format!("{}fixture", catalog::PREFIX);
    let valid = output(&exact, &evidence(row));
    assert!(
        evidence::validate_case(
            row,
            &exact,
            &execution(),
            &valid.replace("sophia_m3_acceptance", "other")
        )
        .is_err()
    );
    assert!(evidence::validate_case(row, &exact, &execution(), &(valid.clone() + &valid)).is_err());
    let mut record = evidence(row);
    record.case = "A.partial_proof".into();
    assert!(evidence::validate_case(row, &exact, &execution(), &output(&exact, &record)).is_err());
    record = evidence(row);
    record.subcases.pop_first();
    assert!(evidence::validate_case(row, &exact, &execution(), &output(&exact, &record)).is_err());
}

#[test]
fn timeout_and_process_collection_errors_override_test_success() {
    let row = &inventory().cases[0];
    let exact = format!("{}fixture", catalog::PREFIX);
    let text = output(&exact, &evidence(row));
    let mut run = execution();
    run.timed_out = true;
    assert!(evidence::validate_case(row, &exact, &run, &text).is_err());
    run = execution();
    run.collection.descendants_found = 1;
    assert!(evidence::validate_case(row, &exact, &run, &text).is_err());
    run = execution();
    run.collection.error = Some("cleanup failed".into());
    assert!(evidence::validate_case(row, &exact, &run, &text).is_err());
    run = execution();
    run.collection.root_waited = false;
    assert!(evidence::validate_case(row, &exact, &run, &text).is_err());
}

#[test]
fn test_owned_actor_collection_is_independent_of_process_exit() {
    let row = &inventory().cases[0];
    let exact = format!("{}fixture", catalog::PREFIX);
    let mut record = evidence(row);
    record.cleanup.actors_collected = 1;
    assert!(evidence::validate_case(row, &exact, &execution(), &output(&exact, &record)).is_err());
    record = evidence(row);
    record.cleanup.complete = false;
    assert!(evidence::validate_case(row, &exact, &execution(), &output(&exact, &record)).is_err());
}

#[test]
fn namespace_launcher_children_must_all_be_collected() {
    let mut run = execution();
    run.collection.descendants_found = 1;
    assert!(!evidence::launcher_collected(&run));
    run.collection.descendants_reaped = 1;
    assert!(evidence::launcher_collected(&run));
    assert!(!run.clean(), "a case still cannot leak descendants");
    run.collection.error = Some("collection failed".into());
    assert!(!evidence::launcher_collected(&run));
}

#[test]
fn case_filters_and_external_binary_options_are_refused() {
    for option in [
        "--case=A.press_release_repress",
        "--filter=foo",
        "--binary=/tmp/test",
        "--skip=poison",
    ] {
        assert!(host::options(&[option.into()]).is_err());
    }
}

struct Temporary(PathBuf);
impl Temporary {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "m3-harness-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn legacy_component_test_cannot_be_bound_as_integrated_case() {
    let temporary = Temporary::new();
    let path = temporary.0.join("bindings.json");
    std::fs::write(&path, r#"{"schema":1,"cases":{"A.press_release_repress":"x11_socket::routing_tests::pointer_pair"}}"#).unwrap();
    assert!(catalog::bindings(&path).is_err());
}

#[test]
fn source_content_hash_detects_changed_bytes_and_modes() {
    use std::os::unix::fs::PermissionsExt;
    let temporary = Temporary::new();
    let path = temporary.0.join("source");
    std::fs::write(&path, "before").unwrap();
    let before = identity::contents(&temporary.0).unwrap();
    std::fs::write(&path, "after").unwrap();
    assert_ne!(before, identity::contents(&temporary.0).unwrap());
    let after = identity::contents(&temporary.0).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_ne!(after, identity::contents(&temporary.0).unwrap());
}

#[test]
fn actual_timeout_kills_and_waits_for_its_child() {
    let _serial = PROCESS_TEST.lock().unwrap();
    process::arm_subreaper().unwrap();
    let temporary = Temporary::new();
    let run = process::run(
        process::private_command("/usr/bin/sleep").arg("5"),
        &temporary.0.join("child.log"),
        Duration::from_millis(20),
    )
    .unwrap();
    assert!(run.timed_out && run.collection.root_waited);
    assert!(!run.clean());
    assert!(run.collection.remaining.is_empty());
}

#[test]
fn actual_orphan_is_collected_but_cannot_pass() {
    let _serial = PROCESS_TEST.lock().unwrap();
    process::arm_subreaper().unwrap();
    let temporary = Temporary::new();
    let run = process::run(
        process::private_command("/usr/bin/sh").args(["-c", "sleep 5 & exit 0"]),
        &temporary.0.join("child.log"),
        Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(run.returncode, Some(0));
    assert!(run.collection.root_waited);
    assert!(run.collection.descendants_found > 0);
    assert_eq!(
        run.collection.descendants_found,
        run.collection.descendants_reaped
    );
    assert!(run.collection.remaining.is_empty());
    assert!(!run.clean());
}

#[test]
fn bound_test_name_must_be_unique_and_exact() {
    let temporary = Temporary::new();
    let path = temporary.0.join("bindings.json");
    for suffix in ["test --ignored", "", "a::b"] {
        std::fs::write(
            &path,
            serde_json::json!({"schema":1,"cases":{
            "A.press_release_repress":format!("{}{suffix}", catalog::PREFIX)}})
            .to_string(),
        )
        .unwrap();
        assert!(catalog::bindings(&path).is_err());
    }
}

#[test]
fn component_names_are_closed_nonempty_unique_and_not_acceptance_aliases() {
    use super::components;
    let exact = "x11_socket::routing_tests::component_control".to_owned();
    components::validate_names(std::slice::from_ref(&exact)).unwrap();
    for names in [
        Vec::new(),
        vec![exact.clone(), exact],
        vec![format!("{}alias", catalog::PREFIX)],
        vec!["filter".into()],
    ] {
        assert!(components::validate_names(&names).is_err());
    }
    let (_, opts) = components::options(&[
        "--suite=retained-maintenance".into(),
        "--filter=anything".into(),
    ])
    .unwrap();
    assert!(host::options(&opts).is_err());
    assert!(components::options(&[]).is_err());
    assert!(components::options(&["--suite=a".into(), "--suite=b".into()]).is_err());
}

fn component_config() -> Config {
    serde_json::from_value(serde_json::json!({
        "schema":1,"run_id":"synthetic-component-fixture","self_test":false,
        "component_suite":"synthetic","component_tests":["x11_socket::routing_tests::component_control"],
        "build_timeout":1,"case_timeout":1,
        "source":{"commit":"synthetic","tree":"synthetic","clean":true,"archive_sha256":"synthetic","content_sha256":"synthetic"},
        "host_namespaces":{},"inventory_sha256":"synthetic","bindings_sha256":"synthetic",
        "xtask_sha256":"synthetic","toolchain_sha256":{}
    })).unwrap()
}

#[test]
fn component_pass_cannot_become_acceptance_or_omit_a_control() {
    use super::{components, worker};
    let config = component_config();
    let temporary = Temporary::new();
    let path = temporary.0.join("config.json");
    identity::json(&path, &config).unwrap();
    let mut report = worker::initial(&config, &inventory(), &path).unwrap();
    assert!(
        !worker::successful(&report, &config),
        "missing controls cannot pass"
    );
    let component = report.components.as_mut().unwrap();
    component.verdict = Verdict::Pass;
    component.tests[0].status = Verdict::Pass;
    component.tests[0].execution = Some(execution());
    assert!(worker::successful(&report, &config));
    assert_eq!(report.overall, Verdict::NotRun);
    assert!(
        report
            .cases
            .iter()
            .all(|case| case.status == Verdict::NotRun)
    );
    report.cases[0].status = Verdict::Pass;
    assert!(components::validate(&report, &config).is_err());
    report.cases[0].status = Verdict::NotRun;
    report.components.as_mut().unwrap().tests.clear();
    assert!(!worker::successful(&report, &config));
}

#[test]
fn component_parser_rejects_zero_ignored_timeout_and_uncollected_tests() {
    let exact = "x11_socket::routing_tests::component_control";
    let text = output(exact, &evidence(&inventory().cases[0]));
    evidence::validate_exact_test(exact, &execution(), &text).unwrap();
    for broken in [
        text.replace("running 1 test", "running 0 tests"),
        text.replace("... ok", "... ignored"),
    ] {
        assert!(evidence::validate_exact_test(exact, &execution(), &broken).is_err());
    }
    let mut run = execution();
    run.timed_out = true;
    assert!(evidence::validate_exact_test(exact, &run, &text).is_err());
    run.timed_out = false;
    run.collection.error = Some("cleanup failed".into());
    assert!(evidence::validate_exact_test(exact, &run, &text).is_err());
}
