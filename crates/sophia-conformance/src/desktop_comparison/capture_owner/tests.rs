#![cfg(test)]

use super::protect_owner_directory;
use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn capture_directory_is_tightened_independently_of_umask() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should follow epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "sophia-desktop-comparison-mode-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&path).expect("test directory should be created");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o775))
        .expect("test mode should be widened");
    protect_owner_directory(&path).expect("capture mode should be protected");
    let mode = fs::metadata(&path).expect("mode should be readable").mode() & 0o777;
    assert_eq!(mode, 0o700);
    fs::remove_dir(&path).expect("test directory should be removed");
}
