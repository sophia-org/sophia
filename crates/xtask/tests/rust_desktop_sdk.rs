//! Snapshot checks must reject every way the vendored Rust SDK can drift from
//! its pin or from Sophia's contract.
#[path = "../src/git_tree.rs"]
mod git_tree;
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
    std::fs::create_dir_all(root.0.join("protocol/golden")).unwrap();
    std::fs::create_dir_all(root.0.join("docs/references")).unwrap();
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
    for (_, authoritative) in rust_desktop_sdk::CONTRACTS {
        std::fs::copy(repo().join(authoritative), root.0.join(authoritative)).unwrap();
    }
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
    // `run` also builds and tests the snapshot, and `vendor` needs a signed
    // SDK checkout; the gate and the operator run those, not this test.
    let _ = rust_desktop_sdk::run;
    let _ = rust_desktop_sdk::vendor;
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
    refused(&snapshot, &root.0, "must not be symlinked");
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
fn drift_in_every_contract_pair_and_an_unrecorded_digest_are_refused() {
    // Six file contracts remain after the socket schema and seven frame
    // corpora retire. Every retained pair still gets a drift control.
    assert_eq!(rust_desktop_sdk::CONTRACTS.len(), 6);
    for (_, authoritative) in rust_desktop_sdk::CONTRACTS {
        let (root, snapshot) = scratch();
        std::fs::write(root.0.join(authoritative), b"changed contract").unwrap();
        refused(
            &snapshot,
            &root.0,
            &format!("contract drift: {authoritative}"),
        );
    }

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

#[test]
fn the_tree_and_revision_are_bound_to_the_recorded_commit() {
    // A source change recorded consistently in the manifest still fails: the
    // tree no longer matches the pinned commit's.
    let (root, snapshot) = scratch();
    let file = snapshot.join("source/README.md");
    let mut changed = std::fs::read(&file).unwrap();
    changed.extend_from_slice(b"\nchanged\n");
    std::fs::write(&file, &changed).unwrap();
    let digest = {
        use sha2::Digest as _;
        format!("{:x}", sha2::Sha256::digest(&changed))
    };
    edit_manifest(&snapshot, |m| m["files"]["README.md"] = digest.into());
    refused(&snapshot, &root.0, "does not match its pinned commit");

    // Another well-formed revision does not name the recorded commit.
    let (root, snapshot) = scratch();
    edit_manifest(&snapshot, |m| m["revision"] = "1".repeat(40).into());
    refused(&snapshot, &root.0, "does not identify upstream.commit");

    // Nor does an edited commit object.
    let (root, snapshot) = scratch();
    let commit = snapshot.join("upstream.commit");
    let mut raw = std::fs::read(&commit).unwrap();
    raw.extend_from_slice(b"forged\n");
    std::fs::write(&commit, raw).unwrap();
    refused(&snapshot, &root.0, "does not identify upstream.commit");
}

#[test]
fn installing_the_pinned_revision_reproduces_the_committed_snapshot() {
    let (root, snapshot) = scratch();
    let stage = root.0.join("stage");
    std::fs::create_dir_all(&stage).unwrap();
    assert!(
        std::process::Command::new("cp")
            .args(["-R", "--"])
            .arg(repo().join("vendor/rust-desktop-sdk/source"))
            .arg(&stage)
            .status()
            .unwrap()
            .success()
    );
    let raw = std::fs::read(snapshot.join("upstream.commit")).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(snapshot.join("manifest.json")).unwrap()).unwrap();
    let revision = manifest["revision"].as_str().unwrap().to_owned();
    rust_desktop_sdk::install(&root.0, &stage, &raw, &revision).unwrap();
    for name in ["manifest.json", "upstream.commit"] {
        assert_eq!(
            std::fs::read(snapshot.join(name)).unwrap(),
            std::fs::read(repo().join("vendor/rust-desktop-sdk").join(name)).unwrap(),
            "{name}"
        );
    }
    assert!(rust_desktop_sdk::verify(&snapshot, &root.0).is_ok());
}

#[test]
fn a_stage_that_fails_verification_leaves_the_pin_untouched() {
    let (root, snapshot) = scratch();
    let before = std::fs::read(snapshot.join("manifest.json")).unwrap();
    let stage = root.0.join("stage");
    std::fs::create_dir_all(stage.join("source")).unwrap();
    std::fs::write(stage.join("source/README.md"), b"not the SDK").unwrap();
    let raw = std::fs::read(snapshot.join("upstream.commit")).unwrap();
    let revision = "0".repeat(40);
    assert!(rust_desktop_sdk::install(&root.0, &stage, &raw, &revision).is_err());
    assert_eq!(
        std::fs::read(snapshot.join("manifest.json")).unwrap(),
        before
    );
    assert!(rust_desktop_sdk::verify(&snapshot, &root.0).is_ok());
}

#[test]
fn nested_sdk_work_uses_the_selected_cargo_target() {
    let source = Path::new("/readonly/source");
    assert_eq!(
        rust_desktop_sdk::target_root(source, Some(std::ffi::OsStr::new("/private/build"))),
        Path::new("/private/build")
    );
    assert_eq!(
        rust_desktop_sdk::target_root(source, Some(std::ffi::OsStr::new("relative-build"))),
        source.join("relative-build")
    );
    assert_eq!(
        rust_desktop_sdk::target_root(source, None),
        source.join("target")
    );
}
