//! One headless M3 gate, with a closed case inventory and owned child processes.

mod catalog;
mod components;
mod evidence;
mod host;
mod identity;
mod m4;
mod m5;
mod m6;
mod process;
mod profiles;
mod types;
mod worker;
mod x11bench;

use std::path::Path;

pub fn run(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    process::arm_subreaper()?;
    match arguments {
        [inside] if inside == "--contained" => worker::run().map(|()| Vec::new()),
        [help] if help == "--help" => {
            Ok(vec!["cargo xtask check m3-acceptance --output=/NEW/DIR --target-dir=/OWNED/TARGET [--self-test]".into()])
        }
        _ => host::run(repo, arguments, None),
    }
}

pub fn run_components(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    process::arm_subreaper()?;
    let (suite, options) = components::options(arguments)?;
    host::run(repo, &options, Some(&suite))
}

pub fn run_m4(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    process::arm_subreaper()?;
    match arguments {
        [inside] if inside == "--contained" => worker::run().map(|()| Vec::new()),
        [help] if help == "--help" => Ok(vec![
            "cargo xtask check m4-acceptance --output=/NEW/DIR --target-dir=/OWNED/TARGET [--self-test]".into(),
        ]),
        _ => host::run_for(repo, arguments, None, types::Gate::M4),
    }
}

/// M6: every t093 gate on one exact source, composed into one verdict.
pub fn run_m6(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    m6::run(repo, arguments)
}

/// The X11 conformance profiles, xtest and native-input, as a gate.
pub fn run_profiles(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    profiles::run(repo, arguments)
}

pub fn run_m5(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    process::arm_subreaper()?;
    match arguments {
        [inside] if inside == "--contained" => worker::run().map(|()| Vec::new()),
        [help] if help == "--help" => Ok(vec![
            "cargo xtask check m5-acceptance --output=/NEW/DIR --target-dir=/OWNED/TARGET [--self-test]".into(),
        ]),
        _ => host::run_for(repo, arguments, None, types::Gate::M5),
    }
}

#[path = "../tests/support/m3_acceptance.rs"]
mod tests;

#[path = "../tests/support/m4_acceptance.rs"]
mod m4_tests;

#[path = "../tests/support/m4_integrity.rs"]
mod m4_integrity;

#[path = "../tests/support/m5_acceptance.rs"]
mod m5_tests;

#[path = "../tests/support/x11_profile.rs"]
mod profile_tests;

#[path = "../tests/support/x11bench.rs"]
mod x11bench_tests;

#[path = "../tests/support/m6_evidence.rs"]
mod m6_tests;
