#![cfg(test)]
//! Parser evidence is kept separate from Session acceptance.
use super::{catalog, identity, m4, types::*};
use std::collections::BTreeMap;

fn inventory() -> Inventory {
    serde_json::from_str(include_str!(
        "../../../../tools/probes/m4_acceptance/inventory.json"
    ))
    .unwrap()
}

fn execution() -> Execution {
    Execution {
        command: "synthetic M4 evidence parser fixture".into(),
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
        case: "M4.authorization".into(),
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
        "\nrunning 1 test\ntest authorization ... ok\nsophia_m4_acceptance {}\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.1s\n",
        serde_json::to_string(record).unwrap()
    )
}

fn validate(run: &Execution, text: &str) -> Result<CaseEvidence, String> {
    m4::validate_case(Gate::M4, &inventory().cases[1], "authorization", run, text)
}

#[test]
fn m4_inventory_is_distinct_and_cannot_promote_m3() {
    let value = inventory();
    assert!(catalog::validate(&value).is_err());
    let mut rows = catalog::initial(&value);
    assert_eq!(m4::overall(Gate::M4, &rows).unwrap(), Verdict::NotRun);
    for row in &mut rows {
        row.status = Verdict::Pass;
    }
    assert_eq!(m4::overall(Gate::M4, &rows).unwrap(), Verdict::Pass);
    rows.pop();
    assert!(m4::overall(Gate::M4, &rows).is_err());
    rows.push(rows[0].clone());
    assert!(m4::overall(Gate::M4, &rows).is_err());
}

#[test]
fn m4_missing_duplicate_and_foreign_records_are_rejected() {
    let evidence = record();
    let positive = output(&evidence);
    validate(&execution(), &positive).unwrap();
    assert!(validate(&execution(), "").is_err());
    assert!(validate(&execution(), &format!("{positive}{positive}")).is_err());
    let mut foreign = evidence;
    foreign.case = "C.capacity".into();
    assert!(validate(&execution(), &output(&foreign)).is_err());
}

#[test]
fn m4_each_subcase_and_each_actor_is_required() {
    let mut evidence = record();
    evidence.subcases.pop_first();
    assert!(validate(&execution(), &output(&evidence)).is_err());
    evidence = record();
    evidence.subcases.insert("disabled".into(), Verdict::NotRun);
    assert!(validate(&execution(), &output(&evidence)).is_err());
    evidence = record();
    evidence.cleanup.actors_collected = 0;
    assert!(validate(&execution(), &output(&evidence)).is_err());
    evidence = record();
    evidence.cleanup.pending_actors = 1;
    assert!(validate(&execution(), &output(&evidence)).is_err());
}

#[test]
fn m4_zero_tests_timeouts_and_leaked_processes_cannot_pass() {
    let positive = output(&record());
    for changed in [
        positive.replace("running 1 test", "running 0 tests"),
        positive.replace("authorization ... ok", "authorization ... ignored"),
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
    run.collection.descendants_found = 1;
    run.collection.descendants_reaped = 1;
    assert!(validate(&run, &positive).is_err());
}

#[test]
fn m4_unknown_build_target_identity_is_refused() {
    for invalid in ["", "HEAD", "../source", &"x".repeat(64)] {
        assert!(super::host::target_namespace(invalid).is_err());
    }
    assert_ne!(
        super::host::target_namespace(&"a".repeat(64)).unwrap(),
        super::host::target_namespace(&"b".repeat(64)).unwrap()
    );
}

#[test]
fn m4_report_cannot_borrow_a_different_source_or_target() {
    // Validation fails on identity before any containment or process fields
    // could authorize an aggregate. Both mismatches are preserved as failures.
    let temporary = std::env::temp_dir().join(format!("m4-identity-{}.json", std::process::id()));
    let config: Config = serde_json::from_value(serde_json::json!({
        "schema":1,"gate":"m4","run_id":"fixture","self_test":false,
        "component_tests":[],"build_timeout":1,"case_timeout":1,
        "source":{"commit":"fixture","tree":"fixture","clean":true,
        "archive_sha256":"fixture","content_sha256":"a".repeat(64)},
        "build_target_namespace":format!("source-{}", "a".repeat(64)),
        "host_namespaces":{},"inventory_sha256":"fixture","bindings_sha256":"fixture",
        "xtask_sha256":"fixture","toolchain_sha256":{}
    }))
    .unwrap();
    identity::json(&temporary, &config).unwrap();
    let mut report = super::worker::initial(&config, &inventory(), &temporary).unwrap();
    report.source.commit = "another source".into();
    assert!(
        super::worker::validate_report(&report, &config, &temporary)
            .unwrap_err()
            .contains("identity")
    );
    report.source = config.source.clone();
    report.build_target_namespace = format!("source-{}", "b".repeat(64));
    assert!(
        super::worker::validate_report(&report, &config, &temporary)
            .unwrap_err()
            .contains("identity")
    );
    std::fs::remove_file(temporary).unwrap();
}

#[test]
fn m4_changed_test_or_activation_binary_is_rejected() {
    let directory = std::env::temp_dir().join(format!("m4-binary-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("evidence")).unwrap();
    let test = directory.join("evidence/test-binary");
    let probe = directory.join("evidence/private-instance-probe");
    std::fs::write(&test, b"original test executable").unwrap();
    std::fs::write(&probe, b"original activation executable").unwrap();
    let mut report: Report = serde_json::from_value(serde_json::json!({
        "schema":1,"run_id":"fixture","purpose":"harness_self_test",
        "overall":"NOT_RUN","cases":[],
        "source":{"commit":"fixture","tree":"fixture","clean":true,
            "archive_sha256":"fixture","content_sha256":"fixture"},
        "build_target_namespace":"fixture","config_sha256":"fixture",
        "source_attested_inside":true,"source_unchanged_after":true,
        "binary":{"path":"evidence/test-binary","sha256":identity::digest(&test).unwrap(),
            "activation_probe":{"path":"evidence/private-instance-probe",
            "sha256":identity::digest(&probe).unwrap()}}
    }))
    .unwrap();
    m4::validate_binary(&report, &directory).unwrap();
    std::fs::write(&test, b"different executable").unwrap();
    assert!(
        m4::validate_binary(&report, &directory)
            .unwrap_err()
            .contains("binary identity")
    );
    std::fs::write(&test, b"original test executable").unwrap();
    std::fs::write(&probe, b"different activation executable").unwrap();
    assert!(
        m4::validate_binary(&report, &directory)
            .unwrap_err()
            .contains("probe identity")
    );
    std::fs::write(&probe, b"original activation executable").unwrap();
    report.binary.as_mut().unwrap()["path"] = serde_json::json!("../other/test-binary");
    assert!(m4::validate_binary(&report, &directory).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires the M4 contained runner's attested activation probe"]
fn m4_kernel_activation_excludes_an_outside_socket_and_keeps_a_delegated_pipe() {
    use sophia_conformance::private_instance::{Launch, Mount};
    use std::io::{Read, Write};
    use std::os::fd::{AsFd, AsRawFd};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::time::Duration;

    let directory = std::env::temp_dir().join(format!("m4-kernel-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let outside = directory.join("outside.sock");
    let listener = UnixListener::bind(&outside).unwrap();
    let mut positive = UnixStream::connect(&outside).unwrap();
    let (mut peer, _) = listener.accept().unwrap();
    // Make a real connected outside descriptor inheritable. It is deliberately
    // absent from the delegated inventory: the namespace launcher must close it.
    rustix::io::fcntl_setfd(&positive, rustix::io::FdFlags::empty()).unwrap();
    positive
        .write_all(b"authorized fabricated endpoint")
        .unwrap();
    let mut bytes = [0; 30];
    peer.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"authorized fabricated endpoint");
    // Keep the listener alive during the inner attempt, so its refusal cannot
    // be explained by a positive-control server that has already disappeared.
    let (read, write) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC).unwrap();
    let mut writer = std::fs::File::from(write);
    writer.write_all(b"explicit capability").unwrap();
    drop(writer);
    let launch = Launch {
        source: "/work/source".into(),
        directory: directory.join("run"),
        command: vec![
            "/work/probe".into(),
            "--activation-fd".into(),
            "{activation_fd}".into(),
            "--control-fd".into(),
            read.as_raw_fd().to_string(),
            "--outside".into(),
            outside.display().to_string(),
        ],
        mounts: vec![Mount {
            source: "/work/evidence/private-instance-probe".into(),
            destination: "/work/probe".into(),
            writable: false,
        }],
        timeout: Duration::from_secs(15),
    };
    let mut child = launch.spawn(&[read.as_fd()]).unwrap();
    let status = child.wait().unwrap();
    let text = std::fs::read_to_string(child.log()).unwrap();
    assert!(status.success(), "{text}");
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["activated"], true);
    assert_eq!(report["outside_connected"], false);
    assert_eq!(report["delegated"], "explicit capability");
    drop(listener);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
#[ignore = "requires the M4 contained runner's attested activation probe"]
fn m4_uncontained_and_forged_activation_cannot_start_a_host() {
    use std::time::Duration;
    let log = std::env::temp_dir().join(format!("m4-entry-{}.log", std::process::id()));
    for arguments in [
        vec![],
        vec!["--activation-fd", "-1"],
        vec!["--inside", "true"],
    ] {
        let run = super::process::run(
            super::process::private_command("/work/evidence/private-instance-probe")
                .args(arguments),
            &log,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(run.returncode, Some(1));
        assert!(!run.timed_out);
        assert_eq!(run.collection.descendants_found, 0);
    }
    std::fs::remove_file(log).unwrap();
}

#[test]
#[ignore = "requires the M4 contained runner's attested activation probe"]
fn m4_real_activation_pipe_cannot_disguise_unchanged_or_fake_namespaces() {
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::time::Duration;
    let log = std::env::temp_dir().join(format!("m4-forgery-{}.log", std::process::id()));
    for fake in [false, true] {
        let namespaces = sophia_conformance::private_instance::NAMESPACES.map(|name| {
            let path = if fake {
                "/dev/null".into()
            } else {
                format!("/proc/self/ns/{name}")
            };
            let file = std::fs::File::open(path).unwrap();
            rustix::io::fcntl_setfd(&file, rustix::io::FdFlags::empty()).unwrap();
            (name, file)
        });
        let (read, write) = rustix::pipe::pipe_with(rustix::pipe::PipeFlags::CLOEXEC).unwrap();
        let payload = serde_json::json!({"namespaces":namespaces.iter().map(|(name, file)|
            (*name, file.as_raw_fd())).collect::<BTreeMap<_,_>>(),"descriptors":{}});
        std::fs::File::from(write)
            .write_all(&serde_json::to_vec(&payload).unwrap())
            .unwrap();
        rustix::io::fcntl_setfd(&read, rustix::io::FdFlags::empty()).unwrap();
        let run = super::process::run(
            super::process::private_command("/work/evidence/private-instance-probe")
                .args(["--activation-fd", &read.as_raw_fd().to_string()]),
            &log,
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(run.returncode, Some(1));
        assert!(!run.timed_out);
        assert_eq!(run.collection.descendants_found, 0);
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .contains("did not cross the kernel")
        );
    }
    std::fs::remove_file(log).unwrap();
}
