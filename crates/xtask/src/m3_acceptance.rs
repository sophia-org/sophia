//! One headless M3 gate, with a closed case inventory and owned child processes.

mod catalog;
mod components;
mod evidence;
mod host;
mod identity;
mod m4;
mod process;
mod types;
mod worker;

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

#[path = "../tests/support/m3_acceptance.rs"]
mod tests;

#[path = "../tests/support/m4_acceptance.rs"]
mod m4_tests;
