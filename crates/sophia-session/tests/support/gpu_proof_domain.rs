#![cfg(test)]

use super::*;
use std::os::unix::net::UnixListener;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "gpu-domain-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn device_and_descriptor_inventories_refuse_overflow() {
    let fixture = Fixture::new();
    for id in 0..INVENTORY_BOUND {
        fs::write(fixture.0.join(id.to_string()), []).unwrap();
    }
    let mut remaining = INVENTORY_BOUND;
    inspect_devices(&fixture.0, Path::new("/absent"), &mut remaining).unwrap();
    inspect_fds(&fixture.0).unwrap();
    fs::write(fixture.0.join("overflow"), []).unwrap();
    let mut remaining = INVENTORY_BOUND;
    assert!(inspect_devices(&fixture.0, Path::new("/absent"), &mut remaining).is_err());
    assert!(inspect_fds(&fixture.0).is_err());
}

#[test]
fn descriptor_observation_refuses_socket_and_missing_selected_device() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("ordinary"), []).unwrap();
    inspect_fds(&fixture.0).unwrap();
    let _socket = UnixListener::bind(fixture.0.join("socket")).unwrap();
    assert!(inspect_fds(&fixture.0).is_err());
    assert!(inspect(&fixture.0, "/dev/dri/renderD128", 226, 128).is_err());
}
