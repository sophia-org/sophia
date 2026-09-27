//! Snapshot checks must reject changed source, extra files and contract drift.
#[path = "../src/c_desktop_sdk.rs"]
mod c_desktop_sdk;

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
    std::fs::write(&header, original).unwrap();
    let extra = source.join("extra.c");
    std::fs::write(&extra, b"unlisted").unwrap();
    assert!(c_desktop_sdk::verify(&snapshot, &repo).is_err());
    std::fs::remove_file(&extra).unwrap();
    std::os::unix::fs::symlink("src/sophia_9p_client.h", &extra).unwrap();
    assert!(
        c_desktop_sdk::verify(&snapshot, &repo)
            .unwrap_err()
            .contains("regular files")
    );
    std::fs::remove_file(&extra).unwrap();
    for dir in ["protocol", "docs/references"] {
        std::fs::create_dir_all(root.0.join(dir)).unwrap();
    }
    std::fs::write(
        root.0.join("protocol/sophia-shell-files-v1.kdl"),
        b"changed contract",
    )
    .unwrap();
    assert!(
        c_desktop_sdk::verify(&snapshot, &root.0)
            .unwrap_err()
            .contains("contract drift")
    );
}
