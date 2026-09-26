//! Private owner tests, with real kernel credentials for export/worker controls.
//! The store lock and near-exhaustion counters are fixture inputs, not new APIs.
#![cfg(test)]
use super::export::{InspectionExport, Node};
use super::publication::Shared;
use super::*;
use sophia_9p::{Export, OpenFlags, ReadOutcome};
use std::io::{Read, Write};
use std::time::Duration;

#[path = "inspection_wire.rs"]
mod wire;

fn value() -> InspectionSnapshot {
    InspectionSnapshot {
        session_generation: 2,
        wm_epoch: 9,
        scene_generation: 4,
        selected_capabilities: 0,
        wire: InspectionWire::Files,
        state: InspectionState::Ready,
        outputs: vec![],
        surfaces: vec![],
    }
}
fn setup() -> (Arc<Shared>, InspectionPublisher, UnixStream) {
    let (tx, rx) = UnixStream::pair().unwrap();
    tx.set_nonblocking(true).unwrap();
    let shared = Arc::new(Shared::new(tx));
    shared.fence(9, None).unwrap();
    let publisher = InspectionPublisher {
        shared: shared.clone(),
    };
    publisher.publish(value(), None).unwrap();
    (shared, publisher, rx)
}
fn export(shared: Arc<Shared>, stream: &UnixStream) -> InspectionExport {
    let domain = Arc::new(HostDomain::new().unwrap());
    let peer = domain.admit(stream, &[]).unwrap();
    InspectionExport::new(shared, domain, peer).unwrap()
}

#[test]
fn publication_lock_contention_invalidates_watch_and_unchanged_state_resynchronizes() {
    let (shared, publisher, _wake) = setup();
    let (server, _client) = UnixStream::pair().unwrap();
    let mut owner = export(shared.clone(), &server);
    let mut snapshot = owner.open(&Node::Snapshot, OpenFlags(0)).unwrap();
    let first = decode_inspection_snapshot(&match owner
        .read(&Node::Snapshot, &mut snapshot, 0, 4096)
        .unwrap()
    {
        ReadOutcome::Ready(b) => b,
        _ => panic!(),
    })
    .unwrap();
    let mut watch = owner.open(&Node::Events, OpenFlags(0)).unwrap();
    let locked = shared.writer.lock().unwrap();
    assert_eq!(
        publisher
            .publish(value(), Some(InspectionEvent::ConfigurationChanged))
            .unwrap(),
        PublishOutcome::Busy { loss_generation: 1 }
    );
    assert_eq!(
        owner.read(&Node::Events, &mut watch, first.event_offset, 4096),
        Err(sophia_9p::Errno::ESTALE)
    );
    drop(locked);
    assert!(matches!(
        publisher.publish(value(), None).unwrap(),
        PublishOutcome::Published { sequence: 2 }
    ));
    assert_eq!(
        owner.read(&Node::Events, &mut watch, first.event_offset, 4096),
        Err(sophia_9p::Errno::ESTALE)
    );
    owner.release(Node::Snapshot, Some(snapshot));
    let mut fresh = owner.open(&Node::Snapshot, OpenFlags(0)).unwrap();
    let next = decode_inspection_snapshot(&match owner
        .read(&Node::Snapshot, &mut fresh, 0, 4096)
        .unwrap()
    {
        ReadOutcome::Ready(b) => b,
        _ => panic!(),
    })
    .unwrap();
    assert_eq!((next.loss_generation, next.sequence), (1, 2));
    assert!(next.event_offset > first.event_offset);
}

#[test]
fn qid_and_sequence_exhaustion_refuse_before_snapshot_or_cursor_mutation() {
    for qids in [true, false] {
        let (shared, publisher, _wake) = setup();
        let (old, tail) = {
            let mut view = shared.view.lock().unwrap();
            let store = Arc::make_mut(&mut *view);
            if qids {
                shared.next_qid.store(u64::MAX, Ordering::SeqCst);
            } else {
                store.sequence = u64::MAX;
            }
            (store.snapshot.as_ref().unwrap().clone(), store.tail)
        };
        assert_eq!(
            publisher.publish(value(), Some(InspectionEvent::ProjectionCommitted)),
            Err(InspectionError::Exhausted)
        );
        let store = shared.view().unwrap();
        assert!(Arc::ptr_eq(&old, store.snapshot.as_ref().unwrap()));
        assert_eq!(store.tail, tail);
        assert_eq!(shared.loss.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn fixed_ring_bounds_and_snapshot_cursor_cover_one_atomic_publication() {
    let (shared, publisher, _wake) = setup();
    for _ in 0..100 {
        publisher
            .publish(value(), Some(InspectionEvent::ProjectionCommitted))
            .unwrap();
    }
    let store = shared.view().unwrap();
    assert_eq!(store.events.len(), INSPECTION_MAX_EVENTS);
    assert!(store.ring_bytes <= INSPECTION_MAX_RING_BYTES);
    let snapshot = &store.snapshot.as_ref().unwrap().record;
    let event = decode_inspection_event(&store.events.back().unwrap().bytes).unwrap();
    assert_eq!(snapshot.event_offset, store.tail);
    assert_eq!(
        (
            snapshot.generation,
            snapshot.sequence,
            snapshot.loss_generation
        ),
        (event.generation, event.sequence, event.loss_generation)
    );
    assert!(store.floor() > 0);
}

#[test]
fn reader_work_after_arc_clone_does_not_contend_with_publication() {
    let (shared, publisher, _wake) = setup();
    let (cloned_tx, cloned_rx) = std::sync::mpsc::sync_channel(1);
    let (done_tx, done_rx) = std::sync::mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        let reader_shared = shared.clone();
        let reader = scope.spawn(move || {
            let view = reader_shared.view().unwrap();
            cloned_tx.send(()).unwrap();
            // This is the same immutable view used by status formatting and
            // event slicing. Deliberately retain it until publication finishes.
            let snapshot = &view.snapshot.as_ref().unwrap().record;
            for _ in 0..100 {
                assert!(!encode_inspection_snapshot(snapshot).unwrap().is_empty());
            }
            done_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            assert_eq!(view.sequence, 1);
        });
        cloned_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        assert_eq!(
            publisher
                .publish(value(), Some(InspectionEvent::ProjectionCommitted))
                .unwrap(),
            PublishOutcome::Published { sequence: 2 }
        );
        assert_eq!(shared.loss.load(Ordering::SeqCst), 0);
        done_tx.send(()).unwrap();
        reader.join().unwrap();
    });
}

#[test]
fn bounded_concurrent_reader_and_publisher_make_progress_with_explicit_loss() {
    let (shared, publisher, _wake) = setup();
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            for _ in 0..1000 {
                let view = shared.view().unwrap();
                let snapshot = view.snapshot.as_ref().unwrap();
                assert_eq!(
                    decode_inspection_snapshot(&snapshot.bytes).unwrap(),
                    snapshot.record
                );
                std::thread::yield_now();
            }
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let mut published = 0;
        while published < 100 {
            assert!(std::time::Instant::now() < deadline);
            match publisher
                .publish(value(), Some(InspectionEvent::SnapshotChanged))
                .unwrap()
            {
                PublishOutcome::Published { .. } => published += 1,
                PublishOutcome::Busy { loss_generation } => assert!(loss_generation > 0),
                PublishOutcome::Unchanged => panic!("an explicit event is never coalesced"),
            }
            std::thread::yield_now();
        }
        reader.join().unwrap();
    });
}

#[test]
fn delayed_old_generation_cannot_overwrite_new_publication_at_handoff() {
    let (shared, publisher, _wake) = setup();
    let old = shared.view().unwrap();
    let delayed = Arc::new(old.as_ref().clone());
    shared.fence(10, None).unwrap();
    let mut fresh = value();
    fresh.wm_epoch = 10;
    publisher.publish(fresh, None).unwrap();
    let current = shared.view().unwrap();
    assert_eq!(
        publisher.handoff(&old, delayed, old.generation, 0),
        Err(InspectionError::Fenced)
    );
    assert!(Arc::ptr_eq(&current, &shared.view().unwrap()));
    assert_eq!(
        current.snapshot.as_ref().unwrap().record.snapshot.wm_epoch,
        10
    );
}

#[test]
fn worker_revocation_discards_unsent_reply_before_another_core_turn() {
    let (shared, publisher, _wake) = setup();
    let mut large = value();
    large.surfaces = (0..1024)
        .map(|index| InspectionSurface {
            id: InspectionSurfaceId {
                index,
                generation: 1,
            },
            state_generation: 2,
            output: None,
            geometry: InspectionRect {
                x: 1,
                y: 2,
                width: 100,
                height: 100,
            },
        })
        .collect();
    publisher.publish(large, None).unwrap();
    let (socket, mut client) = UnixStream::pair().unwrap();
    rustix::net::sockopt::set_socket_send_buffer_size(&socket, 1024).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let owner = export(shared.clone(), &socket);
    let limits = sophia_9p::Limits::new(65536, 512, 8, 16, 131072, 1).unwrap();
    let mut server = sophia_9p::unix::Server::new(owner, limits).unwrap();
    server.adopt(socket).map_err(|e| e.error).unwrap();
    let mut peers = vec![server];
    for request in [
        wire::version(),
        wire::attach(),
        wire::walk(2, 1, b"snapshot"),
        wire::open(3, 1, 0),
    ] {
        client.write_all(&request).unwrap();
        super::worker::service_peers(&mut peers);
        assert_ne!(wire::receive(&mut client).unwrap()[4], 7);
    }
    client.write_all(&wire::read(4, 1, 0, 65525)).unwrap();
    super::worker::service_peers(&mut peers);
    assert!(
        matches!(
            publisher
                .publish(value(), Some(InspectionEvent::ProjectionCommitted))
                .unwrap(),
            PublishOutcome::Published { .. }
        ),
        "unread observer output cannot hold publication credit"
    );
    shared.fence(10, None).unwrap();
    super::worker::service_peers(&mut peers);
    assert!(
        peers.is_empty(),
        "production worker removed the revoked core before resuming it"
    );
    let mut delivered = Vec::new();
    client.read_to_end(&mut delivered).unwrap();
    assert!(
        delivered.len() >= 7,
        "a prefix was actually delivered before fencing"
    );
    assert_eq!(delivered[4], 117);
    let reply_size = u32::from_le_bytes(delivered[..4].try_into().unwrap()) as usize;
    assert_eq!(reply_size, 65536);
    assert!(
        delivered.len() < reply_size,
        "the remaining unsent response was discarded, not recalled"
    );
}
