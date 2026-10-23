//! Supplied fds exercise custody only; no descriptor is imported by a GPU.
use super::*;
use crate::shell_component_connections::capture::source::Source;
use sophia_protocol::shell_files::{ShellImageMetadata, ShellImagePlane, ShellSourceDisposition};
use std::{
    io::Read,
    time::{Duration, Instant},
};

fn source(f: &mut Connections, id: u64, deadline: Instant) -> (Source, UnixStream) {
    let key = f.key();
    let mut metadata = fixture::metadata(key.grant);
    metadata.offer_id = id;
    metadata.resource = id;
    let reservation = f
        .inner
        .epochs
        .reserve_capture_image(key.grant, &metadata, 0)
        .unwrap();
    let (fd, peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    (
        Source {
            binding: Binding {
                slot: key.slot,
                grant: key.grant,
                metadata,
                plane: ShellImagePlane {
                    fd_index: 0,
                    offset: 0,
                    stride: 16,
                },
            },
            source: LiveShellCaptureSource {
                fd: fd.into(),
                width: 4,
                height: 4,
                stride: 16,
                modifier: 0,
                offset: 0,
            },
            reservation,
            deadline,
        },
        peer,
    )
}
fn alive(peer: &mut UnixStream) {
    assert_eq!(
        peer.read(&mut [0]).unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}
fn gone(peer: &mut UnixStream) {
    // Parallel process-launch fixtures can briefly inherit a CLOEXEC socket
    // between fork and exec. Bound the EOF proof rather than assuming those
    // transient kernel references have disappeared in this exact instant.
    peer.set_nonblocking(false).unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

#[test]
fn pending_endpoint_keeps_source_and_credit_then_timeout_keeps_journal_custody() {
    let mut f = Connections::new();
    let now = Instant::now();
    let (source, mut peer) = source(&mut f, 1, now + Duration::from_secs(1));
    let binding = source.binding;
    assert!(f.inner.retain_capture_source(source, 0).is_ok());
    assert_eq!(
        f.inner
            .epochs
            .resources(binding.grant)
            .unwrap()
            .usage()
            .staging,
        64
    );
    assert_eq!(f.inner.capture_pending(), 1);
    assert!(f.inner.capture_maintenance_wait() <= Duration::from_millis(10));
    assert!(
        f.inner
            .service_capture_sources_with(0, now, |_, _, _, _| panic!("unsettled source"))
            .is_empty()
    );
    alive(&mut peer);
    assert_eq!(
        f.inner.submit_retained_capture_source(
            binding,
            sophia_renderer_live::LiveRendererImageId::from_raw(1),
            64,
            0,
            now
        ),
        Err(ContentStoreError::Incomplete)
    );
    let attempt = f.inner.connections[0].attempt.take();
    assert!(f.inner.finish_after_backend_drop(()).is_err());
    f.inner.connections[0].attempt = attempt;
    let mut attempts = 0;
    let errors =
        f.inner
            .service_capture_sources_with(1, now + Duration::from_secs(2), |_, _, b, d| {
                assert_eq!(b, binding);
                assert_eq!(
                    d,
                    sophia_protocol::shell_files::ShellSourceDisposition::RefusedBeforeUse
                );
                attempts += 1;
                Err(ShellTransportError::ContentQueueSaturated)
            });
    assert_eq!(
        (errors.len(), attempts, f.inner.capture_pending()),
        (1, 1, 1)
    );
    assert_eq!(
        f.inner
            .epochs
            .resources(binding.grant)
            .unwrap()
            .usage()
            .staging,
        0,
        "logical failure must settle before source-release publication"
    );
    alive(&mut peer);
    // Already-settled ticket is not failed twice, and a publication error did
    // not drop the descriptor. Only the next successful journal append does.
    assert!(
        f.inner
            .service_capture_sources_with(2, now + Duration::from_secs(3), |_, _, _, _| Ok(
                Publication::Journaled
            ))
            .is_empty()
    );
    assert_eq!(f.inner.capture_pending(), 0);
    gone(&mut peer);
}

#[test]
fn disconnected_source_reconciles_old_ticket_without_publishing_to_successor() {
    let mut f = Connections::new();
    let now = Instant::now();
    let (source, mut peer) = source(&mut f, 1, now + Duration::from_secs(5));
    assert!(f.inner.retain_capture_source(source, 0).is_ok());
    f.inner.connections[0].attempt = Some((
        sophia_protocol::ContentGrant {
            connection_epoch: 2,
            content_grant_epoch: 2,
        },
        ComponentConnectionPhase::Connected,
    ));
    let errors = f
        .inner
        .service_capture_sources_with(0, now, |connections, _, binding, _| {
            publish_current(connections, binding, |_| {
                panic!("old source reached successor")
            })
        });
    assert!(errors.is_empty());
    assert_eq!(f.inner.capture_pending(), 0);
    gone(&mut peer);
}

#[test]
fn shared_four_slot_bound_covers_waiting_submitted_and_returned_sources() {
    let mut f = Connections::new();
    let now = Instant::now();
    let mut bindings = Vec::new();
    let mut peers = Vec::new();
    for id in 1..=4 {
        let (s, peer) = source(&mut f, id, now + Duration::from_secs(10));
        bindings.push(s.binding);
        peers.push(peer);
        assert!(f.inner.retain_capture_source(s, 0).is_ok());
    }
    // Logical stores cap open transfers too. A separately supplied ticket
    // isolates the Session aggregate-capacity refusal before ticket lookup.
    let mut extra_f = Connections::new();
    let (extra, mut extra_peer) = source(&mut extra_f, 5, now + Duration::from_secs(10));
    let fd = extra.source.fd.as_raw_fd();
    let (error, extra) = f.inner.retain_capture_source(extra, 0).err().unwrap();
    assert_eq!(error, ContentStoreError::Budget);
    assert_eq!(extra.source.fd.as_raw_fd(), fd);
    alive(&mut extra_peer);
    let mut native = Native::default();
    native.offer(1);
    f.inner
        .capture_link
        .service(&mut native, &mut f.inner.capture, true)
        .unwrap();
    f.inner
        .submit_retained_capture_source(
            bindings[0],
            sophia_renderer_live::LiveRendererImageId::from_raw(1),
            64,
            0,
            now,
        )
        .unwrap();
    assert_eq!(f.inner.capture_pending(), 4);
    let (error, extra) = f.inner.retain_capture_source(extra, 0).err().unwrap();
    assert_eq!(error, ContentStoreError::Budget);
    // Direct submission cannot bypass waiting-source slots either.
    let d = f.inner.capture_link.destination.as_ref().unwrap().clone();
    let rejected = f
        .inner
        .submit_capture(
            extra.binding,
            request(&d),
            extra.reservation,
            extra.deadline,
            0,
        )
        .err()
        .unwrap();
    assert_eq!(
        rejected.reason,
        crate::shell_capture_owner::SubmitFailure::Capacity
    );
    // Explicitly reconcile the refused fifth ticket; it never entered custody.
    extra_f
        .inner
        .epochs
        .fail_capture(
            &mut Some(rejected.reservation),
            sophia_runtime::ContentCaptureFailure::Cancelled,
            0,
        )
        .unwrap();
    drop(rejected.request);
    drop(extra.source);
    native.executor.take();
    // A real pre-use return is still a held Owner settlement, sharing the bound.
    assert_eq!(f.inner.capture_pending(), 4);
    for peer in &mut peers {
        alive(peer);
    }
}

#[test]
fn physical_budget_failure_and_expired_submission_preserve_waiting_source() {
    let mut f = Connections::new();
    let now = Instant::now();
    let (s, mut peer) = source(&mut f, 1, now + Duration::from_secs(1));
    let binding = s.binding;
    assert!(f.inner.retain_capture_source(s, 0).is_ok());
    let mut native = Native::default();
    let d = native.offer(1);
    f.inner
        .capture_link
        .service(&mut native, &mut f.inner.capture, true)
        .unwrap();
    assert_eq!(
        f.inner.submit_retained_capture_source(
            binding,
            sophia_renderer_live::LiveRendererImageId::from_raw(1),
            257,
            0,
            now
        ),
        Err(ContentStoreError::Budget)
    );
    assert_eq!(f.inner.capture_pending(), 1);
    alive(&mut peer);
    assert_eq!(
        f.inner.submit_retained_capture_source(
            binding,
            sophia_renderer_live::LiveRendererImageId::from_raw(1),
            64,
            0,
            now + Duration::from_secs(2)
        ),
        Err(ContentStoreError::Revoked)
    );
    assert!(
        d.budget.reserve_capture(256).is_ok(),
        "no physical capacity leaked"
    );
    alive(&mut peer);
}

#[test]
fn malformed_and_foreign_tickets_return_exact_custody_without_endpoint_demand() {
    let mut f = Connections::new();
    let now = Instant::now();
    let (mut s, mut peer) = source(&mut f, 1, now + Duration::from_secs(1));
    let fd = s.source.fd.as_raw_fd();
    s.source.stride = 32;
    let (error, mut s) = f.inner.retain_capture_source(s, 0).err().unwrap();
    assert_eq!(error, ContentStoreError::Stale);
    assert_eq!(s.source.fd.as_raw_fd(), fd);
    assert!(!f.inner.capture_link.needs_turn());
    alive(&mut peer);
    s.source.stride = 16;
    let mut other = Connections::new();
    let (error, s) = other.inner.retain_capture_source(s, 0).err().unwrap();
    assert_eq!(error, ContentStoreError::Stale);
    assert_eq!(s.source.fd.as_raw_fd(), fd);
    assert!(!other.inner.capture_link.needs_turn());
    assert!(f.inner.retain_capture_source(s, 0).is_ok());
    alive(&mut peer);
}

#[test]
fn clock_failure_retains_source_without_publication_and_retry_settles_once() {
    let mut f = Connections::new();
    let now = Instant::now();
    let (s, mut peer) = source(&mut f, 1, now + Duration::from_secs(1));
    assert!(f.inner.retain_capture_source(s, 10).is_ok());
    let errors = f
        .inner
        .service_capture_sources_with(9, now, |_, _, _, _| panic!("regressed clock"));
    assert_eq!(errors.len(), 1);
    assert_eq!(f.inner.capture_pending(), 1);
    alive(&mut peer);
    assert!(
        f.inner
            .service_capture_sources_with(10, now + Duration::from_secs(2), |_, _, _, _| Ok(
                Publication::Journaled
            ))
            .is_empty()
    );
    assert_eq!(f.inner.capture_pending(), 0);
    gone(&mut peer);
}

#[test]
fn failed_enqueue_restores_source_and_releases_only_unused_physical_credit() {
    let mut f = Connections::new();
    let now = Instant::now();
    let (s, mut peer) = source(&mut f, 1, now + Duration::from_secs(5));
    let binding = s.binding;
    assert!(f.inner.retain_capture_source(s, 0).is_ok());
    let mut native = Native::default();
    let d = Destination {
        output: OutputId::from_raw(1),
        budget: LiveRendererSnapshotBudget::new(8, 512),
        epoch: LiveRendererSnapshotEpoch::new(1, 2, 3),
        facts: Facts::default(),
    };
    let (mut inbox, executor) = LiveShellCaptureCustody::default()
        .channel(d.epoch.clone(), d.budget.clone())
        .unwrap();
    // Supplied queue pressure at the actual native inbox; no job is executed.
    // Physical capacity remains available, isolating the failed-enqueue path.
    for _ in 0..4 {
        assert!(
            inbox
                .submit(request(&d), now + Duration::from_secs(5))
                .is_ok()
        );
    }
    native.executor = Some(executor);
    native.reply = Some(Endpoint {
        inbox,
        destination: d.clone(),
    });
    f.inner
        .capture_link
        .service(&mut native, &mut f.inner.capture, true)
        .unwrap();
    // Submission returns its exact request before any GPU access.
    assert_eq!(
        f.inner.submit_retained_capture_source(
            binding,
            sophia_renderer_live::LiveRendererImageId::from_raw(1),
            64,
            0,
            now
        ),
        Err(ContentStoreError::Incomplete)
    );
    assert_eq!(f.inner.capture_pending(), 1);
    assert!(d.budget.reserve_capture(256).is_ok());
    alive(&mut peer);
    assert!(
        f.inner
            .service_capture_sources_with(0, now, |_, _, _, _| Ok(Publication::Journaled))
            .is_empty()
    );
    gone(&mut peer);
}

#[test]
fn shutdown_retains_waiting_source_after_all_grants_retire() {
    let mut f = Connections::new();
    let now = Instant::now();
    let (s, mut peer) = source(&mut f, 1, now + Duration::from_secs(5));
    let grant = s.binding.grant;
    assert!(f.inner.retain_capture_source(s, 0).is_ok());
    f.inner.connections[0].attempt = None;
    assert!(f.inner.epochs.disconnect(grant));
    // Neither an active connection nor an active registry grant masks this
    // guard: the waiting descriptor alone must prevent backend disposal.
    assert!(f.inner.finish_after_backend_drop(()).is_err());
    alive(&mut peer);
    assert!(
        f.inner
            .service_capture_sources_with(0, now, |_, _, _, _| Ok(Publication::Disconnected))
            .is_empty()
    );
    assert_eq!(f.inner.capture_pending(), 0);
    gone(&mut peer);
    assert!(f.inner.finish_after_backend_drop(()).is_ok());
}

fn admit(f: &mut Connections, d: &Destination, deadline: Instant) -> ShellImageMetadata {
    let key = f.key();
    let metadata = fixture::metadata(key.grant);
    let reservation = f
        .inner
        .epochs
        .reserve_capture_image(key.grant, &metadata, 0)
        .unwrap();
    f.inner.hold_capture_admission(
        key.slot,
        key.grant,
        sophia_runtime::ShellCaptureAdmission {
            metadata,
            reservation,
            deadline,
        },
        d.budget.reserve_capture(64).unwrap(),
    );
    metadata
}

#[test]
fn admitted_offer_keeps_its_ticket_until_the_source_then_submits_with_it() {
    let mut f = Connections::new();
    let now = Instant::now();
    let mut native = Native::default();
    let d = native.offer(1);
    f.inner
        .capture_link
        .demand(OutputId::from_raw(1), &f.inner.capture)
        .unwrap();
    f.inner
        .capture_link
        .service(&mut native, &mut f.inner.capture, true)
        .unwrap();
    let metadata = admit(&mut f, &d, now + Duration::from_secs(10));
    assert_eq!(f.inner.capture_pending(), 1, "admission holds a job slot");
    assert_eq!(d.budget.usage(), (1, 64));
    f.inner.submit_received_captures(0, now);
    assert_eq!(
        f.inner.capture.as_ref().unwrap().pending(),
        0,
        "no Source yet"
    );
    let key = f.key();
    let (fd, mut peer) = UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    f.inner.receive_capture_source(
        key.slot,
        key.grant,
        sophia_runtime::ShellCaptureImageSource {
            metadata,
            plane: ShellImagePlane {
                offset: 0,
                stride: 16,
                fd_index: 0,
            },
            source_bytes: 64,
            fd: fd.into(),
        },
    );
    f.inner.submit_received_captures(0, now);
    assert_eq!(f.inner.capture_sources.pending(), 0);
    assert_eq!(f.inner.capture.as_ref().unwrap().pending(), 1);
    assert_eq!(f.inner.capture_pending(), 1);
    assert_eq!(
        d.budget.usage(),
        (1, 64),
        "the pre-admission ticket moved; nothing was charged twice"
    );
    alive(&mut peer);
}

#[test]
fn admission_without_source_settles_and_releases_its_ticket_at_the_deadline() {
    let mut f = Connections::new();
    let now = Instant::now();
    let mut native = Native::default();
    let d = native.offer(1);
    let metadata = admit(&mut f, &d, now);
    let mut released = Vec::new();
    assert!(
        f.inner
            .service_capture_sources_with(0, now, |_, _, binding, disposition| {
                released.push((binding.metadata, disposition));
                Ok(crate::shell_capture_owner::Publication::Journaled)
            })
            .is_empty()
    );
    assert_eq!(
        released,
        [(metadata, ShellSourceDisposition::RefusedBeforeUse)]
    );
    assert_eq!(f.inner.capture_pending(), 0);
    assert_eq!(d.budget.usage(), (0, 0), "unused physical credit returns");
}
