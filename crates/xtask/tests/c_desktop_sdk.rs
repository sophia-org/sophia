//! Snapshot checks must reject changed source, extra files and contract drift.
#[path = "../src/c_desktop_sdk.rs"]
mod c_desktop_sdk;
#[path = "../src/git_tree.rs"]
mod git_tree;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn pinned_source_and_contract_are_both_required() {
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(c_desktop_sdk::run(&repo).is_ok());
    let root = Scratch(std::env::temp_dir().join(format!(
        "sophia-sdk-pin-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    let snapshot = root.0.join("vendor/c-desktop-sdk");
    std::fs::create_dir_all(&snapshot).unwrap();
    assert!(
        std::process::Command::new("cp")
            .args(["-R", "--"])
            .arg(repo.join("vendor/c-desktop-sdk/source"))
            .arg(&snapshot)
            .status()
            .unwrap()
            .success()
    );
    std::fs::copy(
        repo.join("vendor/c-desktop-sdk/manifest.json"),
        snapshot.join("manifest.json"),
    )
    .unwrap();
    std::fs::copy(
        repo.join("vendor/c-desktop-sdk/upstream.commit"),
        snapshot.join("upstream.commit"),
    )
    .unwrap();
    assert!(c_desktop_sdk::verify(&snapshot, &repo).is_ok());
    let source = snapshot.join("source");
    let header = source.join("src/sophia_9p_client.h");
    let original = std::fs::read(&header).unwrap();
    std::fs::write(&header, b"corrupt").unwrap();
    assert!(
        c_desktop_sdk::verify(&snapshot, &repo)
            .unwrap_err()
            .contains("differs from its pin")
    );
    std::fs::write(&header, &original).unwrap();
    std::fs::remove_file(&header).unwrap();
    assert!(
        c_desktop_sdk::verify(&snapshot, &repo)
            .unwrap_err()
            .contains("differs from its pin")
    );
    std::fs::write(&header, &original).unwrap();
    let mode = std::fs::metadata(&header).unwrap().permissions().mode();
    std::fs::set_permissions(&header, std::fs::Permissions::from_mode(mode | 0o100)).unwrap();
    assert!(
        c_desktop_sdk::verify(&snapshot, &repo)
            .unwrap_err()
            .contains("source tree")
    );
    std::fs::set_permissions(&header, std::fs::Permissions::from_mode(mode)).unwrap();
    let extra = source.join("extra.c");
    std::fs::write(&extra, b"unlisted").unwrap();
    assert!(
        c_desktop_sdk::verify(&snapshot, &repo)
            .unwrap_err()
            .contains("differs from its pin: extra.c")
    );
    std::fs::remove_file(&extra).unwrap();
    std::os::unix::fs::symlink("src/sophia_9p_client.h", &extra).unwrap();
    assert!(
        c_desktop_sdk::verify(&snapshot, &repo)
            .unwrap_err()
            .contains("regular files")
    );
    std::fs::remove_file(&extra).unwrap();
    let moved = snapshot.join("source-real");
    std::fs::rename(&source, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &source).unwrap();
    assert!(
        c_desktop_sdk::verify(&snapshot, &repo)
            .unwrap_err()
            .contains("must not be symlinked")
    );
    std::fs::remove_file(&source).unwrap();
    std::fs::rename(&moved, &source).unwrap();

    let manifest_path = snapshot.join("manifest.json");
    let manifest_bytes = std::fs::read(&manifest_path).unwrap();
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
    for (key, value) in [
        ("schema", serde_json::json!(2)),
        (
            "repository",
            serde_json::json!("https://example.invalid/other"),
        ),
        ("revision", serde_json::json!("not-a-commit")),
        (
            "revision",
            serde_json::json!("0000000000000000000000000000000000000000"),
        ),
    ] {
        let mut bad = manifest.clone();
        bad[key] = value;
        std::fs::write(&manifest_path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(c_desktop_sdk::verify(&snapshot, &repo).is_err());
    }
    for name in ["../outside", "/absolute", "", "src/./file", "src//file"] {
        let mut bad = manifest.clone();
        bad["files"][name] = serde_json::json!("0".repeat(64));
        std::fs::write(&manifest_path, serde_json::to_vec(&bad).unwrap()).unwrap();
        assert!(
            c_desktop_sdk::verify(&snapshot, &repo)
                .unwrap_err()
                .contains("manifest entry")
        );
    }
    std::fs::write(&manifest_path, manifest_bytes).unwrap();

    let mut contracts = vec![
        "protocol/sophia-shell-files-v1.kdl".to_owned(),
        "protocol/sophia-shell-v1.kdl".to_owned(),
        "docs/sophia-9p-profile.md".to_owned(),
        "docs/sophia-shell-files.md".to_owned(),
        "docs/references/diod-9p2000L-protocol.md".to_owned(),
        "bindings/c/sophia_wm_v1.c".to_owned(),
        "bindings/c/sophia_wm_v1.h".to_owned(),
    ];
    for name in [
        "catalog-actions",
        "content-malformed",
        "content",
        "indicators",
        "launcher",
        "native-launcher",
        "reference",
        "tabs",
        "v1-malformed",
        "v1",
    ] {
        contracts.push(format!("protocol/golden/sophia-shell-{name}.frames"));
    }
    for name in &contracts {
        let path = root.0.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(repo.join(name), path).unwrap();
    }
    assert!(c_desktop_sdk::verify(&snapshot, &root.0).is_ok());
    for name in &contracts {
        let path = root.0.join(name);
        std::fs::write(&path, b"changed contract").unwrap();
        let error = c_desktop_sdk::verify(&snapshot, &root.0).unwrap_err();
        assert!(
            error.contains("contract drift") && error.contains(name),
            "{error}"
        );
        std::fs::copy(repo.join(name), path).unwrap();
    }
}
