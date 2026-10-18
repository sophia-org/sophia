//! Supplied launcher authority; real 9P routing and ISB descriptor custody.
use super::*;

struct Rig {
    connection: ShellConnection,
    peer: Peer,
    owner: OwnedFd,
    dir: PathBuf,
    limits: ContentLimits,
    lifecycle: NativeLauncherLifecycle,
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
                revision: 9,
                capabilities: 0x9a0 | SEALED,
                limits_published: true,
            },
            false,
        );
        let ShellContentRecord::Limits(limits) = take_role_content(&mut connection) else {
            panic!("missing Limits");
        };
        let lifecycle = NativeLauncherLifecycle::new(limits.clone()).unwrap();
        let owner = drive_to_ready(&mut connection, &mut peer, &listener);
        Self {
            connection,
            peer,
            owner,
            dir,
            limits,
            lifecycle,
        }
    }
    fn begin(&mut self, id: u64) -> ShellImageMetadata {
        let mut m = metadata();
        m.resource = id;
        m.admission_epoch = self.limits.grant.content_grant_epoch;
        self.connection
            .image_begin(m, sealed_fd(), Instant::now())
            .unwrap();
        answer_submit(
            &mut self.connection,
            &mut self.peer,
            ShellFileKind::ImageBegin,
        );
        ShellImageMetadata::decode(&self.peer.transactions.last().unwrap()[32..]).unwrap()
    }
    fn send(&mut self, m: &ShellImageMetadata) {
        let bytes = self
            .peer
            .event_image(ShellFileKind::ImageAdmitted, &m.encode().unwrap());
        self.peer.push(bytes);
        drive(&mut self.connection, &mut self.peer, |c, _| {
            c.image_adapter().unwrap().offer(m.offer_id).unwrap().state == OfferState::Sent
        });
        let (_, fds) = recv_packet(&self.owner);
        assert_eq!(fds.len(), 1);
    }
    fn status(&mut self, m: &ShellImageMetadata, status: u16) {
        let body = encode_shell_file_resource_status_body(&ShellFileTransactionRecord {
            transaction: TransactionId::from_raw(m.offer_id),
            record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
                grant: self.limits.grant,
                resource: Self::resource(m),
                status,
                reason: 0,
                next_ordinal: 0,
                admitted_bytes: m.declared_bytes,
            }),
        })
        .unwrap();
        let bytes = self.peer.event(ShellFileKind::ResourceStatus, &body);
        self.peer.push(bytes);
        drive(&mut self.connection, &mut self.peer, |c, _| {
            c.image_adapter()
                .unwrap()
                .offer(m.offer_id)
                .unwrap()
                .status
                .is_some()
        });
    }
    fn release(&mut self, m: &ShellImageMetadata, disposition: ShellSourceDisposition) {
        let body = ShellSourceReleased {
            offer_id: m.offer_id,
            resource: m.resource,
            resource_generation: m.resource_generation,
            disposition,
        }
        .encode()
        .unwrap();
        let bytes = self.peer.event_image(ShellFileKind::SourceReleased, &body);
        self.peer.push(bytes);
        drive(&mut self.connection, &mut self.peer, |c, _| {
            c.image_adapter().unwrap().offer(m.offer_id).unwrap().state == OfferState::Settled
        });
    }
    fn complete(&mut self, id: u64) -> ShellImageMetadata {
        let m = self.begin(id);
        self.send(&m);
        self.status(&m, 2);
        self.release(&m, ShellSourceDisposition::Retained);
        m
    }
    fn resource(m: &ShellImageMetadata) -> ContentResourceId {
        ContentResourceId {
            id: m.resource,
            generation: m.resource_generation,
        }
    }
    fn adopt(&mut self, m: &ShellImageMetadata) -> Result<ContentResourceId, ShellClientError> {
        self.connection
            .adopt_native_image(&mut self.lifecycle, m.offer_id)
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
    let m = r.begin(21);
    assert!(r.adopt(&m).is_err());
    r.send(&m);
    assert!(r.adopt(&m).is_err());
    r.status(&m, 2);
    assert!(r.adopt(&m).is_err(), "Accepted alone is not settlement");
    assert!(
        r.connection
            .image_adapter()
            .unwrap()
            .offer(m.offer_id)
            .is_some()
    );
    assert_eq!(r.lifecycle.resource_state(Rig::resource(&m)), None);
    r.release(&m, ShellSourceDisposition::Retained);
    assert_eq!(r.adopt(&m).unwrap(), Rig::resource(&m));
    assert!(
        r.connection
            .image_adapter()
            .unwrap()
            .offer(m.offer_id)
            .is_none()
    );
    assert!(
        r.adopt(&m).is_err(),
        "the consumed offer cannot be adopted twice"
    );
    r.connection
        .retire_native_resource(
            &mut r.lifecycle,
            TransactionId::from_raw(500),
            Rig::resource(&m),
        )
        .unwrap();
    r.observe(); // The Accepted was buffered before adoption, delivered after Retire.
    assert_eq!(
        r.lifecycle.resource_state(Rig::resource(&m)),
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
            resource: Rig::resource(&m),
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
    assert_eq!(r.lifecycle.resource_state(Rig::resource(&m)), None);
    assert!(
        r.adopt(&m).is_err(),
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
        let m = r.complete(21);
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
            assert!(r.adopt(&m).is_err());
            assert!(
                r.connection
                    .image_adapter()
                    .unwrap()
                    .offer(m.offer_id)
                    .is_some()
            );
        } else {
            r.adopt(&m).unwrap();
        }
    }
}

#[test]
fn adoption_checks_grant_bounds_connection_and_duplicate_identity() {
    let mut r = Rig::new("native-checks");
    let m = r.complete(21);
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
        assert!(
            r.connection
                .adopt_native_image(&mut other, m.offer_id)
                .is_err()
        );
        assert!(
            r.connection
                .image_adapter()
                .unwrap()
                .offer(m.offer_id)
                .is_some()
        );
    }
    let mut other = Rig::new("native-other-connection");
    assert!(
        other
            .connection
            .adopt_native_image(&mut r.lifecycle, m.offer_id)
            .is_err()
    );
    r.adopt(&m).unwrap();
    r.observe();
    let duplicate = r.complete(21);
    assert!(matches!(
        r.adopt(&duplicate),
        Err(ShellClientError::InvalidArgument(
            "native resource already tracked"
        ))
    ));
    assert!(
        r.connection
            .image_adapter()
            .unwrap()
            .offer(duplicate.offer_id)
            .is_some()
    );
}

#[test]
fn contradictory_status_fails_adopted_and_retiring_resources() {
    for retiring in [false, true] {
        for fault in 0..3 {
            let mut r = Rig::new(&format!("native-contradiction-{retiring}-{fault}"));
            let m = r.complete(21);
            r.adopt(&m).unwrap();
            r.observe();
            if retiring {
                r.connection
                    .retire_native_resource(
                        &mut r.lifecycle,
                        TransactionId::from_raw(500),
                        Rig::resource(&m),
                    )
                    .unwrap();
            }
            let record = ShellFileTransactionRecord {
                transaction: TransactionId::from_raw(m.offer_id + u64::from(fault == 0)),
                record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
                    grant: r.limits.grant,
                    resource: Rig::resource(&m),
                    status: match fault {
                        1 => 3,
                        2 => 1,
                        _ => 2,
                    },
                    reason: 0,
                    next_ordinal: 0,
                    admitted_bytes: m.declared_bytes,
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
        let m = r.complete(id);
        r.adopt(&m).unwrap();
        r.observe();
    }
    let m = r.complete(17);
    assert!(matches!(r.adopt(&m), Err(ShellClientError::QueueSaturated)));
    assert_eq!(
        r.connection
            .image_adapter()
            .unwrap()
            .offer(m.offer_id)
            .unwrap()
            .state,
        OfferState::Settled
    );
}

#[test]
fn rejected_and_cancelled_offers_never_become_placeable() {
    for (status, disposition) in [
        (3, ShellSourceDisposition::RefusedBeforeUse),
        (4, ShellSourceDisposition::CancelledBeforeImport),
    ] {
        let mut r = Rig::new(&format!("native-refused-{status}"));
        let m = r.begin(21);
        r.send(&m);
        r.status(&m, status);
        r.release(&m, disposition);
        assert!(r.adopt(&m).is_err());
        assert_eq!(r.lifecycle.resource_state(Rig::resource(&m)), None);
        assert!(
            r.connection
                .image_adapter()
                .unwrap()
                .offer(m.offer_id)
                .is_some()
        );
    }
}

#[test]
fn owning_session_adopts_isb_without_exposing_its_connection() {
    let (mut connection, mut peer, dir, listener) = connect_role_image(
        "native-owning-session",
        4096,
        Profile {
            role: "launcher",
            revision: 9,
            capabilities: 0x9a0 | SEALED,
            limits_published: true,
        },
        false,
    );
    let owner = drive_to_ready(&mut connection, &mut peer, &listener);
    let mut session = NativeLauncherSession::new(connection, 500).unwrap();
    let mut m = metadata();
    m.admission_epoch = session.content_limits().grant.content_grant_epoch;
    let grant = session.content_limits().grant;
    let admitted = session.image_begin(m, sealed_fd(), Instant::now()).unwrap();
    let step = |session: &mut NativeLauncherSession, peer: &mut Peer| {
        session.dispatch(Instant::now(), 1_000, 65_536).unwrap();
        peer.pump();
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while peer.submits.is_empty() {
        step(&mut session, &mut peer);
        assert!(Instant::now() < deadline);
    }
    let held = peer.submits.pop_front().unwrap();
    let record = peer.transactions.last().unwrap();
    let submission = u64::from_le_bytes(record[16..24].try_into().unwrap());
    let metadata = ShellImageMetadata::decode(&record[32..]).unwrap();
    peer.answer_write(held.tag, held.data.len() as u32);
    let event = peer.submitted_image(submission, ShellFileKind::ImageBegin);
    peer.push(event);
    let event = peer.event_image(ShellFileKind::ImageAdmitted, &metadata.encode().unwrap());
    peer.push(event);
    while session
        .image_adapter()
        .unwrap()
        .offer(admitted.offer)
        .unwrap()
        .state
        != OfferState::Sent
    {
        step(&mut session, &mut peer);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(recv_packet(&owner).1.len(), 1);
    let resource = ContentResourceId {
        id: metadata.resource,
        generation: metadata.resource_generation,
    };
    let body = encode_shell_file_resource_status_body(&ShellFileTransactionRecord {
        transaction: TransactionId::from_raw(admitted.offer),
        record: ShellContentRecord::ResourceStatus(ContentResourceStatus {
            grant,
            resource,
            status: 2,
            reason: 0,
            next_ordinal: 0,
            admitted_bytes: metadata.declared_bytes,
        }),
    })
    .unwrap();
    let event = peer.event(ShellFileKind::ResourceStatus, &body);
    peer.push(event);
    let body = ShellSourceReleased {
        offer_id: admitted.offer,
        resource: resource.id,
        resource_generation: resource.generation,
        disposition: ShellSourceDisposition::Retained,
    }
    .encode()
    .unwrap();
    let event = peer.event_image(ShellFileKind::SourceReleased, &body);
    peer.push(event);
    while session
        .image_adapter()
        .unwrap()
        .offer(admitted.offer)
        .unwrap()
        .state
        != OfferState::Settled
    {
        step(&mut session, &mut peer);
        assert!(Instant::now() < deadline);
    }
    assert_eq!(session.adopt_image(admitted.offer).unwrap(), resource);
    assert!(
        session
            .image_adapter()
            .unwrap()
            .offer(admitted.offer)
            .is_none()
    );
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
