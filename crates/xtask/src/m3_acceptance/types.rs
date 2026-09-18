use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Verdict {
    Pass,
    Fail,
    NotRun,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Case {
    pub case: String,
    pub gate: String,
    pub required: bool,
    pub requirement: String,
    pub subcases: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Inventory {
    pub schema: u32,
    pub plan_source_commit: String,
    pub cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Bindings {
    pub schema: u32,
    pub cases: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Collection {
    pub root_waited: bool,
    pub descendants_found: usize,
    pub descendants_reaped: usize,
    pub remaining: Vec<u32>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Execution {
    pub command: String,
    pub returncode: Option<i32>,
    pub timed_out: bool,
    pub elapsed_millis: u128,
    pub collection: Collection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ActorCollection {
    pub actors_started: usize,
    pub actors_collected: usize,
    pub pending_actors: usize,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CaseEvidence {
    pub schema: u32,
    pub case: String,
    pub subcases: BTreeMap<String, Verdict>,
    pub cleanup: ActorCollection,
    pub observations: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CaseResult {
    pub case: String,
    pub status: Verdict,
    pub reason: String,
    pub subcases: BTreeMap<String, Verdict>,
    pub test: Option<String>,
    pub execution: Option<Execution>,
    pub evidence: Option<CaseEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SourceIdentity {
    pub commit: String,
    pub tree: String,
    pub clean: bool,
    pub archive_sha256: String,
    pub content_sha256: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Config {
    pub schema: u32,
    pub run_id: String,
    pub self_test: bool,
    #[serde(default)]
    pub component_suite: Option<String>,
    #[serde(default)]
    pub component_tests: Vec<String>,
    pub build_timeout: u64,
    pub case_timeout: u64,
    pub build_target_namespace: String,
    pub source: SourceIdentity,
    pub host_namespaces: BTreeMap<String, String>,
    pub inventory_sha256: String,
    pub bindings_sha256: String,
    pub xtask_sha256: String,
    pub toolchain_sha256: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Containment {
    pub validated: bool,
    pub namespaces: BTreeMap<String, String>,
    pub delegated_descriptors: usize,
    pub render_devices_present: bool,
    pub input_devices_present: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Report {
    pub schema: u32,
    pub run_id: String,
    pub purpose: String,
    pub overall: Verdict,
    pub cases: Vec<CaseResult>,
    pub source: SourceIdentity,
    pub build_target_namespace: String,
    pub config_sha256: String,
    pub containment: Option<Containment>,
    pub build: Option<Execution>,
    pub binary: Option<serde_json::Value>,
    pub self_tests: Option<Execution>,
    #[serde(default)]
    pub components: Option<ComponentReport>,
    pub launcher: Option<Execution>,
    pub source_attested_inside: bool,
    pub source_unchanged_after: bool,
    pub harness_error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ComponentReport {
    pub suite: String,
    pub verdict: Verdict,
    pub tests: Vec<ComponentResult>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct ComponentResult {
    pub test: String,
    pub status: Verdict,
    pub reason: String,
    pub execution: Option<Execution>,
}

pub(super) struct Options {
    pub output: PathBuf,
    pub target: PathBuf,
    pub registry: PathBuf,
    pub self_test: bool,
    pub timeout: u64,
    pub build_timeout: u64,
    pub case_timeout: u64,
}
