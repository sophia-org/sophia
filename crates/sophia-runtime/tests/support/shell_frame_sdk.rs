//! Imported owning clients against the real opt-in negotiation and file/owner path.
//! Supplied authentication and renderer completion; no protected launch or native claim.
use super::*;
use sophia_9p_client::pipeline::{Pipeline, PipelineLimits, Reply};
use sophia_protocol::*;
use sophia_shell_client::{FrameStream, FrameStreamState, ShellClientOptions, ShellConnection};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

use crate::image_test_authority as authority;
#[path = "shell_frame_sdk_c.rs"]
mod c;
#[path = "shell_frame_sdk_image.rs"]
mod image;
#[path = "shell_frame_terminal_probe.rs"]
mod terminal_probe;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Retained,
    Probe { omit_prepared: bool, split: bool },
    RetainedFallback,
    CopiedFallback,
    CCopiedFallback,
}

fn next(client: &mut ShellConnection, stream: &mut FrameStream) -> ShellFrameRecord {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        client.poll_io().unwrap();
        if let Some(record) = client.take_frame().unwrap() {
            stream.observe(&record).unwrap();
            return record;
        }
        assert!(Instant::now() < until, "SDK frame response timed out");
        std::thread::yield_now();
    }
}

#[test]
fn imported_sdk_owns_credit_through_the_real_retained_scene_and_close() {
    lifecycle(Case::Retained);
}

#[test]
fn drained_stream_keeps_resources_and_accepts_a_legacy_candidate_on_the_same_connection() {
    lifecycle(Case::RetainedFallback);
}

#[test]
fn sdk_establishes_images_then_copies_drains_and_presents_legacy_without_reconnecting() {
    lifecycle(Case::CopiedFallback);
}

#[test]
fn c_sdk_establishes_images_then_copies_drains_and_presents_legacy_without_reconnecting() {
    lifecycle(Case::CCopiedFallback);
}

enum Peer {
    Rust(std::thread::JoinHandle<(ShellConnection, Option<terminal_probe::Pending>)>),
    C(c::Peer),
}

impl Peer {
    fn pid(&self) -> u32 {
        match self {
            Self::Rust(_) => std::process::id(),
            Self::C(peer) => peer.pid(),
        }
    }

    fn is_finished(&mut self) -> bool {
        match self {
            Self::Rust(peer) => peer.is_finished(),
            Self::C(peer) => peer.is_finished(),
        }
    }

    fn finish(self) -> Option<(ShellConnection, terminal_probe::Pending)> {
        match self {
            Self::Rust(peer) => {
                let (client, pending) = peer.join().unwrap();
                pending.map(|pending| (client, pending))
            }
            Self::C(mut peer) => {
                peer.finish();
                None
            }
        }
    }
}

fn lifecycle(case: Case) {
    let copied = matches!(
        case,
        Case::CopiedFallback | Case::CCopiedFallback | Case::Probe { .. }
    );
    let fallback = !matches!(case, Case::Retained | Case::Probe { .. });
    let mut f = fixture::Fixture::new(64);
    let mut t = Transport::reserve(&mut f);
    if copied {
        image::supply_allocation(&mut t, &f);
    }
    let key = fixture::key(1);
    let key = ShellFrameStreamKey {
        grant: key.grant,
        output: key.output,
        allocation: key.allocation,
        facts_generation: key.facts_generation,
        stream_id: key.stream_id,
    };
    let allocation = &f.allocations[0];
    let mut commit = ShellFrameRecord::Commit(ShellFrameCommit {
        transaction: fixture::tx(101),
        candidate: ContentCandidate {
            grant: key.grant,
            candidate_generation: 1,
            output: key.output,
            facts_generation: key.facts_generation,
            pacing_permit: key.stream_id,
            interaction_generation: 8,
            surfaces: vec![ContentSurface {
                allocation: allocation.allocation,
                scale_generation: allocation.scale_generation,
                role: allocation.role,
                edge: allocation.edge,
                margins: allocation.margins,
                reservation_extent: allocation.allowed_reservation_extent,
                parent_surface_index: u16::MAX,
                anchor_parent_rect: allocation.anchor_parent_rect,
            }],
            placements: vec![ContentPlacement {
                resource: fixture::resource(if copied { 10 } else { 2 }),
                surface_index: 0,
                destination_x_px: 0,
                destination_y_px: 0,
            }],
            targets: vec![],
        },
        image: None,
        retire: None,
    });
    if copied && let ShellFrameRecord::Commit(frame) = &mut commit {
        frame.candidate.placements.push(ContentPlacement {
            resource: fixture::resource(6),
            surface_index: 0,
            destination_x_px: 2,
            destination_y_px: 0,
        });
    }
    let path = t.transport.socket_path().to_owned();
    let image_dir = t.path.clone();
    let capabilities = SHELL_FRAME_CAPABILITY
        | crate::owned_ring_protocol::CAPABILITY
        | SOPHIA_SHELL_CAPABILITY_CONTENT_SURFACE
        | SOPHIA_SHELL_CAPABILITY_DESCRIPTOR_SWITCHER;
    let mut peer = if case == Case::CCopiedFallback {
        Peer::C(c::Peer::spawn(&image_dir, &path))
    } else {
        Peer::Rust(std::thread::spawn(move || {
            let timeout = Duration::from_secs(5);
            let remote = UnixStream::connect(path).unwrap();
            let mut pipeline = Pipeline::over(remote, PipelineLimits::default(), timeout).unwrap();
            let (tag, afid) = pipeline.auth(&[], &[]).unwrap();
            assert!(matches!(
                pipeline.wait(tag, Instant::now() + timeout).unwrap(),
                Reply::Auth(_)
            ));
            let (tag, root) = pipeline.attach_with_afid(afid, &[], &[]).unwrap();
            assert!(matches!(
                pipeline.wait(tag, Instant::now() + timeout).unwrap(),
                Reply::Attach(_)
            ));
            let tag = pipeline.clunk(afid).unwrap();
            pipeline.wait(tag, Instant::now() + timeout).unwrap();
            let mut client = ShellConnection::from_attached(
                pipeline,
                root,
                ShellClientOptions {
                    minimum_revision: 13,
                    maximum_revision: 13,
                    required_capabilities: capabilities,
                    handshake_timeout: timeout,
                },
            )
            .unwrap();
            assert_eq!(client.welcome().selected_revision, 13);
            if copied {
                image::enable(&mut client, image_dir, key);
            }
            let mut stream = FrameStream::new();
            if copied {
                image::establish(&mut client, &stream, key);
            }
            stream.open(&mut client, fixture::tx(100), key).unwrap();
            assert!(!stream.has_credit());
            assert!(
                matches!(next(&mut client, &mut stream), ShellFrameRecord::Opened { key: actual, .. } if actual == key)
            );
            assert!(stream.has_credit());
            let source = if copied {
                Some(image::submit(&mut client, &mut stream, &mut commit, key))
            } else {
                stream.submit(&mut client, &commit).unwrap();
                None
            };
            assert!(!stream.has_credit());
            assert!(stream.submit(&mut client, &commit).is_err());
            if matches!(case, Case::Probe { .. }) {
                return (
                    client,
                    Some(terminal_probe::Pending {
                        stream,
                        source: source.unwrap(),
                    }),
                );
            }
            let prepared = next(&mut client, &mut stream);
            assert!(
                matches!(&prepared, ShellFrameRecord::Outcome { outcome, credit_expires_msec: 0, .. } if outcome.kind == 1),
                "{prepared:?}"
            );
            assert!(!stream.has_credit(), "Prepared must not return credit");
            assert!(
                matches!(next(&mut client, &mut stream), ShellFrameRecord::Outcome { outcome, credit_expires_msec, .. } if outcome.kind == 2 && credit_expires_msec > 10)
            );
            assert!(stream.has_credit());
            stream.close(&mut client, fixture::tx(102)).unwrap();
            assert!(!stream.has_credit());
            assert!(
                matches!(next(&mut client, &mut stream), ShellFrameRecord::Closed { key: actual, .. } if actual == key)
            );
            assert_eq!(stream.state(), FrameStreamState::Closed);
            assert!(stream.frame_work_drained());
            if let Some(source) = source {
                image::settle(&mut client, source);
                image::fallback(&mut client, &stream, &mut commit, key);
            }
            if fallback {
                legacy_frame(&mut client, &commit);
            }
            (client, None)
        }))
    };
    t.transport
        .state
        .authorize_authenticated_peer(
            &crate::ProtectionDomainEvidence {
                backend: crate::ProtectionBackendKind::Bubblewrap,
                supervisor_pid: peer.pid(),
                peer_pid: peer.pid(),
                roles: [crate::ProtectionDomainRole::MetadataShell]
                    .into_iter()
                    .collect(),
            },
            Box::new(authority::Factory),
        )
        .unwrap();
    let (mut discovery, limits, formats, bounds) =
        crate::shell_transport::owned_copy::wire_tests::config();
    discovery.connection_epoch = key.grant.connection_epoch;
    t.transport
        .state
        .configure_image_channel(discovery, limits, formats, bounds)
        .unwrap();
    t.transport
        .state
        .begin_frame_file_negotiation(
            &t.transport.content_epochs,
            key.grant.connection_epoch,
            Duration::from_secs(5),
            crate::ShellContentAdmissionPolicy::Granted {
                discrete_input: false,
            },
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    let mut negotiated = false;
    let mut submitted = 0;
    let mut displays = Vec::new();
    let mut owner = crate::ShellIsbImageOwner::new(1 << 20, 8).unwrap();
    while !peer.is_finished() {
        assert!(
            Instant::now() < until,
            "SDK/owner handshake or lifecycle hung"
        );
        if !negotiated {
            if let Some(welcome) = t
                .transport
                .state
                .poll_negotiation(&mut t.transport.content_epochs, 65536)
                .unwrap()
            {
                assert_eq!(welcome.selected_revision, 13);
                assert_eq!(welcome.capabilities & capabilities, capabilities);
                negotiated = true;
            }
        } else {
            t.transport.service_owner_turn().unwrap();
            owner
                .service(
                    &mut t.transport.state,
                    &mut t.transport.content_epochs,
                    10,
                    Instant::now(),
                )
                .unwrap();
            t.transport.service_content_resources(10).unwrap();
            let contexts = [crate::ContentCandidateContext {
                output: key.output,
                facts_generation: key.facts_generation,
                interaction_generation: 8,
                allocations: &f.allocations,
            }];
            t.transport
                .state
                .service_content_demands(
                    &mut t.transport.content_epochs,
                    &[key.output],
                    &f.allocations,
                )
                .unwrap();
            if let Some((transaction, demand)) = t
                .transport
                .state
                .next_content_demand(&t.transport.content_epochs)
            {
                t.transport
                    .state
                    .grant_content_demand(
                        &mut t.transport.content_epochs,
                        transaction,
                        demand.output,
                        9,
                        10,
                    )
                    .unwrap();
            }
            t.transport
                .state
                .service_content_candidates(&mut t.transport.content_epochs, &contexts, 10)
                .unwrap();
            if t.candidates().pending_candidate_count() != 0 && !matches!(case, Case::Probe { .. })
            {
                let generation = submitted + 1;
                displays.push(
                    t.transport
                        .begin_content_submission(key.output, generation, 10)
                        .unwrap(),
                );
                t.transport
                    .content_prepared(key.grant, key.output, generation, 20, 30, 10)
                    .unwrap();
                t.transport
                    .content_presented_at(key.grant, key.output, generation, 40, 20, 30, 10)
                    .unwrap();
                if copied {
                    image::assert_pixels(&t.transport.content_epochs, generation);
                }
                submitted += 1;
            }
            t.transport.poll_io().unwrap();
        }
        std::thread::yield_now();
    }
    let pending = peer.finish();
    assert!(negotiated);
    if let Case::Probe {
        omit_prepared,
        split,
    } = case
    {
        let (client, pending) = pending.unwrap();
        terminal_probe::run(
            &mut t,
            &mut owner,
            &f,
            client,
            pending,
            omit_prepared,
            split,
        );
        return;
    }
    assert!(pending.is_none());
    assert_eq!(submitted, if fallback { 2 } else { 1 });
    assert_eq!(owner.pending(), 0);
    assert!(
        displays.len() == submitted as usize,
        "native owner lease is independent of frame credit"
    );
}

fn next_content(client: &mut ShellConnection) -> ShellContentRecord {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        client.poll_io().unwrap();
        if let Some((_, record)) = client.take_content().unwrap()
            && matches!(
                record,
                ShellContentRecord::FramePermit(_) | ShellContentRecord::CandidateOutcome(_)
            )
        {
            return record;
        }
        assert!(
            Instant::now() < until,
            "legacy response after SFT drain timed out"
        );
        std::thread::yield_now();
    }
}

fn legacy_frame(client: &mut ShellConnection, prior: &ShellFrameRecord) {
    let ShellFrameRecord::Commit(prior) = prior else {
        panic!("commit")
    };
    let scene = &prior.candidate;
    client
        .enqueue_content(
            fixture::tx(110),
            &ShellContentRecord::FrameDemand(ContentFrameDemand {
                grant: scene.grant,
                output: scene.output,
                allocation: scene.surfaces[0].allocation,
                demand_id: 1,
                reason: 1,
            }),
        )
        .unwrap();
    let ShellContentRecord::FramePermit(permit) = next_content(client) else {
        panic!("permit")
    };
    assert_eq!(permit.state, 1);
    let mut candidate = scene.clone();
    candidate.candidate_generation = 2;
    candidate.pacing_permit = permit.permit_id;
    let records = candidate.parts();
    client
        .enqueue_content_group(fixture::tx(111), &records)
        .unwrap();
    for kind in [1, 2] {
        let ShellContentRecord::CandidateOutcome(outcome) = next_content(client) else {
            panic!("outcome")
        };
        assert_eq!((outcome.kind, outcome.candidate_generation), (kind, 2));
    }
    assert_eq!(
        client.welcome().selected_revision,
        13,
        "no reconnect or renegotiation"
    );
}
