//! Real mailbox custody, supplied endpoint driver; no GPU or admission.
use super::*;
use crate::captured_resource_fixture as fixture;
use sophia_renderer_live::{
    LiveShellCaptureCustody, LiveShellCaptureExecutor, LiveShellCaptureRequest,
    LiveShellCaptureSource,
};
use std::os::{fd::AsRawFd, unix::net::UnixStream};

#[path = "capture_source.rs"]
mod source_tests;

#[derive(Default)]
struct Native {
    requests: Vec<OutputId>,
    polls: usize,
    closes: usize,
    busy_request: bool,
    busy_close: bool,
    reply: Option<Endpoint>,
    executor: Option<LiveShellCaptureExecutor>,
}
impl Native {
    fn offer(&mut self, output: u64) -> Destination {
        let destination = Destination {
            output: OutputId::from_raw(output),
            budget: LiveRendererSnapshotBudget::new(4, 256),
            epoch: LiveRendererSnapshotEpoch::new(1, 2, 3),
        };
        let (inbox, executor) = LiveShellCaptureCustody::default()
            .channel(destination.epoch.clone(), destination.budget.clone())
            .unwrap();
        self.executor = Some(executor);
        self.reply = Some(Endpoint {
            inbox,
            destination: destination.clone(),
        });
        destination
    }
}
impl Backend for Native {
    fn request(&mut self, output: OutputId) -> io::Result<()> {
        if self.busy_request {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        self.requests.push(output);
        Ok(())
    }
    fn poll(&mut self) -> io::Result<Option<Endpoint>> {
        self.polls += 1;
        Ok(self.reply.take())
    }
    fn close(&mut self) -> io::Result<()> {
        self.closes += 1;
        if self.busy_close {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        self.executor.take();
        self.reply.take();
        Ok(())
    }
}

#[test]
fn demand_installs_once_and_never_steals_another_output() {
    let mut link = Link::default();
    let mut owner = None;
    let mut native = Native::default();
    link.service(&mut native, &mut owner, true).unwrap();
    assert_eq!(
        (native.requests.len(), native.polls, native.closes),
        (0, 0, 0)
    );
    assert!(!link.demand(OutputId::from_raw(1), &owner).unwrap());
    assert!(link.needs_turn());
    native.busy_request = true;
    assert!(link.service(&mut native, &mut owner, true).is_err());
    assert!(!link.requested);
    native.busy_request = false;
    link.service(&mut native, &mut owner, true).unwrap();
    assert!(link.demand(OutputId::from_raw(2), &owner).is_err());
    let destination = native.offer(1);
    link.service(&mut native, &mut owner, true).unwrap();
    assert_eq!(native.requests.len(), 1);
    assert!(link.demand(OutputId::from_raw(1), &owner).unwrap());
    assert!(Arc::ptr_eq(
        &link.destination.as_ref().unwrap().budget,
        &destination.budget
    ));
    assert!(!link.needs_turn());
    assert!(link.demand(OutputId::from_raw(2), &owner).is_err());
    link.service(&mut native, &mut owner, true).unwrap();
    assert_eq!(
        native.polls, 2,
        "installed owner is not repeatedly polled for installation"
    );
}

#[test]
fn withdrawn_endpoint_keeps_close_intent_until_queue_accepts_then_allows_drained_replacement() {
    let mut link = Link::default();
    let mut owner = None;
    let mut native = Native::default();
    link.demand(OutputId::from_raw(1), &owner).unwrap();
    let old = native.offer(1);
    link.service(&mut native, &mut owner, true).unwrap();
    old.epoch.invalidate();
    native.busy_close = true;
    assert!(link.service(&mut native, &mut owner, true).is_err());
    assert!(link.needs_turn());
    assert!(owner.as_ref().unwrap().replaceable());
    assert!(link.demand(OutputId::from_raw(1), &owner).is_err());
    native.busy_close = false;
    link.service(&mut native, &mut owner, true).unwrap();
    assert!(!link.needs_turn());
    assert!(!link.demand(OutputId::from_raw(2), &owner).unwrap());
    let new = native.offer(2);
    link.service(&mut native, &mut owner, true).unwrap();
    assert!(new.epoch.is_valid());
    assert!(!old.epoch.is_valid());
    assert!(!owner.as_ref().unwrap().is_closed());
}

#[test]
fn stale_or_wrong_output_reply_is_never_installed_and_shutdown_never_requests() {
    for stale in [false, true] {
        let mut link = Link::default();
        let mut owner = None;
        let mut native = Native::default();
        link.demand(OutputId::from_raw(1), &owner).unwrap();
        let d = native.offer(if stale { 1 } else { 2 });
        if stale {
            d.epoch.invalidate();
        }
        assert!(link.service(&mut native, &mut owner, true).is_err());
        assert!(owner.is_none());
        assert!(link.needs_turn());
        link.service(&mut native, &mut owner, true).unwrap();
        assert_eq!(native.closes, 1);
        assert!(!d.epoch.is_valid());
    }
    let mut link = Link::default();
    let mut owner = None;
    let mut native = Native::default();
    link.demand(OutputId::from_raw(1), &owner).unwrap();
    link.service(&mut native, &mut owner, false).unwrap();
    assert!(native.requests.is_empty());
    assert!(!link.needs_turn());
}

struct Connections {
    inner: ShellComponentConnections,
    directory: std::path::PathBuf,
}
impl Connections {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "capture-link-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let mut inner = ShellComponentConnections::new().unwrap();
        inner
            .add(
                "bar",
                ShellComponentRole::Bar,
                &directory,
                rustix::process::getuid().as_raw(),
            )
            .unwrap();
        let grant = sophia_protocol::ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        };
        inner.connections[0].attempt = Some((grant, ComponentConnectionPhase::Connected));
        inner.epochs = fixture::allocation_registry(grant);
        Self { inner, directory }
    }
    fn key(&self) -> ComponentConnectionKey {
        ComponentConnectionKey {
            slot: 0,
            grant: self.inner.connections[0].attempt.unwrap().0,
        }
    }
}
impl Drop for Connections {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn request(d: &Destination) -> LiveShellCaptureRequest {
    let (fd, _) = UnixStream::pair().unwrap();
    LiveShellCaptureRequest {
        source: LiveShellCaptureSource {
            fd: fd.into(),
            width: 4,
            height: 4,
            stride: 16,
            modifier: 0,
            offset: 0,
        },
        image_id: sophia_renderer_live::LiveRendererImageId::from_raw(1),
        reservation: d.budget.reserve_capture(64).unwrap(),
        budget: d.budget.clone(),
        epoch: d.epoch.clone(),
    }
}

#[test]
fn reserved_target_and_exact_destination_gate_submission_and_retained_custody_blocks_replacement() {
    let mut f = Connections::new();
    let key = f.key();
    let metadata = fixture::metadata(key.grant);
    let ticket = f
        .inner
        .epochs
        .reserve_capture_image(key.grant, &metadata, 0)
        .unwrap();
    let wrong_key = ComponentConnectionKey { slot: 1, ..key };
    assert!(
        f.inner
            .prepare_capture_endpoint(wrong_key, &ticket, 0)
            .is_err()
    );
    assert!(!f.inner.capture_link.needs_turn());
    let mut foreign = fixture::allocation_registry(key.grant);
    let foreign_ticket = foreign
        .reserve_capture_image(key.grant, &metadata, 0)
        .unwrap();
    assert!(
        f.inner
            .prepare_capture_endpoint(key, &foreign_ticket, 0)
            .is_err(),
        "same fields are not the same reservation"
    );
    assert!(!f.inner.capture_link.needs_turn());
    assert!(
        f.inner
            .prepare_capture_endpoint(key, &ticket, 0)
            .unwrap()
            .is_none()
    );
    let mut native = Native::default();
    let destination = native.offer(1);
    f.inner
        .capture_link
        .service(&mut native, &mut f.inner.capture, true)
        .unwrap();
    let binding = Binding {
        slot: key.slot,
        grant: key.grant,
        metadata,
        plane: sophia_protocol::shell_files::ShellImagePlane {
            offset: 0,
            stride: 16,
            fd_index: 0,
        },
    };
    let mut r = request(&destination);
    let fd = r.source.fd.as_raw_fd();
    r.epoch = LiveRendererSnapshotEpoch::new(1, 2, 3);
    let rejected = f
        .inner
        .submit_capture(
            binding,
            r,
            ticket,
            std::time::Instant::now() + Duration::from_secs(2),
            0,
        )
        .unwrap_err();
    assert_eq!(rejected.request.source.fd.as_raw_fd(), fd);
    assert_eq!(f.inner.capture.as_ref().unwrap().pending(), 0);
    let mut r = rejected.request;
    r.epoch = destination.epoch.clone();
    r.budget = LiveRendererSnapshotBudget::new(4, 256);
    let rejected = f
        .inner
        .submit_capture(
            binding,
            r,
            rejected.reservation,
            std::time::Instant::now() + Duration::from_secs(2),
            0,
        )
        .unwrap_err();
    assert_eq!(rejected.request.source.fd.as_raw_fd(), fd);
    let mut r = rejected.request;
    r.budget = destination.budget.clone();
    f.inner
        .submit_capture(
            binding,
            r,
            rejected.reservation,
            std::time::Instant::now() + Duration::from_secs(2),
            0,
        )
        .unwrap();
    destination.epoch.invalidate();
    f.inner
        .capture_link
        .service(&mut native, &mut f.inner.capture, true)
        .unwrap();
    assert!(!f.inner.capture.as_ref().unwrap().replaceable());
    assert!(
        f.inner
            .capture_link
            .demand(OutputId::from_raw(1), &f.inner.capture)
            .is_err()
    );
    let (other, _executor) =
        sophia_renderer_live::live_shell_capture_channel(LiveRendererSnapshotEpoch::new(1, 3, 3))
            .unwrap();
    assert!(f.inner.install_capture_owner(other).is_err());
    // The exact journal is now disconnected; only proven pre-use rollback may
    // release custody. Receipt drain must still run after endpoint closure.
    f.inner.connections[0].attempt = None;
    assert!(f.inner.service_capture_completions().is_empty());
    assert_eq!(destination.budget.usage(), (0, 0));
    assert!(f.inner.capture.as_ref().unwrap().replaceable());
    assert!(
        !f.inner
            .capture_link
            .demand(OutputId::from_raw(1), &f.inner.capture)
            .unwrap()
    );
}

#[test]
fn source_publication_routes_exact_slot_and_grant_across_reconnect_and_pressure() {
    let mut f = Connections::new();
    let key = f.key();
    let second_dir = f.directory.join("dock");
    f.inner
        .add(
            "dock",
            ShellComponentRole::Dock,
            &second_dir,
            rustix::process::getuid().as_raw(),
        )
        .unwrap();
    let other_grant = sophia_protocol::ContentGrant {
        connection_epoch: 2,
        content_grant_epoch: 2,
    };
    f.inner.connections[1].attempt = Some((other_grant, ComponentConnectionPhase::Connected));
    let binding = |slot, grant| Binding {
        slot,
        grant,
        metadata: fixture::metadata(grant),
        plane: sophia_protocol::shell_files::ShellImagePlane {
            offset: 0,
            stride: 16,
            fd_index: 0,
        },
    };
    let old = binding(0, key.grant);
    let other = binding(1, other_grant);
    for b in [other, old, other] {
        assert_eq!(
            super::super::publish_current(&mut f.inner.connections, b, |c| {
                assert_eq!(c.id, if b.slot == 0 { "bar" } else { "dock" });
                Ok(())
            })
            .unwrap(),
            Publication::Journaled
        );
    }
    let successor = sophia_protocol::ContentGrant {
        connection_epoch: 3,
        content_grant_epoch: 3,
    };
    f.inner.connections[0].attempt = Some((successor, ComponentConnectionPhase::Connected));
    assert_eq!(
        super::super::publish_current(&mut f.inner.connections, old, |_| panic!(
            "old journal must not alias successor"
        ))
        .unwrap(),
        Publication::Disconnected
    );
    assert_eq!(
        super::super::publish_current(&mut f.inner.connections, binding(0, successor), |_| Err(
            ShellTransportError::ContentQueueSaturated
        )),
        Err(ShellTransportError::ContentQueueSaturated)
    );
    assert_eq!(
        super::super::publish_current(&mut f.inner.connections, other, |c| {
            assert_eq!(c.id, "dock");
            Ok(())
        })
        .unwrap(),
        Publication::Journaled
    );
}

#[test]
fn disconnected_requester_cannot_install_pending_reply_into_successor() {
    let mut f = Connections::new();
    let key = f.key();
    let ticket = f
        .inner
        .epochs
        .reserve_capture_image(key.grant, &fixture::metadata(key.grant), 0)
        .unwrap();
    f.inner.prepare_capture_endpoint(key, &ticket, 0).unwrap();
    let mut native = Native::default();
    f.inner
        .capture_link
        .service(&mut native, &mut f.inner.capture, true)
        .unwrap();
    let destination = native.offer(1);
    let successor = sophia_protocol::ContentGrant {
        connection_epoch: 2,
        content_grant_epoch: 2,
    };
    f.inner.connections[0].attempt = Some((successor, ComponentConnectionPhase::Connected));
    f.inner
        .capture_link
        .observe_requester(&f.inner.connections, &mut f.inner.capture);
    f.inner
        .capture_link
        .service(&mut native, &mut f.inner.capture, true)
        .unwrap();
    assert!(f.inner.capture.is_none());
    assert!(!destination.epoch.is_valid());
    assert_eq!(native.closes, 1);
    assert!(!f.inner.capture_link.needs_turn());
}
