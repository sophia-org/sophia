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
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const REPOSITORY: &str = "https://github.com/sophia-org/sophia-desktop-sdk-rs";

/// Each SDK copy, and the Sophia file it must equal byte for byte.
pub const CONTRACTS: &[(&str, &str)] = &[
    (
        "spec/sophia-shell-files-v1.kdl",
        "protocol/sophia-shell-files-v1.kdl",
    ),
    ("spec/sophia-shell-files.md", "docs/sophia-shell-files.md"),
    (
        "spec/sophia-shell-descriptors.md",
        "docs/sophia-shell-descriptors.md",
    ),
    // The shell contract adopts the WM envelope, custody and retry rules
    // (the bounded EAGAIN backoff, no exactly-once across disconnect) that
    // the client implements.
    ("spec/sophia-wm-files.md", "docs/sophia-wm-files.md"),
    ("spec/sophia-9p-profile.md", "docs/sophia-9p-profile.md"),
    (
        "spec/references/diod-9p2000L-protocol.md",
        "docs/references/diod-9p2000L-protocol.md",
    ),
];

#[derive(Deserialize, Serialize)]
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
    let target =
        target_root(repo, std::env::var_os("CARGO_TARGET_DIR").as_deref()).join("rust-desktop-sdk");
    let target = target.to_str().ok_or("non-UTF-8 target path")?;
    let arguments = [
        "test",
        "--offline",
        "--locked",
        "--workspace",
        "--all-features",
        "--manifest-path",
        manifest,
        "--target-dir",
        target,
    ];
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
    Ok(vec![format!(
        "rust_desktop_sdk status=pass revision={revision}"
    )])
}

/// Nested SDK checks honor Cargo's selected build directory. A contained
/// check must not fall back to writing into its read-only source checkout.
pub fn target_root(repo: &Path, configured: Option<&std::ffi::OsStr>) -> PathBuf {
    repo.join(
        configured
            .map(Path::new)
            .unwrap_or_else(|| Path::new("target")),
    )
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
        let name = local
            .strip_prefix("spec/")
            .ok_or("contract copy outside spec/")?;
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

/// Replaces the vendored snapshot with the signed SDK `revision` from the
/// local checkout `sdk`, offline: `git archive` into a scratch stage, the raw
/// commit object beside it, the manifest from the shared Git inventory. The
/// stage must pass [`verify`] before it replaces anything, so a failure
/// leaves the current pin untouched.
pub fn vendor(repo: &Path, arguments: &[String]) -> Result<Vec<String>, String> {
    let [sdk, revision] = arguments else {
        return Err("usage: cargo xtask vendor-rust-desktop-sdk SDK_CHECKOUT REVISION".into());
    };
    let sdk = Path::new(sdk);
    let git = |arguments: &[&str]| -> Result<Vec<u8>, String> {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(sdk)
            .args(arguments)
            .output()
            .map_err(|error| format!("could not run git: {error}"))?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(format!(
                "git {arguments:?}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }
    };
    let revision = String::from_utf8(git(&[
        "rev-parse",
        "--verify",
        &format!("{revision}^{{commit}}"),
    ])?)
    .map_err(|_| "non-UTF-8 revision".to_owned())?
    .trim()
    .to_owned();
    git(&["verify-commit", &revision])?;
    let stage = target_root(repo, std::env::var_os("CARGO_TARGET_DIR").as_deref())
        .join(format!("rust-desktop-sdk-stage-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let result = (|| {
        let source = stage.join("source");
        std::fs::create_dir_all(&source)
            .map_err(|error| format!("could not create {}: {error}", source.display()))?;
        let archive = git(&["archive", &revision])?;
        let mut tar = std::process::Command::new("tar")
            .arg("-x")
            .arg("-C")
            .arg(&source)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not run tar: {error}"))?;
        std::io::Write::write_all(tar.stdin.as_mut().ok_or("tar stdin")?, &archive)
            .map_err(|error| format!("could not feed tar: {error}"))?;
        drop(tar.stdin.take());
        if !tar.wait().map_err(|error| error.to_string())?.success() {
            return Err("tar could not extract the SDK archive".to_owned());
        }
        let raw = git(&["cat-file", "commit", &revision])?;
        install(repo, &stage, &raw, &revision)
    })();
    let _ = std::fs::remove_dir_all(&stage);
    result.map(|()| vec![format!("rust_desktop_sdk vendored revision={revision}")])
}

/// Completes a stage holding `source/`: writes its raw commit and manifest,
/// verifies it as the check does, and only then swaps it in for the
/// committed snapshot.
pub fn install(repo: &Path, stage: &Path, raw: &[u8], revision: &str) -> Result<(), String> {
    let write = |path: &Path, bytes: &[u8]| {
        std::fs::write(path, bytes)
            .map_err(|error| format!("could not write {}: {error}", path.display()))
    };
    write(&stage.join("upstream.commit"), raw)?;
    let inventory = crate::git_tree::inventory(&stage.join("source"))?;
    let manifest = Manifest {
        schema: 1,
        repository: REPOSITORY.to_owned(),
        revision: revision.to_owned(),
        files: inventory.files,
    };
    let mut json = serde_json::to_string_pretty(&manifest).map_err(|error| error.to_string())?;
    json.push('\n');
    write(&stage.join("manifest.json"), json.as_bytes())?;
    verify(stage, repo)?;
    let snapshot = repo.join("vendor/rust-desktop-sdk");
    let retired = stage.join("retired-source");
    let rename = |from: &Path, to: &Path| {
        std::fs::rename(from, to).map_err(|error| {
            format!(
                "could not move {} to {}: {error}",
                from.display(),
                to.display()
            )
        })
    };
    rename(&snapshot.join("source"), &retired)?;
    rename(&stage.join("source"), &snapshot.join("source"))?;
    for name in ["manifest.json", "upstream.commit"] {
        rename(&stage.join(name), &snapshot.join(name))?;
    }
    std::fs::remove_dir_all(&retired)
        .map_err(|error| format!("could not remove {}: {error}", retired.display()))
}
