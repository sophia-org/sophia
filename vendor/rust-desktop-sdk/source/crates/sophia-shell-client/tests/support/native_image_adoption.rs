//! Supplied launcher authority; real 9P routing and owned-ring custody.
use super::*;

struct Rig {
    connection: ShellConnection,
    peer: Peer,
    _owner: OwnedFd,
    _source: OwnedFd,
    dir: PathBuf,
    limits: ContentLimits,
    lifecycle: NativeLauncherLifecycle,
    uploads: u64,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Rig {
    fn new(name: &str) -> Self {
        let (mut connection, mut peer, dir, listener) = connect_role_image(
            name,
            4096,
            Profile {
                role: "launcher",
                revision: 10,
                capabilities: 0x9a0 | OWNED,
                limits_published: true,
            },
            false,
        );
        let ShellContentRecord::Limits(limits) = take_role_content(&mut connection) else {
            panic!("missing Limits");
        };
        let lifecycle = NativeLauncherLifecycle::new(limits.clone()).unwrap();
        let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
        let (source, _size) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
        Self {
            connection,
            peer,
            _owner: accepted,
            _source: source,
            dir,
            limits,
            lifecycle,
            uploads: 0,
        }
    }

    fn resource(id: u64) -> ContentResourceId {
        ContentResourceId { id, generation: 1 }
    }

    /// Upload one 2x2 image for resource `id`; returns the offer id.
    /// The single slot's generation is 2, 4, 6, ... per upload.
    fn begin(&mut self, id: u64) -> u64 {
        self.uploads += 1;
        let writes = self.peer.control_writes.len();
        let offer = self
            .connection
            .image_upload_ring(
                RingUpload {
                    resource: id,
                    resource_generation: 1,
                    allocation: 61,
                    allocation_generation: 1,
                    output: 71,
                    output_generation: 1,
                    topology_generation: 1,
                    width: 2,
                    height: 2,
                },
                &[9u8; 16],
                Instant::now(),
            )
            .unwrap();
        drive(&mut self.connection, &mut self.peer, |_, peer| {
            peer.control_writes.len() == writes + 1
        });
        offer
    }

    fn generation(&self) -> u64 {
        self.uploads * 2
    }

    fn status(&mut self, offer: u64, resource: u64, status: u16) {
        let bytes = self.peer.event(
            ShellFileKind::ResourceStatus,
            &status_body(offer, resource, 1, status),
        );
        self.peer.push(bytes);
        drive(&mut self.connection, &mut self.peer, |c, _| {
            c.image_ring_offer(offer).unwrap().status.is_some()
        });
    }

    fn release(&mut self, offer: u64, resource: u64, reason: u16) {
        let released = ring_released(
            self.peer.take_sequence(),
            offer,
            self.generation(),
            resource,
            1,
            reason,
        );
        self.peer.push(released);
        drive(&mut self.connection, &mut self.peer, |c, _| {
            c.image_ring_offer(offer).unwrap().state != RingOfferState::Pending
        });
    }

    fn complete(&mut self, id: u64) -> u64 {
        let offer = self.begin(id);
        self.status(offer, id, 2);
        self.release(offer, id, 0);
        offer
    }

    fn adopt(&mut self, offer: u64) -> Result<ContentResourceId, ShellClientError> {
        self.connection
            .adopt_native_image(&mut self.lifecycle, offer)
    }

    fn observe(&mut self) {
        while let Some(value) = self.connection.take_native_launcher_observation().unwrap() {
            self.lifecycle.observe(&value).unwrap();
        }
    }
}

#[test]
fn adoption_requires_settlement_and_keeps_retirement_through_late_status() {
    let mut r = Rig::new("native-adopt");
    let offer = r.begin(21);
    assert!(r.adopt(offer).is_err());
    r.status(offer, 21, 2);
    assert!(r.adopt(offer).is_err(), "Accepted alone is not settlement");
    assert!(r.connection.image_ring_offer(offer).is_some());
    assert_eq!(r.lifecycle.resource_state(Rig::resource(21)), None);
    r.release(offer, 21, 0);
    assert_eq!(r.adopt(offer).unwrap(), Rig::resource(21));
    assert!(r.connection.image_ring_offer(offer).is_none());
    assert!(
        r.adopt(offer).is_err(),
        "the consumed offer cannot be adopted twice"
    );
    r.connection
        .retire_native_resource(
            &mut r.lifecycle,
            TransactionId::from_raw(500),
            Rig::resource(21),
        )
        .unwrap();
    r.observe(); // The Accepted was buffered before adoption, delivered after Retire.
    assert_eq!(
        r.lifecycle.resource_state(Rig::resource(21)),
        Some(NativeResourceState::Retiring)
    );
    answer_submit(
        &mut r.connection,
        &mut r.peer,
        ShellFileKind::ResourceRetire,
    );
    let body = encode_shell_file_resource_released_body(&ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(500),
        record: ShellContentRecord::ResourceReleased(ContentResourceReleased {
            grant: r.limits.grant,
            resource: Rig::resource(21),
            reason: 1,
        }),
    })
    .unwrap();
    let bytes = r.peer.event(ShellFileKind::ResourceReleased, &body);
    r.peer.push(bytes);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        r.peer.pump();
        r.connection.poll_io().unwrap();
        if let Some(released) = r.connection.take_native_launcher_observation().unwrap() {
            r.lifecycle.observe(&released).unwrap();
            break;
        }
        assert!(Instant::now() < deadline);
    }
    assert_eq!(r.lifecycle.resource_state(Rig::resource(21)), None);
    assert!(
        r.adopt(offer).is_err(),
        "release cannot make an old offer replayable"
    );
}

#[test]
fn closed_image_channel_keeps_adoption_but_role_close_does_not() {
    for role_close in [false, true] {
        let mut r = Rig::new(if role_close {
            "native-role-close"
        } else {
            "native-channel-close"
        });
        let offer = r.complete(21);
        let body = ShellChannelClosed {
            channel_generation: 3,
            reason: ShellChannelClosedReason::PeerDisconnect,
        }
        .encode()
        .unwrap();
        let bytes = r.peer.event_image(ShellFileKind::ChannelClosed, &body);
        r.peer.push(bytes);
        drive(&mut r.connection, &mut r.peer, |c, _| {
            c.image_adapter().unwrap().channel() == Some(ImageChannelState::Closed)
        });
        if role_close {
            r.connection.close();
            assert!(r.adopt(offer).is_err());
            assert!(r.connection.image_ring_offer(offer).is_some());
        } else {
            r.adopt(offer).unwrap();
        }
    }
}

#[test]
fn adoption_checks_grant_bounds_connection_and_duplicate_identity() {
    let mut r = Rig::new("native-checks");
    let offer = r.complete(21);
    for mutation in 0..5 {
        let mut limits = r.limits.clone();
        match mutation {
            0 => limits.grant.connection_epoch += 1,
            1 => limits.grant.content_grant_epoch += 1,
            2 => limits.max_width_px = 1,
            3 => limits.max_height_px = 1,
            _ => limits.max_resource_bytes = 4,
        }
        let mut other = NativeLauncherLifecycle::new(limits).unwrap();
        assert!(r.connection.adopt_native_image(&mut other, offer).is_err());
        assert!(r.connection.image_ring_offer(offer).is_some());
    }
    let mut other = Rig::new("native-other-connection");
    assert!(
        other
            .connection
            .adopt_native_image(&mut r.lifecycle, offer)
            .is_err()
    );
    r.adopt(offer).unwrap();
    r.observe();
    let duplicate = r.complete(21);
    assert!(matches!(
        r.adopt(duplicate),
        Err(ShellClientError::InvalidArgument(
            "native resource already tracked"
        ))
    ));
    assert!(r.connection.image_ring_offer(duplicate).is_some());
}

#[test]
fn contradictory_status_fails_adopted_and_retiring_resources() {
    for retiring in [false, true] {
        for fault in 0..3 {
            let mut r = Rig::new(&format!("native-contradiction-{retiring}-{fault}"));
            let offer = r.complete(21);
            r.adopt(offer).unwrap();
            r.observe();
            if retiring {
                r.connection
                    .retire_native_resource(
                        &mut r.lifecycle,
                        TransactionId::from_raw(500),
                        Rig::resource(21),
                    )
                    .unwrap();
            }
            let record = ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(offer + u64::from(fault == 0)),
                record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
                    grant: r.limits.grant,
                    resource: Rig::resource(21),
                    status: match fault {
                        1 => 3,
                        2 => 1,
                        _ => 2,
                    },
                    reason: 0,
                    next_ordinal: 0,
                    admitted_bytes: 16,
                }),
            };
            let bytes = r.peer.event(
                ShellFileKind::ResourceStatus,
                &encode_shell_file_resource_status_body(&record).unwrap(),
            );
            r.peer.push(bytes);
            let start = Instant::now();
            let observation = loop {
                r.connection.poll_io().unwrap();
                r.peer.pump();
                if let Some(value) = r.connection.take_native_launcher_observation().unwrap() {
                    break value;
                }
                assert!(start.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(1));
            };
            assert!(matches!(
                r.lifecycle.observe(&observation),
                Err(ShellClientError::Protocol("adopted image status"))
            ));
            assert_eq!(r.lifecycle.state(), NativeLifecycleState::Failed);
        }
    }
}

#[test]
fn a_full_lifecycle_preserves_the_settled_offer() {
    let mut r = Rig::new("native-capacity");
    for id in 1..=16 {
        let offer = r.complete(id);
        r.adopt(offer).unwrap();
        r.observe();
    }
    let offer = r.complete(17);
    assert!(matches!(
        r.adopt(offer),
        Err(ShellClientError::QueueSaturated)
    ));
    assert_eq!(
        r.connection.image_ring_offer(offer).unwrap().state,
        RingOfferState::Settled
    );
}

#[test]
fn rejected_and_cancelled_offers_never_become_placeable() {
    for status in [3u16, 4] {
        let mut r = Rig::new(&format!("native-refused-{status}"));
        let offer = r.begin(21);
        r.status(offer, 21, status);
        // The runtime pins the release reason to the status kind:
        // Cancelled travels with reason 11, Rejected with any other.
        r.release(offer, 21, if status == 4 { 11 } else { 1 });
        assert!(r.adopt(offer).is_err());
        assert_eq!(r.lifecycle.resource_state(Rig::resource(21)), None);
        assert!(r.connection.image_ring_offer(offer).is_some());
    }
}

#[test]
fn owning_session_adopts_ring_without_exposing_its_connection() {
    let (mut connection, mut peer, dir, listener) = connect_role_image(
        "native-owning-session",
        4096,
        Profile {
            role: "launcher",
            revision: 10,
            capabilities: 0x9a0 | OWNED,
            limits_published: true,
        },
        false,
    );
    let accepted = drive_to_channel_ready(&mut connection, &mut peer, &listener);
    let (_source, _size) = drive_to_ring_ready(&mut connection, &mut peer, &accepted);
    let mut session = NativeLauncherSession::new(connection, 500).unwrap();
    let offer = session
        .image_upload_ring(
            RingUpload {
                resource: 21,
                resource_generation: 1,
                allocation: 61,
                allocation_generation: 1,
                output: 71,
                output_generation: 1,
                topology_generation: 1,
                width: 2,
                height: 2,
            },
            &[9u8; 16],
            Instant::now(),
        )
        .unwrap();
    let step = |session: &mut NativeLauncherSession, peer: &mut Peer| {
        session.dispatch(Instant::now(), 1_000, 65_536).unwrap();
        peer.pump();
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while peer.control_writes.len() < 2 {
        step(&mut session, &mut peer);
        assert!(Instant::now() < deadline);
    }
    let resource = ContentResourceId {
        id: 21,
        generation: 1,
    };
    let event = peer.event(ShellFileKind::ResourceStatus, &status_body(offer, 21, 1, 2));
    peer.push(event);
    let released = ring_released(peer.take_sequence(), offer, 2, 21, 1, 0);
    peer.push(released);
    while session
        .image_adapter()
        .unwrap()
        .ring_offer(offer)
        .unwrap()
        .state
        != RingOfferState::Settled
    {
        step(&mut session, &mut peer);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(session.adopt_image(offer).unwrap(), resource);
    assert!(session.image_adapter().unwrap().ring_offer(offer).is_none());
    let retirement = session.retire_resource(resource).unwrap();
    assert_eq!(session.custody(retirement.first), Ok(Custody::Queued));
    while session.next_event(1_000).unwrap().is_some() {
        session.done().unwrap();
    }
    assert_eq!(
        session.lifecycle().resource_state(resource),
        Some(NativeResourceState::Retiring)
    );
    assert!(
        session
            .image_readiness()
            .unwrap()
            .unwrap()
            .image_fd
            .is_some()
    );
    session.close();
    std::fs::remove_dir_all(dir).unwrap();
}
