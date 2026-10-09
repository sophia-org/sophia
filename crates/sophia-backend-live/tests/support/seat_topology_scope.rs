#![cfg(test)]

use super::*;
use TopologyEventSource::{Kernel, Processed};
use udev::EventType::{Add, Bind, Change, Remove, Unbind};

fn card(gpu: &str, index: u32) -> SeatDrmCard {
    SeatDrmCard {
        seat: "seat0".into(),
        gpu_id: Some(format!("pci-{gpu}").into()),
        node: format!("/dev/dri/card{index}").into(),
        sysfs_node: format!("/sys/devices/{gpu}/drm/card{index}").into(),
        physical_device: format!("/sys/devices/{gpu}").into(),
        device_number: rustix::fs::makedev(226, index),
        filesystem: 1,
        inode: u64::from(index) + 100,
    }
}

fn event(
    scope: &mut SeatTopologyScope,
    source: TopologyEventSource,
    action: udev::EventType,
    path: &str,
    hotplug: bool,
) -> bool {
    let path = Path::new(path);
    scope.observe(source, action, path.file_name().unwrap(), path, hotplug)
}

#[test]
fn a_foreign_hotplug_never_notifies_even_when_card_names_or_connector_ids_match() {
    let mut scope = SeatTopologyScope::new(vec![card("desktop", 1)]);
    for source in [Kernel, Processed] {
        for action in [Change, Remove, Unbind] {
            for path in [
                "/sys/devices/development/drm/card1",
                "/sys/devices/development/drm/card1/card1-DP-1",
                "/sys/devices/development/drm/renderD128",
                "/sys/devices/desktop/drm/card10",
                "/sys/devices/desktop/drm/card10/card10-DP-1",
                "/sys/devices/desktop/drm/card1/card10-DP-1",
                "/sys/devices/desktop/drm/renderD128extra",
                "/sys/devices/desktop/drm/renderD",
            ] {
                assert!(
                    !event(&mut scope, source, action, path, true),
                    "foreign {path}"
                );
            }
        }
    }
    assert!(
        !scope
            .refresh(Instant::now(), || Ok(vec![card("desktop", 1)]))
            .unwrap(),
        "foreign processed events cannot create a notice when membership is unchanged"
    );
}

#[test]
fn a_shared_pci_bridge_does_not_make_render_nodes_siblings() {
    let mut scope = SeatTopologyScope::new(vec![card("bridge/gpu-a", 0)]);
    assert!(event(
        &mut scope,
        Kernel,
        Remove,
        "/sys/devices/bridge/gpu-a/drm/renderD128",
        false
    ));
    assert!(!event(
        &mut scope,
        Kernel,
        Remove,
        "/sys/devices/bridge/gpu-b/drm/renderD129",
        false
    ));
}

#[test]
fn a_removal_already_reflected_in_the_baseline_cannot_revoke_another_card() {
    let mut scope = SeatTopologyScope::new(vec![card("desktop", 1)]);
    let removed = "/sys/devices/gone/drm/card0";
    assert!(!event(&mut scope, Kernel, Remove, removed, false));
    assert!(!event(&mut scope, Processed, Remove, removed, false));
    assert!(
        !scope
            .refresh(Instant::now(), || Ok(vec![card("desktop", 1)]))
            .unwrap(),
        "a queued pre-baseline removal is a replay, not a new notice"
    );
}

#[test]
fn cached_admission_preserves_kernel_revocation_without_a_current_seat_lookup() {
    let mut scope = SeatTopologyScope::new(vec![card("desktop", 1)]);
    for path in [
        "/sys/devices/desktop/drm/card1",
        "/sys/devices/desktop/drm/renderD129",
    ] {
        for action in [Remove, Unbind] {
            assert!(
                event(&mut scope, Kernel, action, path, false),
                "revocation {path}"
            );
        }
    }
    assert!(event(
        &mut scope,
        Kernel,
        Change,
        "/sys/devices/desktop/drm/card1/card1-DP-1",
        true
    ));
    assert!(
        !scope
            .refresh(Instant::now(), || panic!(
                "kernel events cannot admit a device"
            ))
            .unwrap()
    );
}

#[test]
fn only_settled_comparison_can_admit_a_new_card_and_replay_is_quiet() {
    let mut scope = SeatTopologyScope::new(vec![card("desktop", 1)]);
    let new = "/sys/devices/development/drm/card0";
    for action in [Add, Bind, Change] {
        assert!(!event(&mut scope, Kernel, action, new, true));
    }
    assert!(
        !scope
            .refresh(Instant::now(), || panic!(
                "kernel event admitted a new card"
            ))
            .unwrap()
    );
    assert!(!event(&mut scope, Processed, Add, new, false));
    let admitted = vec![card("development", 0), card("desktop", 1)];
    assert!(
        scope
            .refresh(Instant::now(), || Ok(admitted.clone()))
            .unwrap(),
        "new membership notifies"
    );
    assert!(event(&mut scope, Kernel, Change, new, true));
    for action in [Add, Bind, Change] {
        assert!(!event(&mut scope, Processed, action, new, false));
        assert!(
            !scope
                .refresh(Instant::now(), || Ok(admitted.clone()))
                .unwrap(),
            "replay is quiet"
        );
    }
}

#[test]
fn reassignment_and_removal_notify_before_forgetting_the_previous_card() {
    for action in [Change, Remove, Unbind] {
        let mut scope = SeatTopologyScope::new(vec![card("desktop", 1)]);
        let path = "/sys/devices/desktop/drm/card1";
        event(&mut scope, Processed, action, path, false);
        assert!(
            scope.refresh(Instant::now(), || Ok(vec![])).unwrap(),
            "removal or a foreign current assignment still revokes the previous member"
        );
        assert!(
            !event(&mut scope, Kernel, Change, path, true),
            "no longer admitted"
        );
    }
}

#[test]
fn failed_comparison_keeps_revocation_identity_and_retries_without_a_new_event() {
    let mut scope = SeatTopologyScope::new(vec![card("desktop", 1)]);
    let path = "/sys/devices/desktop/drm/card1";
    let now = Instant::now();
    event(&mut scope, Processed, Change, path, false);
    assert!(
        scope
            .refresh(now, || Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "uninitialized"
            )))
            .is_err()
    );
    assert!(
        event(&mut scope, Kernel, Remove, path, false),
        "failed comparison preserves revocation"
    );
    for elapsed in [0, 1, 100, 249] {
        event(&mut scope, Processed, Change, path, false);
        assert!(
            !scope
                .refresh(now + Duration::from_millis(elapsed), || panic!(
                    "retry was not paced"
                ))
                .unwrap()
        );
    }
    assert!(
        scope
            .refresh(now + Duration::from_millis(250), || Ok(vec![]))
            .unwrap(),
        "retry must run with no additional event and observe the withdrawal"
    );
    assert!(
        !scope
            .refresh(now + Duration::from_millis(300), || panic!(
                "settled state re-read"
            ))
            .unwrap()
    );
}

#[test]
fn a_failed_comparison_remains_owed_when_no_further_event_arrives() {
    let mut scope = SeatTopologyScope::new(vec![card("desktop", 1)]);
    let now = Instant::now();
    event(
        &mut scope,
        Processed,
        Change,
        "/sys/devices/desktop/drm/card1",
        false,
    );
    assert!(
        scope
            .refresh(now, || Err(io::Error::other("udev is unsettled")))
            .is_err()
    );
    // No observe/mark_dirty between failure and the next poll: the failed
    // comparison itself must preserve this obligation.
    assert!(
        scope
            .refresh(now + Duration::from_millis(250), || Ok(vec![]))
            .unwrap(),
        "the failed comparison must retry without another event"
    );
}

#[test]
fn path_reuse_renumbering_and_node_replacement_are_membership_changes() {
    let original = card("desktop", 1);
    for replacement in [
        card("other-gpu", 1),
        card("desktop", 3),
        SeatDrmCard {
            inode: 500,
            ..original.clone()
        },
        SeatDrmCard {
            device_number: rustix::fs::makedev(226, 5),
            ..original.clone()
        },
    ] {
        let mut scope = SeatTopologyScope::new(vec![original.clone()]);
        scope.mark_dirty();
        assert!(
            scope
                .refresh(Instant::now(), || Ok(vec![replacement]))
                .unwrap(),
            "membership compares identity, not just a card name"
        );
    }
}

#[test]
fn a_queued_relevant_event_after_baseline_still_publishes_and_a_full_queue_keeps_revocation() {
    use super::super::{LiveDrmTopologyMonitorStats, publish_topology_notice};
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::sync_channel,
    };
    let mut scope = SeatTopologyScope::new(vec![card("desktop", 1)]);
    let (sender, receiver) = sync_channel(1);
    let sequence = AtomicU64::new(0);
    let observed = AtomicU64::new(0);
    let coalesced = AtomicU64::new(0);
    for action in [Change, Remove, Unbind] {
        if event(
            &mut scope,
            Kernel,
            action,
            "/sys/devices/desktop/drm/card1",
            true,
        ) {
            assert!(publish_topology_notice(&sender, &sequence, &observed, &coalesced).unwrap());
        }
    }
    receiver
        .try_recv()
        .expect("queued baseline/revocation notice remains deliverable");
    assert!(receiver.try_recv().is_err());
    assert_eq!(sequence.load(Ordering::Acquire), 3);
    assert_eq!(
        LiveDrmTopologyMonitorStats {
            observed: observed.load(Ordering::Acquire),
            coalesced: coalesced.load(Ordering::Acquire),
            delivered: 1
        },
        LiveDrmTopologyMonitorStats {
            observed: 3,
            coalesced: 2,
            delivered: 1
        }
    );
}
