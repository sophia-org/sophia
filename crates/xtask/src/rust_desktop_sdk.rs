//! Verify the immutable Rust desktop SDK snapshot, then run its own tests.
//!
//! The snapshot's Git tree must be the tree its recorded raw commit names, and
//! that commit must hash to the pinned revision (`git_tree`).
//!
//! Sophia builds the SDK's crates through path dependencies on
//! `vendor/rust-desktop-sdk/source`; the shell contract's conformance tests
//! live in the SDK, so this check is also what runs them against Sophia's
//! authoritative contract.
use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const REPOSITORY: &str = "https://github.com/sophia-org/sophia-desktop-sdk-rs";

/// Each SDK copy, and the Sophia file it must equal byte for byte.
const CONTRACTS: &[(&str, &str)] = &[(
    "spec/sophia-shell-files-v1.kdl",
    "protocol/sophia-shell-files-v1.kdl",
)];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    repository: String,
    revision: String,
    files: BTreeMap<String, String>,
}

pub fn run(repo: &Path) -> Result<Vec<String>, String> {
    let snapshot = repo.join("vendor/rust-desktop-sdk");
    let revision = verify(&snapshot, repo)?;
    let manifest = snapshot.join("source/Cargo.toml");
    let manifest = manifest.to_str().ok_or("non-UTF-8 SDK path")?;
    let target = repo.join("target/rust-desktop-sdk");
    let target = target.to_str().ok_or("non-UTF-8 target path")?;
    for arguments in [
        &[
            "test",
            "--offline",
            "--locked",
            "--workspace",
            "--manifest-path",
            manifest,
            "--target-dir",
            target,
        ][..],
        &[
            "test",
            "--offline",
            "--locked",
            "--workspace",
            "--all-features",
            "--manifest-path",
            manifest,
            "--target-dir",
            target,
        ],
    ] {
        let status = std::process::Command::new("cargo")
            .current_dir(repo)
            .args(arguments)
            .status()
            .map_err(|error| format!("could not run cargo: {error}"))?;
        if !status.success() {
            return Err(format!(
                "Rust desktop SDK tests {arguments:?} exited with {status}"
            ));
        }
    }
    Ok(vec![format!(
        "rust_desktop_sdk status=pass revision={revision}"
    )])
}

/// Checks the snapshot against its manifest and its contract copies against
/// Sophia's, and returns the pinned revision. Runs nothing.
pub fn verify(snapshot: &Path, repo: &Path) -> Result<String, String> {
    let path = snapshot.join("manifest.json");
    let bytes = std::fs::read(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid {}: {error}", path.display()))?;
    if manifest.schema != 1
        || manifest.repository != REPOSITORY
        || !hex(&manifest.revision, 40)
        || manifest.files.is_empty()
    {
        return Err("invalid Rust SDK snapshot identity".into());
    }
    for (name, digest) in &manifest.files {
        // Path::components drops `.` segments, so split the text itself: a
        // manifest name is plain relative segments and nothing else.
        if name
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
            || !hex(digest, 64)
        {
            return Err(format!("invalid Rust SDK manifest entry {name:?}"));
        }
    }
    let source = snapshot.join("source");
    let inventory = crate::git_tree::inventory(&source)?;
    let actual = &inventory.files;
    if let Some(name) = actual
        .iter()
        .find(|(name, digest)| manifest.files.get(*name) != Some(*digest))
        .map(|(name, _)| name)
    {
        return Err(format!("Rust SDK snapshot differs from its pin: {name}"));
    }
    if let Some(name) = manifest
        .files
        .keys()
        .find(|name| !actual.contains_key(*name))
    {
        return Err(format!("Rust SDK snapshot is missing {name}"));
    }
    for (local, authoritative) in CONTRACTS {
        if read(&source.join(local))? != read(&repo.join(authoritative))? {
            return Err(format!("Rust SDK contract drift: {authoritative}"));
        }
    }
    let sums = String::from_utf8(read(&source.join("spec/SHA256SUMS"))?)
        .map_err(|_| "spec/SHA256SUMS is not UTF-8".to_owned())?;
    for (local, _) in CONTRACTS {
        let name = Path::new(local)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("contract without a file name")?;
        let digest = format!("{:x}", Sha256::digest(read(&source.join(local))?));
        if !sums.lines().any(|line| line == format!("{digest}  {name}")) {
            return Err(format!("Rust SDK spec/SHA256SUMS does not record {name}"));
        }
    }
    crate::git_tree::verify_commit(
        &read(&snapshot.join("upstream.commit"))?,
        &manifest.revision,
        &inventory.tree,
    )?;
    Ok(manifest.revision)
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
