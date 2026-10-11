use sophia_portal::{file_service::SessionPortalFileService, files::PortalFileAuthority};
use sophia_protocol::{
    NamespaceCapabilities, NamespaceContext, NamespaceId, NamespaceProfile, PortalRequest,
    portal_files::PortalFileLimits,
};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

struct Authority;
impl PortalFileAuthority for Authority {
    fn namespace(&self, id: NamespaceId) -> Option<NamespaceContext> {
        [1, 2].contains(&id.raw()).then(|| {
            NamespaceContext::new(id, NamespaceProfile::Confined, NamespaceCapabilities::ALL)
                .unwrap()
        })
    }
    fn source_generation(&self, _: &PortalRequest) -> Option<u64> {
        Some(1)
    }
    fn now_msec(&self) -> u64 {
        1
    }
    fn live(&self) -> bool {
        true
    }
}

#[test]
fn an_expired_queued_request_cannot_allocate_a_late_connection() {
    let service =
        SessionPortalFileService::spawn(Authority, PortalFileLimits::defaults(1), |_| false)
            .unwrap();
    let first = service
        .connect(NamespaceId::from_raw(1), NamespaceId::from_raw(2), 1)
        .unwrap();
    assert!(
        service
            .connect(NamespaceId::from_raw(1), NamespaceId::from_raw(2), 1)
            .is_err()
    );
    drop(first);
    let deadline = Instant::now() + Duration::from_secs(2);
    let next = loop {
        match service.connect(NamespaceId::from_raw(1), NamespaceId::from_raw(2), 1) {
            Ok(next) => break next,
            Err(sophia_9p::Errno::EAGAIN) => {
                assert!(
                    Instant::now() < deadline,
                    "expired request must leave the bounded queue"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            Err(error) => panic!("fresh successor refused: {error:?}"),
        }
    };
    assert_eq!(
        next.epoch, 2,
        "expired queued request must not allocate a connection epoch"
    );
}

#[test]
fn next_private_requester_waits_for_previous_transport_retirement() {
    let service =
        SessionPortalFileService::spawn(Authority, PortalFileLimits::defaults(1), |_| false)
            .unwrap();
    let first = service
        .connect(NamespaceId::from_raw(1), NamespaceId::from_raw(2), 1)
        .unwrap();
    std::thread::scope(|scope| {
        let (sent, received) = mpsc::sync_channel(1);
        let service = &service;
        scope.spawn(move || {
            sent.send(service.connect(NamespaceId::from_raw(1), NamespaceId::from_raw(2), 1))
                .unwrap();
        });
        assert!(
            matches!(
                received.recv_timeout(Duration::from_millis(100)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "next private requester must wait for previous transport retirement"
        );
        drop(first);
        let second = received
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .unwrap();
        assert_eq!(
            second.epoch, 2,
            "fresh admission follows observed disconnect"
        );
    });
}
