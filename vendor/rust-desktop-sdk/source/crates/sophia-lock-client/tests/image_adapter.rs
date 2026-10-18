#[path = "support/image_peer.rs"]
mod peer;
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits};
use sophia_image_producer::{
    ImagePath, OfferState, ProducerMetadata, ResourceStatusKind, create_isb, finish_isb,
};
use sophia_lock_client::{
    LockImageConfig, Session, journal::Event, protocol::lock_files::*, startup::Opening,
};
use std::{
    thread,
    time::{Duration, Instant},
};
fn run(absent: bool, cancel: bool) {
    let peer = peer::start(absent, cancel);
    let start = Instant::now();
    let wire = Pipeline::over(
        peer.stream,
        PipelineLimits {
            max_outstanding: 16,
            ..PipelineLimits::default()
        },
        Duration::from_secs(2),
    )
    .unwrap();
    let mut opening = Opening::direct(
        wire,
        LockNegotiate {
            minimum_revision: 1,
            maximum_revision: 2,
            requested_capabilities: 5,
            chords: vec![],
        },
        0,
        3000,
    )
    .unwrap();
    while !opening.is_opened() {
        assert!(start.elapsed() < Duration::from_secs(3));
        opening
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
        thread::yield_now();
    }
    let mut session = Session::from_opened_with_image(
        opening.into_opened().unwrap(),
        LockImageConfig {
            endpoint_dir: peer.dir.clone(),
            startup_deadline_ms: 3000,
            transfer_timeout_ms: 1000,
            formats_capacity: 163904,
        },
    )
    .unwrap();
    let mut offer = None;
    let mut status = false;
    let mut cancelled = false;
    loop {
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "image path {:?}, obligations {:?}",
            session.image().unwrap().path(),
            session.obligations()
        );
        let now = start.elapsed().as_millis() as u64;
        session.dispatch(now, 65536).unwrap();
        if let Some((_, event)) = session.event() {
            assert!(matches!(event, Event::ResourceStatus(_)));
            status = true;
            session.consume().unwrap();
        }
        let image = session.image().unwrap();
        if absent {
            if image.path() == ImagePath::Error {
                break;
            }
        } else if let Some(id) = offer {
            if let Some(info) = image.producer().offer(id)
                && info.state == OfferState::Settled
            {
                assert_eq!(
                    info.status,
                    Some(if cancel {
                        ResourceStatusKind::Cancelled
                    } else {
                        ResourceStatusKind::Accepted
                    })
                );
                assert_eq!(info.disposition, if cancel { 4 } else { 1 });
                assert!(status);
                session.image_forget(id).unwrap();
                break;
            }
            if cancel && !cancelled {
                match session.image_cancel(id, now) {
                    Ok(_) => cancelled = true,
                    Err(e) => assert_eq!(e, sophia_lock_client::ImageError::Busy),
                }
            }
        } else if image.path() == ImagePath::Descriptor {
            let source = create_isb(2, 2).unwrap();
            source.write_pixels(0, &[255; 16]).unwrap();
            finish_isb(&source).unwrap();
            let result = session.image_begin(
                ProducerMetadata {
                    resource: 21,
                    resource_generation: 1,
                    admission_epoch: 1,
                    output_id: 1,
                    allocation_id: 1,
                    topology_generation: 1,
                    source_kind: 1,
                    source_format: 1,
                    width: 2,
                    height: 2,
                    stride: 8,
                    plane_count: 1,
                    descriptor_count: 1,
                    declared_bytes: 16,
                    ..ProducerMetadata::default()
                },
                source.into_fd(),
                now,
            );
            match result {
                Ok(admission) => offer = Some(admission.offer),
                Err(e) => assert_eq!(e.error, sophia_lock_client::ImageError::Busy),
            }
        }
        thread::yield_now();
    }
    assert_eq!(session.selected_revision(), Some(2));
    session.close();
    drop(session);
    assert_eq!(peer.thread.join().unwrap(), usize::from(!absent && !cancel));
    std::fs::remove_dir_all(peer.dir).unwrap();
}
#[test]
fn sealed_source_crosses_after_admission_and_status_precedes_release() {
    run(false, false);
}
#[test]
fn absent_discovery_preserves_clean_caller_fallback() {
    run(true, false);
}

#[test]
fn cancel_before_admission_suppresses_the_late_admitted_source() {
    run(false, true);
}

fn failure_keeps_session(case: peer::Case, cause: sophia_image_producer::CloseCause) {
    let peer = peer::start_case(case, false);
    let start = Instant::now();
    let wire = Pipeline::over(
        peer.stream,
        PipelineLimits::default(),
        Duration::from_secs(2),
    )
    .unwrap();
    let mut opening = Opening::direct(
        wire,
        LockNegotiate {
            minimum_revision: 1,
            maximum_revision: 2,
            requested_capabilities: 5,
            chords: vec![],
        },
        0,
        3000,
    )
    .unwrap();
    while !opening.is_opened() {
        assert!(start.elapsed() < Duration::from_secs(3));
        opening
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
    }
    let mut session = Session::from_opened_with_image(
        opening.into_opened().unwrap(),
        LockImageConfig {
            endpoint_dir: peer.dir.clone(),
            startup_deadline_ms: 3000,
            transfer_timeout_ms: 1000,
            formats_capacity: 64,
        },
    )
    .unwrap();
    while session.image().unwrap().cause() == sophia_image_producer::CloseCause::None {
        assert!(start.elapsed() < Duration::from_secs(3));
        session
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
        thread::yield_now();
    }
    assert_eq!(session.selected_revision(), Some(2));
    assert_eq!(session.image().unwrap().cause(), cause);
    assert_eq!(session.image().unwrap().producer().cause(), cause);
    // The image channel may fail, but further 9P drives remain valid.
    for _ in 0..4 {
        session
            .dispatch(start.elapsed().as_millis() as u64, 65536)
            .unwrap();
    }
    session.close();
    drop(session);
    assert_eq!(peer.thread.join().unwrap(), 0);
    std::fs::remove_dir_all(peer.dir).unwrap();
}

#[test]
fn discovery_failures_do_not_kill_the_role_session() {
    use sophia_image_producer::CloseCause;
    for case in [
        peer::Case::Malformed,
        peer::Case::Oversized,
        peer::Case::ClunkError(5),
        peer::Case::ClunkError(2),
        peer::Case::ReadError(13),
        peer::Case::ReadError(116),
        peer::Case::ReadError(11),
    ] {
        failure_keeps_session(case, CloseCause::Discovery);
    }
}
#[test]
fn channel_contradiction_keeps_the_role_session() {
    failure_keeps_session(
        peer::Case::Contradiction,
        sophia_image_producer::CloseCause::Fault,
    );
}
#[test]
fn missing_sealed_grant_has_one_consistent_unavailable_cause() {
    failure_keeps_session(
        peer::Case::Unavailable,
        sophia_image_producer::CloseCause::Unavailable,
    );
}
