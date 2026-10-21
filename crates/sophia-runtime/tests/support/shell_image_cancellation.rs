//! Isolated descriptor accounting for ring registration and transport teardown.
//! In-flight cancel/expiry tests live beside the owned-copy owner, where the
//! worker completion queue can be withheld deterministically.
use super::*;

pub(super) fn isolated_shutdown() {
    const CHILD: &str = "SOPHIA_SHELL_IMAGE_SHUTDOWN_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "disconnected_transport_reclaims_ring_source_custody",
                "--exact",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("transport shutdown child stalled");
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    // The child runs this test alone, so unrelated test FDs cannot alter the
    // baseline. The admitted ring remains owned until transport teardown.
    let descriptors = || std::fs::read_dir("/proc/self/fd").unwrap().count();
    let baseline = descriptors();
    run(Case::Disconnect);
    let deadline = Instant::now() + Duration::from_secs(3);
    while descriptors() != baseline {
        assert!(
            Instant::now() < deadline,
            "transport/worker retained descriptors"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
