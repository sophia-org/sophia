use super::*;
use std::os::unix::ffi::OsStrExt;
use udev::EventType::{Add, Bind, Change, Remove, Unbind, Unknown};

#[test]
fn device_lifecycle_uses_the_authoritative_event_phase() {
    for name in ["card0", "card12", "renderD128", "renderD130"] {
        for action in [Add, Bind, Change, Remove, Unbind, Unknown] {
            assert_eq!(
                topology_event_requires_rescan(
                    TopologyEventSource::Kernel,
                    action,
                    OsStr::new(name),
                    false
                ),
                matches!(action, Remove | Unbind),
                "kernel {name} {action:?}",
            );
            assert!(
                !topology_event_requires_rescan(
                    TopologyEventSource::Processed,
                    action,
                    OsStr::new(name),
                    false,
                ),
                "processed {name} {action:?}",
            );
        }
    }
}

#[test]
fn processed_device_rebuilds_require_a_settled_hotplug_change() {
    for action in [Add, Bind, Remove, Unbind, Unknown] {
        assert!(!topology_event_requires_rescan(
            TopologyEventSource::Processed,
            action,
            OsStr::new("card0"),
            false,
        ));
    }
    assert!(!topology_event_requires_rescan(
        TopologyEventSource::Processed,
        Change,
        OsStr::new("card0"),
        false,
    ));
    assert!(topology_event_requires_rescan(
        TopologyEventSource::Processed,
        Change,
        OsStr::new("card0"),
        true,
    ));
}

#[test]
fn connector_hotplug_is_retained_without_classifying_connectors_as_devices() {
    for name in ["card0-DP-1", "card1-HDMI-A-2"] {
        assert!(topology_event_requires_rescan(
            TopologyEventSource::Kernel,
            Change,
            OsStr::new(name),
            true
        ));
        assert!(!topology_event_requires_rescan(
            TopologyEventSource::Kernel,
            Change,
            OsStr::new(name),
            false
        ));
        for source in [TopologyEventSource::Kernel, TopologyEventSource::Processed] {
            for action in [Add, Bind, Remove, Unbind] {
                assert!(!topology_event_requires_rescan(
                    source,
                    action,
                    OsStr::new(name),
                    false
                ));
            }
        }
        assert!(!topology_event_requires_rescan(
            TopologyEventSource::Processed,
            Change,
            OsStr::new(name),
            true
        ));
    }
}

#[test]
fn incomplete_and_non_ascii_names_do_not_trigger_device_rebuilds() {
    for name in [
        "card",
        "renderD",
        "card-1",
        "card1extra",
        "renderD128extra",
        "card١",
        "controlD64",
    ] {
        for source in [TopologyEventSource::Kernel, TopologyEventSource::Processed] {
            for action in [Add, Bind, Change, Remove, Unbind] {
                assert!(!topology_event_requires_rescan(
                    source,
                    action,
                    OsStr::new(name),
                    false
                ));
            }
        }
    }
    assert!(!topology_event_requires_rescan(
        TopologyEventSource::Processed,
        Add,
        OsStr::from_bytes(b"card\xff"),
        false
    ));
}

fn monitor_fixture() -> (
    LiveDrmTopologyMonitor,
    SyncSender<()>,
    SyncSender<Result<(), String>>,
) {
    let (sender, ready) = sync_channel(1);
    let (_inventory_sender, inventory_ready) = sync_channel(1);
    let (health_sender, health) = sync_channel(1);
    (
        LiveDrmTopologyMonitor {
            ready,
            inventory_ready,
            inventory_baseline: None,
            inventory_dirty: false,
            inventory_retry_at: None,
            health,
            stop: Arc::new(AtomicBool::new(false)),
            latest_sequence: Arc::new(AtomicU64::new(0)),
            observed: Arc::new(AtomicU64::new(0)),
            coalesced: Arc::new(AtomicU64::new(0)),
            delivered: 0,
            worker: None,
        },
        sender,
        health_sender,
    )
}

fn identity(inode: u64, device_number: u64, physical: &str) -> LiveRenderDeviceIdentitySnapshot {
    LiveRenderDeviceIdentitySnapshot {
        node: format!("/dev/dri/renderD{inode}").into(),
        device: 1,
        inode,
        device_number,
        physical_device: physical.into(),
    }
}

#[test]
fn inventory_comparison_ignores_replays_but_detects_headless_membership_and_aba() {
    let a = identity(10, 128, "/sys/devices/gpu-a");
    let b = identity(20, 129, "/sys/devices/gpu-b");
    assert!(!inventory_changed(
        std::slice::from_ref(&a),
        std::slice::from_ref(&a)
    ));
    assert!(inventory_changed(
        std::slice::from_ref(&a),
        &[a.clone(), b.clone()]
    ));
    assert!(inventory_changed(
        &[a],
        &[identity(11, 128, "/sys/devices/gpu-a")]
    ));
}

fn publish(monitor: &LiveDrmTopologyMonitor, sender: &SyncSender<()>) -> Result<bool, String> {
    publish_topology_notice(
        sender,
        &monitor.latest_sequence,
        &monitor.observed,
        &monitor.coalesced,
    )
}

#[test]
fn a_burst_coalesces_to_one_notice_with_the_latest_local_sequence() {
    let (mut monitor, sender, _health) = monitor_fixture();
    for _ in 0..128 {
        assert_eq!(publish(&monitor, &sender), Ok(true));
    }
    assert_eq!(
        monitor.poll_notice().unwrap(),
        Some(LiveDrmTopologyRescanNotice { sequence: 128 })
    );
    assert_eq!(monitor.poll_notice().unwrap(), None);
    assert_eq!(
        monitor.stats(),
        LiveDrmTopologyMonitorStats {
            observed: 128,
            coalesced: 127,
            delivered: 1
        }
    );
}

#[test]
fn a_delivered_kernel_notice_does_not_suppress_post_database_reconciliation() {
    let (mut monitor, sender, _health) = monitor_fixture();
    // The same device change is observed before and after udev updates its database.
    for (source, expected_sequence) in [
        (TopologyEventSource::Kernel, 1),
        (TopologyEventSource::Processed, 2),
    ] {
        assert!(topology_event_requires_rescan(
            source,
            Change,
            OsStr::new("card0"),
            true
        ));
        assert_eq!(publish(&monitor, &sender), Ok(true));
        assert_eq!(
            monitor.poll_notice().unwrap(),
            Some(LiveDrmTopologyRescanNotice {
                sequence: expected_sequence
            })
        );
    }
    assert_eq!(monitor.poll_notice().unwrap(), None);
    assert_eq!(
        monitor.stats(),
        LiveDrmTopologyMonitorStats {
            observed: 2,
            coalesced: 0,
            delivered: 2
        }
    );
}

#[test]
fn exhausted_notice_sequence_fails_without_wrapping_or_publishing() {
    let (mut monitor, sender, _health) = monitor_fixture();
    monitor.latest_sequence.store(u64::MAX, Ordering::Release);
    assert!(publish(&monitor, &sender).is_err());
    assert_eq!(monitor.poll_notice().unwrap(), None);
    assert_eq!(monitor.stats(), LiveDrmTopologyMonitorStats::default());
}

#[test]
fn disconnected_notice_consumer_stops_the_publisher() {
    let (monitor, sender, _health) = monitor_fixture();
    let latest = Arc::clone(&monitor.latest_sequence);
    let observed = Arc::clone(&monitor.observed);
    let coalesced = Arc::clone(&monitor.coalesced);
    drop(monitor);
    assert_eq!(
        publish_topology_notice(&sender, &latest, &observed, &coalesced),
        Ok(false)
    );
}

#[test]
fn failed_render_inventory_comparison_keeps_dirty_and_paces_even_new_notices() {
    let (mut monitor, _sender, _health) = monitor_fixture();
    let (notices, ready) = sync_channel(1);
    monitor.inventory_ready = ready;
    let before = identity(10, 128, "/sys/devices/gpu-a");
    let after = identity(11, 128, "/sys/devices/gpu-a");
    monitor.inventory_baseline = Some(("seat-test".into(), vec![before.clone()]));
    let now = Instant::now();
    notices.send(()).unwrap();
    assert!(
        monitor
            .poll_render_inventory_with(now, |_| {
                Err(io::Error::other("transient inventory failure"))
            })
            .is_err()
    );
    let retry = now + Duration::from_millis(250);
    assert_eq!(monitor.inventory_retry_at, Some(retry));
    for offset in [0, 1, 100, 249] {
        notices.send(()).unwrap();
        assert!(
            !monitor
                .poll_render_inventory_with(now + Duration::from_millis(offset), |_| {
                    panic!("comparison repeated before its retry deadline")
                })
                .unwrap()
        );
        assert!(monitor.inventory_dirty);
        assert_eq!(
            monitor.inventory_retry_at,
            Some(retry),
            "new notice cannot move the deadline"
        );
        assert_eq!(
            monitor.render_inventory_snapshot().unwrap(),
            std::slice::from_ref(&before)
        );
    }
    assert!(
        monitor
            .poll_render_inventory_with(retry, |seat| {
                assert_eq!(seat, "seat-test");
                Ok(vec![after.clone()])
            })
            .unwrap()
    );
    assert!(!monitor.inventory_dirty);
    assert_eq!(monitor.inventory_retry_at, None);
    assert_eq!(monitor.render_inventory_snapshot().unwrap(), &[after]);
    assert!(
        !monitor
            .poll_render_inventory_with(retry, |_| {
                panic!("settled inventory is not re-read without another notice")
            })
            .unwrap()
    );
}
