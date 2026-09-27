//! Snapshot checks must reject every way the vendored Rust SDK can drift from
//! its pin or from Sophia's contract.
#[path = "../src/rust_desktop_sdk.rs"]
mod rust_desktop_sdk;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A private copy of the snapshot and of the contract it must equal.
fn scratch() -> (Scratch, PathBuf) {
    let root = Scratch(std::env::temp_dir().join(format!(
        "sophia-rust-sdk-pin-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    let snapshot = root.0.join("vendor/rust-desktop-sdk");
    std::fs::create_dir_all(&snapshot).unwrap();
    std::fs::create_dir_all(root.0.join("protocol")).unwrap();
    assert!(
        std::process::Command::new("cp")
            .args(["-R", "--"])
            .arg(repo().join("vendor/rust-desktop-sdk/source"))
            .arg(&snapshot)
            .status()
            .unwrap()
            .success()
    );
    for name in ["manifest.json", "upstream.commit"] {
        std::fs::copy(
            repo().join("vendor/rust-desktop-sdk").join(name),
            snapshot.join(name),
        )
        .unwrap();
    }
    std::fs::copy(
        repo().join("protocol/sophia-shell-files-v1.kdl"),
        root.0.join("protocol/sophia-shell-files-v1.kdl"),
    )
    .unwrap();
    (root, snapshot)
}

fn refused(snapshot: &Path, repo: &Path, expected: &str) {
    let error = rust_desktop_sdk::verify(snapshot, repo).unwrap_err();
    assert!(error.contains(expected), "{error:?} lacks {expected:?}");
}

fn edit_manifest(snapshot: &Path, edit: impl FnOnce(&mut serde_json::Value)) {
    let path = snapshot.join("manifest.json");
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    edit(&mut value);
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn the_committed_snapshot_verifies() {
    // `run` also builds and tests the snapshot; the gate runs it, not this test.
    let _ = rust_desktop_sdk::run;
    assert!(rust_desktop_sdk::verify(&repo().join("vendor/rust-desktop-sdk"), &repo()).is_ok());
    let (root, snapshot) = scratch();
    assert!(rust_desktop_sdk::verify(&snapshot, &root.0).is_ok());
}

#[test]
fn changed_missing_extra_and_linked_files_are_refused() {
    let (root, snapshot) = scratch();
    let source = snapshot.join("source");
    let lib = source.join("crates/sophia-9p-records/src/lib.rs");
    let original = std::fs::read(&lib).unwrap();
    std::fs::write(&lib, b"corrupt").unwrap();
    refused(&snapshot, &root.0, "differs from its pin");
    std::fs::remove_file(&lib).unwrap();
    refused(
        &snapshot,
        &root.0,
        "is missing crates/sophia-9p-records/src/lib.rs",
    );
    std::fs::write(&lib, original).unwrap();
    let extra = source.join("extra.rs");
    std::fs::write(&extra, b"unlisted").unwrap();
    refused(&snapshot, &root.0, "differs from its pin: extra.rs");
    std::fs::remove_file(&extra).unwrap();
    std::os::unix::fs::symlink("Cargo.toml", &extra).unwrap();
    refused(&snapshot, &root.0, "only regular files");
    std::fs::remove_file(&extra).unwrap();
    assert!(rust_desktop_sdk::verify(&snapshot, &root.0).is_ok());
}

#[test]
fn a_linked_source_root_is_refused() {
    let (root, snapshot) = scratch();
    let real = snapshot.join("real");
    std::fs::rename(snapshot.join("source"), &real).unwrap();
    std::os::unix::fs::symlink(&real, snapshot.join("source")).unwrap();
    refused(&snapshot, &root.0, "must be a directory");
}

#[test]
fn bad_identities_and_entries_are_refused() {
    for edit in [
        |m: &mut serde_json::Value| m["schema"] = 2.into(),
        |m: &mut serde_json::Value| m["repository"] = "https://example.invalid/sdk".into(),
        |m: &mut serde_json::Value| m["revision"] = "0f840d9".into(),
        |m: &mut serde_json::Value| m["revision"] = "G".repeat(40).into(),
        |m: &mut serde_json::Value| m["files"] = serde_json::json!({}),
    ] {
        let (root, snapshot) = scratch();
        edit_manifest(&snapshot, edit);
        refused(&snapshot, &root.0, "snapshot identity");
    }
    for name in ["../escape", "/absolute", "crates/./x", ""] {
        let (root, snapshot) = scratch();
        edit_manifest(&snapshot, |m| {
            m["files"][name] = "0".repeat(64).into();
        });
        refused(&snapshot, &root.0, "manifest entry");
    }
    let (root, snapshot) = scratch();
    edit_manifest(&snapshot, |m| m["files"]["Cargo.toml"] = "abc".into());
    refused(&snapshot, &root.0, "manifest entry");
    let (root, snapshot) = scratch();
    edit_manifest(&snapshot, |m| m["extra"] = true.into());
    refused(&snapshot, &root.0, "invalid");
}

#[test]
fn contract_drift_and_an_unrecorded_digest_are_refused() {
    let (root, snapshot) = scratch();
    std::fs::write(
        root.0.join("protocol/sophia-shell-files-v1.kdl"),
        b"changed contract",
    )
    .unwrap();
    refused(
        &snapshot,
        &root.0,
        "contract drift: protocol/sophia-shell-files-v1.kdl",
    );

    // A copy and a pin that agree with each other, but not with the recorded
    // digest, still fail.
    let (root, snapshot) = scratch();
    let copy = snapshot.join("source/spec/sophia-shell-files-v1.kdl");
    let mut changed = std::fs::read(&copy).unwrap();
    changed.extend_from_slice(b"// drift\n");
    std::fs::write(&copy, &changed).unwrap();
    std::fs::write(root.0.join("protocol/sophia-shell-files-v1.kdl"), &changed).unwrap();
    let digest = {
        use sha2::Digest as _;
        format!("{:x}", sha2::Sha256::digest(&changed))
    };
    edit_manifest(&snapshot, |m| {
        m["files"]["spec/sophia-shell-files-v1.kdl"] = digest.into();
    });
    refused(&snapshot, &root.0, "SHA256SUMS does not record");
}
