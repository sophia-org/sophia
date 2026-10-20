//! Supplied mailbox installation: no native executor or admission is installed.
use super::*;
use crate::captured_resource_fixture as fixture;
use sophia_renderer_live::*;
use std::{os::unix::net::UnixStream, time::Instant};

#[test]
fn maintenance_continues_after_cache_eviction_until_last_consumer_drops() {
    let mut connections = ShellComponentConnections::new().unwrap();
    let grant = sophia_protocol::ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    connections.epochs = fixture::allocation_registry(grant);
    let metadata = fixture::metadata(grant);
    let mut ticket = Some(
        connections
            .epochs
            .reserve_capture_image(grant, &metadata, 0)
            .unwrap(),
    );
    let supplied = fixture::storage(1);
    assert!(supplied.epoch.is_valid());
    connections
        .epochs
        .accept_capture_image(&mut ticket, &mut Some(supplied.image.clone()), 0)
        .unwrap();
    assert!(connections.epochs.disconnect(grant));
    assert_eq!(
        connections.capture_maintenance_wait(),
        Duration::from_millis(10)
    );
    connections
        .retire_capture_imports(&mut 1, &mut |_| Ok::<_, ()>(()))
        .unwrap();
    assert!(!connections.epochs.capture_retirement_pending());
    assert_eq!(
        connections.capture_maintenance_wait(),
        Duration::from_millis(10),
        "cache acknowledgment is not last-consumer release"
    );
    assert_eq!(supplied.budget.usage(), (1, 64));
    drop(supplied.image);
    connections.collect();
    assert_eq!(supplied.budget.usage(), (0, 0));
    assert_eq!(connections.capture_maintenance_wait(), Duration::MAX);
}

#[test]
fn completion_subscription_and_teardown_follow_pending_custody() {
    let mut connections = ShellComponentConnections::new().unwrap();
    let epoch = LiveRendererSnapshotEpoch::new(1, 2, 3);
    let (inbox, executor) = live_shell_capture_channel(epoch.clone()).unwrap();
    assert!(connections.install_capture_owner(inbox).is_ok());
    let (other, _executor) = live_shell_capture_channel(epoch.clone()).unwrap();
    assert!(
        connections.install_capture_owner(other).is_err(),
        "never replace custody owner"
    );
    let mut count = 0;
    connections.visit_capture_completion(&mut |_| count += 1);
    assert_eq!(count, 0);
    assert_eq!(connections.capture_maintenance_wait(), Duration::MAX);
    let grant = sophia_protocol::ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    connections.epochs = fixture::allocation_registry(grant);
    let metadata = fixture::metadata(grant);
    let ticket = connections
        .epochs
        .reserve_capture_image(grant, &metadata, 0)
        .unwrap();
    let budget = LiveRendererSnapshotBudget::new(1, 64);
    let (fd, _) = UnixStream::pair().unwrap();
    let request = LiveShellCaptureRequest {
        source: LiveShellCaptureSource {
            fd: fd.into(),
            width: 4,
            height: 4,
            stride: 16,
            modifier: 0,
            offset: 0,
        },
        image_id: LiveRendererImageId::from_raw(1),
        reservation: budget.reserve_capture(64).unwrap(),
        budget: budget.clone(),
        epoch,
    };
    connections
        .capture
        .as_mut()
        .unwrap()
        .submit(
            Binding {
                slot: 0,
                grant,
                metadata,
                plane: sophia_protocol::shell_files::ShellImagePlane {
                    offset: 0,
                    stride: 16,
                    fd_index: 0,
                },
            },
            request,
            ticket,
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
    connections.visit_capture_completion(&mut |_| count += 1);
    assert_eq!(count, 1);
    assert_eq!(
        connections.capture_maintenance_wait(),
        Duration::from_millis(10)
    );
    assert!(
        connections.finish_after_backend_drop(()).is_err(),
        "in-flight source survives shutdown request"
    );
    drop(executor); // Real Returned receipt, before any GPU use.
    assert!(connections.service_capture_completions().is_empty());
    assert_eq!(connections.capture.as_ref().unwrap().pending(), 0);
    assert_eq!(budget.usage(), (0, 0)); // No exact live journal in this fixture.
    count = 0;
    connections.visit_capture_completion(&mut |_| count += 1);
    assert_eq!(count, 0);
    assert_eq!(connections.capture_maintenance_wait(), Duration::MAX);
}
