//! Exercise the actual libseat ownership adapter without a seat service or GPU.
//! The child fixes the backend before libseat initializes its process state.

use std::fs;
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::process::Command;

use sophia_seat_device::SeatDevice;

const CHILD: &str = "SOPHIA_SEAT_DESCRIPTOR_TEST_CHILD";

fn descriptor_count() -> usize {
    fs::read_dir("/proc/self/fd")
        .unwrap()
        .map(|entry| entry.unwrap())
        .count()
}

#[test]
fn released_seat_devices_close_their_descriptors() {
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new("timeout")
            .args(["-s", "KILL", "10"])
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "released_seat_devices_close_their_descriptors",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .env("LIBSEAT_BACKEND", "noop")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "descriptor child failed: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        return;
    }

    assert_eq!(std::env::var("LIBSEAT_BACKEND").unwrap(), "noop");
    let mut seat = libseat::Seat::open(|_, _| {}).expect("noop seat backend");
    let baseline = descriptor_count();
    for cycle in 0..64 {
        let device = SeatDevice::open(&mut seat, Path::new("/dev/null")).unwrap();
        let descriptor = format!("/proc/self/fd/{}", device.as_fd().as_raw_fd());
        assert!(fs::read_link(&descriptor).is_ok(), "cycle {cycle}: open fd");
        device.close(&mut seat).unwrap();
        assert_eq!(
            fs::read_link(&descriptor).unwrap_err().kind(),
            std::io::ErrorKind::NotFound,
            "cycle {cycle}: released descriptor remains open",
        );
    }
    assert_eq!(
        descriptor_count(),
        baseline,
        "repeated device releases leaked descriptors",
    );
}
