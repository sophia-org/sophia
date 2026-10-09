#![cfg(test)]

use super::*;
use std::ffi::OsStr;

fn candidate(index: u32) -> SeatDrmCard {
    SeatDrmCard {
        seat: "seat0".into(),
        gpu_id: Some(format!("pci-test:{index}").into()),
        node: format!("/dev/dri/card{index}").into(),
        sysfs_node: format!("/sys/devices/gpu{index}/drm/card{index}").into(),
        physical_device: format!("/sys/devices/gpu{index}").into(),
        device_number: rustix::fs::makedev(226, index),
        filesystem: 1,
        inode: u64::from(index) + 100,
    }
}

#[test]
fn excluded_gpu_is_not_inspected_even_after_card_renumbering() {
    let admission = crate::LiveGpuAdmission::new(["pci-0000:16:00.0".into()]).unwrap();
    let mut cards = Vec::new();
    for node in ["card0", "card9"] {
        admit_seat_card(
            &mut cards,
            "seat0",
            OsStr::new(node),
            true,
            None,
            &admission,
            Some(OsStr::new("pci-0000:16:00.0")),
            || panic!("excluded GPU reached node inspection"),
        )
        .unwrap();
    }
    assert!(cards.is_empty());
    admit_seat_card(
        &mut cards,
        "seat0",
        OsStr::new("card0"),
        true,
        None,
        &admission,
        Some(OsStr::new("pci-0000:03:00.0")),
        || Ok(candidate(0)),
    )
    .unwrap();
    assert_eq!(cards.len(), 1);
    admit_seat_card(
        &mut cards,
        "seat0",
        OsStr::new("card1"),
        true,
        Some(OsStr::new("seat1")),
        &admission,
        Some(OsStr::new("pci-0000:03:00.0")),
        || panic!("exclusion admitted a foreign seat"),
    )
    .unwrap();
    assert_eq!(cards.len(), 1);
    assert!(
        admit_seat_card(
            &mut cards,
            "seat0",
            OsStr::new("card2"),
            true,
            None,
            &admission,
            None,
            || panic!("unknown identity reached inspection")
        )
        .is_err()
    );
}

#[test]
fn seat_selection_never_inspects_foreign_cards_or_connector_records() {
    let mut admitted = Vec::new();
    for (name, initialized, seat) in [
        ("card0", true, None),
        ("card1", true, Some("seat0")),
        ("card3-HDMI-A-1", true, Some("development")),
        ("card4-HDMI-A-1", false, None),
    ] {
        admit_seat_card(
            &mut admitted,
            "development",
            OsStr::new(name),
            initialized,
            seat.map(OsStr::new),
            &crate::LiveGpuAdmission::default(),
            Some(OsStr::new("pci-test:0")),
            || panic!("foreign card or connector record inspected"),
        )
        .unwrap();
    }
    assert!(admitted.is_empty());
    admit_seat_card(
        &mut admitted,
        "development",
        OsStr::new("card4"),
        true,
        Some(OsStr::new("development")),
        &crate::LiveGpuAdmission::default(),
        Some(OsStr::new("pci-test:4")),
        || Ok(candidate(4)),
    )
    .unwrap();
    assert_eq!(admitted, vec![candidate(4)]);
}

#[test]
fn an_uninitialized_card_refuses_the_inventory_before_node_inspection() {
    for seat in ["seat0", "development"] {
        for assigned in [None, Some("seat0"), Some("development")] {
            let mut admitted = vec![candidate(0)];
            let error = admit_seat_card(
                &mut admitted,
                seat,
                OsStr::new("card1"),
                false,
                assigned.map(OsStr::new),
                &crate::LiveGpuAdmission::default(),
                Some(OsStr::new("pci-test:1")),
                || panic!("an uninitialized card must never be inspected"),
            )
            .expect_err("an uninitialized card must refuse, not disappear");
            assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
            assert_eq!(admitted, vec![candidate(0)]);
        }
    }
}

#[test]
fn an_admitted_card_failure_refuses_instead_of_hiding_a_device() {
    let mut admitted = vec![candidate(0)];
    let result = admit_seat_card(
        &mut admitted,
        "seat0",
        OsStr::new("card1"),
        true,
        None,
        &crate::LiveGpuAdmission::default(),
        Some(OsStr::new("pci-test:1")),
        || Err(io::Error::other("admitted node disappeared")),
    );
    assert!(result.is_err());
    assert_eq!(admitted, vec![candidate(0)]);
}

#[test]
fn the_primary_inventory_is_bounded_and_refuses_identity_aliases() {
    let mut admitted = Vec::new();
    for index in 0..16 {
        admit_card(&mut admitted, candidate(index)).unwrap();
    }
    admit_card(&mut admitted, candidate(0)).unwrap();
    let before = admitted.clone();
    assert!(admit_card(&mut admitted, candidate(16)).is_err());
    for which in 0..3 {
        let mut duplicate = candidate(17);
        match which {
            0 => duplicate.node = candidate(0).node,
            1 => duplicate.device_number = candidate(0).device_number,
            2 => duplicate.physical_device = candidate(0).physical_device,
            _ => unreachable!(),
        }
        assert!(admit_card(&mut admitted, duplicate).is_err());
    }
    assert_eq!(admitted, before);
}

#[test]
#[cfg(all(feature = "seat-control", feature = "libdrm-events"))]
fn a_returned_descriptor_must_be_the_admitted_node() {
    let file = fs::File::open("/dev/null").unwrap();
    let original = rustix::fs::fstat(&file).unwrap();
    let mut card = candidate(0);
    card.device_number = original.st_rdev;
    card.filesystem = original.st_dev;
    card.inode = original.st_ino;
    card.validate_opened(&original).unwrap();
    for field in 0..4 {
        let mut different = original;
        match field {
            0 => different.st_rdev ^= 1,
            1 => different.st_dev ^= 1,
            2 => different.st_ino ^= 1,
            3 => different.st_mode = 0,
            _ => unreachable!(),
        }
        assert!(card.validate_opened(&different).is_err(), "field {field}");
    }
}
