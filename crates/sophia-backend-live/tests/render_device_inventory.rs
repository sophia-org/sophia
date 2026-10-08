#![cfg(feature = "drm-hotplug")]

use std::ffi::OsStr;
use std::fs::{self, File};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sophia_backend_live::{LiveRenderDeviceInventoryError, discover_seat_render_devices};

#[path = "../src/drm/seat_inventory/policy.rs"]
mod seat_policy;
#[path = "../src/drm/render_inventory/selection.rs"]
mod selection;
use seat_policy::{is_node_name, seat_matches, valid_seat};
use selection::{RenderCandidate, admit_candidate, render_sibling, validate_identity};

use LiveRenderDeviceInventoryError as E;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "sophia-render-inventory-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn physical(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir(&path).unwrap();
        path.canonicalize().unwrap()
    }

    fn render(&self, name: &str, physical: &Path, minor: u32) -> RenderCandidate {
        let node = self.0.join(name);
        fs::create_dir(&node).unwrap();
        symlink(physical, node.join("device")).unwrap();
        fs::write(node.join("dev"), format!("226:{minor}\n")).unwrap();
        RenderCandidate {
            sysfs_node: node,
            physical_device: physical.to_owned(),
            device_number: rustix::fs::makedev(226, minor),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn explicit_seat_membership_does_not_admit_another_seat() {
    assert!(seat_matches("seat0", true, None));
    assert!(!seat_matches("seat1", true, None));
    assert!(seat_matches("seat1", true, Some(OsStr::new("seat1"))));
    assert!(!seat_matches("seat1", true, Some(OsStr::new("seat0"))));
    assert!(!seat_matches("seat0", true, Some(OsStr::new(""))));
    for seat in ["", "seat 0", "seat\n0", "sëat0"] {
        assert!(!valid_seat(seat));
        assert_eq!(
            discover_seat_render_devices(seat).unwrap_err(),
            E::InvalidSeat
        );
    }
}

#[test]
fn uninitialized_udev_records_never_authorize_a_render_device() {
    for seat in ["seat0", "seat1"] {
        for assigned in [None, Some(OsStr::new("seat0")), Some(OsStr::new("seat1"))] {
            assert!(!seat_matches(seat, false, assigned));
        }
    }
}

#[test]
fn connector_and_incomplete_node_names_are_not_device_candidates() {
    for name in ["card0", "card11"] {
        assert!(is_node_name(OsStr::new(name), "card"));
    }
    for name in [
        "card",
        "card0-HDMI-A-1",
        "card-1",
        "card1extra",
        "renderD128",
    ] {
        assert!(!is_node_name(OsStr::new(name), "card"));
    }
    assert!(is_node_name(OsStr::new("renderD130"), "renderD"));
    assert!(!is_node_name(OsStr::new("renderD130extra"), "renderD"));
}

#[test]
fn headless_physical_identity_selects_the_sibling_independent_of_number_order() {
    let fixture = Fixture::new();
    let selected_physical = fixture.physical("physical-a");
    let unrelated_physical = fixture.physical("physical-b");
    let selected = fixture.render("renderD140", &selected_physical, 140);
    fixture.render("renderD128", &unrelated_physical, 128);
    symlink(fixture.0.join("gone"), fixture.0.join("renderD129")).unwrap();
    fs::create_dir(fixture.0.join("renderD130")).unwrap();
    // No connector, mode, vendor, or boot-primary evidence exists in this fixture.
    assert_eq!(
        render_sibling(&fixture.0, &selected_physical),
        Ok(Some(selected))
    );
    let absent = fixture.physical("physical-absent");
    assert_eq!(render_sibling(&fixture.0, &absent), Ok(None));
}

#[test]
fn multiple_render_siblings_are_refused_instead_of_choosing_first() {
    let fixture = Fixture::new();
    let physical = fixture.physical("physical");
    fixture.render("renderD128", &physical, 128);
    fixture.render("renderD129", &physical, 129);
    assert_eq!(
        render_sibling(&fixture.0, &physical),
        Err(E::AmbiguousRenderNode)
    );
}

#[test]
fn capacity_counts_distinct_devices_and_never_partially_admits_the_seventeenth() {
    let mut selected = Vec::new();
    for index in 0..16 {
        admit_candidate(&mut selected, candidate(index)).unwrap();
    }
    admit_candidate(&mut selected, candidate(7)).unwrap();
    let before = selected.clone();
    assert_eq!(
        admit_candidate(&mut selected, candidate(16)),
        Err(E::CapacityExceeded)
    );
    assert_eq!(selected, before);

    let mut conflicting = candidate(7);
    conflicting.sysfs_node = PathBuf::from("different-node");
    assert_eq!(
        admit_candidate(&mut selected, conflicting),
        Err(E::AmbiguousRenderNode)
    );
    assert_eq!(selected, before);

    let mut reused_device_number = candidate(17);
    reused_device_number.device_number = candidate(7).device_number;
    assert_eq!(
        admit_candidate(&mut selected, reused_device_number),
        Err(E::AmbiguousRenderNode)
    );
    assert_eq!(selected, before);
}

fn candidate(index: u32) -> RenderCandidate {
    RenderCandidate {
        sysfs_node: PathBuf::from(format!("/sys/class/drm/renderD{}", 128 + index)),
        physical_device: PathBuf::from(format!("/sys/devices/physical-{index}")),
        device_number: rustix::fs::makedev(226, 128 + index),
    }
}

#[test]
fn opening_must_preserve_node_and_physical_identity() {
    let file = File::open("/dev/null").unwrap();
    let original = rustix::fs::fstat(&file).unwrap();
    let mut candidate = candidate(0);
    candidate.device_number = original.st_rdev;
    let physical = candidate.physical_device.clone();
    assert_eq!(
        validate_identity(&candidate, &original, &original, &original, &physical),
        Ok(())
    );

    for phase in 0..3 {
        for field in 0..3 {
            let mut observations = [original; 3];
            let changed = &mut observations[phase];
            match field {
                0 => changed.st_dev ^= 1,
                1 => changed.st_ino ^= 1,
                2 => changed.st_rdev ^= 1,
                _ => unreachable!(),
            }
            assert_eq!(
                validate_identity(
                    &candidate,
                    &observations[0],
                    &observations[1],
                    &observations[2],
                    &physical
                ),
                Err(E::IdentityChanged),
                "phase {phase}, field {field}"
            );
        }
    }
    assert_eq!(
        validate_identity(
            &candidate,
            &original,
            &original,
            &original,
            Path::new("/sys/devices/replacement")
        ),
        Err(E::IdentityChanged)
    );
    let mut not_character = original;
    not_character.st_mode = 0;
    assert_eq!(
        validate_identity(&candidate, &original, &not_character, &original, &physical),
        Err(E::InvalidDevice)
    );
}
