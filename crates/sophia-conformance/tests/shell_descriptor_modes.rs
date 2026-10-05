//! Exercise every descriptor host mode without a desktop implementation.
//! The host launches the independent C SDK peer under the production MetadataShell
//! domain. Engine decisions are headless; no rendering or hardware claim follows.

use std::fs::{self, DirBuilder, File};
use std::io::Read;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HOST: &str = env!("CARGO_BIN_EXE_shell_descriptor_conformance_host");
const LAUNCHER_HOST: &str = env!("CARGO_BIN_EXE_shell_launcher_conformance_host");
const LOG_CAP: u64 = 64 * 1024;
static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

#[path = "support/c_file_peer.rs"]
mod c_file_peer;

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Host(Child);

impl Drop for Host {
    fn drop(&mut self) {
        // The fixture forks no children. The host owns its protected supervisor;
        // bwrap's die-with-parent also ends that domain if we kill a stuck host.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn log(path: &Path) -> String {
    let mut bytes = Vec::new();
    File::open(path)
        .unwrap()
        .take(LOG_CAP + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(
        bytes.len() as u64 <= LOG_CAP,
        "host output exceeded cap: {path:?}"
    );
    String::from_utf8(bytes).unwrap()
}

fn run(
    executable: &str,
    mode: &str,
    fault: Option<&str>,
) -> (std::process::ExitStatus, String, String) {
    let scratch = Scratch(std::env::temp_dir().join(format!(
        "sophia-descriptor-modes-{}-{}-{}-{}",
        std::process::id(),
        mode,
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed),
    )));
    DirBuilder::new().mode(0o700).create(&scratch.0).unwrap();
    let peer = c_file_peer::build(&scratch.0, "shell_descriptor_file_peer", 0);
    let stdout = scratch.0.join("stdout");
    let stderr = scratch.0.join("stderr");
    let mut host = Host(
        Command::new(executable)
            .arg(peer)
            .args((executable == HOST).then_some(mode))
            .args(fault.map(|fault| format!("--fault={fault}")))
            .stdin(Stdio::null())
            .stdout(File::create(&stdout).unwrap())
            .stderr(File::create(&stderr).unwrap())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(45);
    let status = loop {
        for path in [&stdout, &stderr] {
            assert!(
                fs::metadata(path).unwrap().len() <= LOG_CAP,
                "host output exceeded cap: {path:?}"
            );
        }
        if let Some(status) = host.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            Instant::now() < deadline,
            "host {mode} timed out: {}",
            log(&stderr)
        );
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(fs::metadata(&stdout).unwrap().len() <= LOG_CAP);
    assert!(fs::metadata(&stderr).unwrap().len() <= LOG_CAP);
    let output = log(&stdout);
    let errors = log(&stderr);
    (status, output, errors)
}

fn passing(mode: &str) -> String {
    let (status, output, errors) = run(HOST, mode, None);
    assert!(
        status.success(),
        "host {mode}: {status}\n{output}\n{errors}"
    );
    output
}

fn descriptor_verdict(output: &str) {
    assert!(output.lines().any(|line| line == "sophia_shell_descriptor_corpus schema=1 status=complete protected=true descriptors=2 activations=1 withdrawn=true surface_ids_disclosed=0 coordinates_disclosed=0 icons_disclosed=0"), "{output}");
}

#[test]
fn descriptor_presentation_activation_and_withdrawal() {
    descriptor_verdict(&passing("--proof"));
}

#[test]
fn persistent_tabs_reference_and_descriptor_lifecycles() {
    let output = passing("--serve");
    assert!(output.lines().any(|line| line == "sophia_tab_protocol_proof status=complete supersession=true activation=true stale_epoch_rejected_by_host=true wire=9p"), "{output}");
    assert!(output.lines().any(|line| line == "sophia_reference_corpus status=complete entries=256 paging=true dismissal=true actions_disclosed=0 wire=9p"), "{output}");
    descriptor_verdict(&output);
}

#[test]
fn reservation_changes_work_area_only_at_commit_and_withdraws() {
    let output = passing("--bar-proof");
    assert!(output.lines().any(|line| line == "sophia_shell_reservation_corpus schema=1 status=complete protected=true edge=bottom thickness=28 reserved_height=1412 withdrawn=true"), "{output}");
}

#[test]
fn tab_acknowledgements_must_name_the_exact_event_and_transaction() {
    // File custody rejects stale epochs before the peer can acknowledge them;
    // the passing serve case checks that host-side refusal. These controls
    // exercise wrong identities and a refusal on a current activation.
    for fault in ["ack-activation", "ack-transaction", "ack-disposition"] {
        let (status, output, errors) = run(HOST, "--serve", Some(fault));
        assert_eq!(
            status.code(),
            Some(1),
            "{fault}: {status}\n{output}\n{errors}"
        );
        let expected = if fault == "ack-disposition" {
            "shell-descriptor-conformance-host: tab activation rejected"
        } else {
            "shell-descriptor-conformance-host: descriptor response timed out"
        };
        assert!(
            errors.lines().any(|line| line == expected),
            "{fault}: {errors}"
        );
        assert!(!output.contains("status=complete"), "{fault}: {output}");
    }
}

#[test]
fn launcher_catalog_presentation_activation_and_replay_over_files() {
    let (status, output, errors) = run(LAUNCHER_HOST, "--serve", None);
    assert!(status.success(), "{status}\n{output}\n{errors}");
    assert!(output.lines().any(|line| line == "sophia_launcher_conformance status=passed wire=9p catalog=4096 unpresented=denied_by_host replay=denied_by_client pending_query=denied_by_host protected=true clean_exit=true"), "{output}");
}
