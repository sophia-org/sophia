//! The peer half of the owned-ring workload: one protocol loop whose turns
//! and pixel oracle reach the owner through a `PeerLink`, so a protected
//! child process can later drive the same frames without a second copy.
use super::*;
use sophia_runtime::frame_cost::{self as cost, Role};
use sophia_shell_client::{ImageAdapterError, RingOfferState, RingUpload};
use std::sync::mpsc;

/// How the peer hands a turn to the owner and checks presented pixels.
pub(super) trait PeerLink {
    /// Announce `turn` and block until the owner releases the peer.
    fn turn(&mut self, turn: PeerTurn);
    /// Check the pixels presented for `frame`; oracle work stays on the peer.
    fn verify(&mut self, frame: u64, height: u32);
}

/// In-process link: turns over channels, the oracle lease over `pixels`.
pub(super) struct ThreadLink {
    pub(super) ready: mpsc::Sender<PeerTurn>,
    pub(super) start: mpsc::Receiver<()>,
    pub(super) pixels: mpsc::Receiver<sophia_runtime::ContentResourceLease>,
    pub(super) verified: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl PeerLink for ThreadLink {
    fn turn(&mut self, turn: PeerTurn) {
        self.ready.send(turn).unwrap();
        self.start.recv_timeout(Duration::from_secs(10)).unwrap();
    }

    fn verify(&mut self, frame: u64, height: u32) {
        // The oracle is peer work; release its extra lease before retirement.
        let lease = self.pixels.recv_timeout(Duration::from_secs(10)).unwrap();
        let bytes = lease.cpu_bytes().unwrap();
        assert_eq!(lease.description().resource, resource(frame));
        assert_eq!(bytes.len(), 1920 * height as usize * 4);
        let expected = [if frame.is_multiple_of(2) { 32 } else { 64 }, 0, 0, 255];
        assert!(bytes.chunks_exact(4).all(|pixel| pixel == expected));
        drop(lease);
        self.verified
            .store(frame, std::sync::atomic::Ordering::Release);
    }
}

fn next(peer: &mut ShellConnection) -> ShellContentRecord {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some((_, record)) = peer.poll_content().unwrap() {
            return record;
        }
        assert!(Instant::now() < deadline, "peer observation timed out");
        std::thread::yield_now();
    }
}

#[derive(Debug)]
pub(super) enum PeerTurn {
    Measure,
    Fill,
    Filled,
}

fn upload_pair(peer: &mut ShellConnection, first: u64, height: u32, link: &mut impl PeerLink) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while peer.image_adapter().unwrap().channel()
        != Some(sophia_shell_client::ImageChannelState::Ready)
    {
        peer.poll_io().unwrap();
        assert!(Instant::now() < deadline, "image channel not ready");
        std::thread::yield_now();
    }
    // Serve control-file writes but defer image jobs until both slots are filled.
    link.turn(PeerTurn::Fill);
    let mut offers = [0; 2];
    for (slot, offer) in offers.iter_mut().enumerate() {
        let generation = first + slot as u64;
        *offer = loop {
            match peer.image_upload_ring_fill(
                RingUpload {
                    resource: resource(generation).id,
                    resource_generation: resource(generation).generation,
                    output: 1,
                    output_generation: 1,
                    allocation: 1,
                    allocation_generation: 1,
                    topology_generation: 1,
                    width: 1920,
                    height,
                },
                1920 * height as usize * 4,
                Instant::now(),
                |bytes| {
                    for pixel in bytes.chunks_exact_mut(4) {
                        pixel.copy_from_slice(&[
                            if generation.is_multiple_of(2) { 32 } else { 64 },
                            0,
                            0,
                            255,
                        ]);
                    }
                },
            ) {
                Ok(offer) => break offer,
                Err(ImageAdapterError::Again) => {
                    peer.poll_io().unwrap();
                }
                Err(error) => panic!("paired ring fill refused: {error:?}"),
            }
            assert!(Instant::now() < deadline, "paired control write timed out");
            std::thread::yield_now();
        };
        assert_eq!(peer.image_ring_offer(*offer).unwrap().slot, slot as u32);
    }
    link.turn(PeerTurn::Filled);
    let mut accepted = [false; 2];
    loop {
        if let Some((_, record)) = peer.poll_content().unwrap() {
            let ShellContentRecord::ResourceStatus(status) = record else {
                panic!("unexpected {record:?}")
            };
            let index = [resource(first), resource(first + 1)]
                .iter()
                .position(|key| *key == status.resource)
                .expect("status must name one of the two current offers");
            assert!(!accepted[index]);
            assert_eq!(status.status, 2, "paired offer refused: {status:?}");
            accepted[index] = true;
        }
        if accepted.iter().all(|accepted| *accepted)
            && offers.iter().all(|offer| {
                peer.image_ring_offer(*offer).unwrap().state == RingOfferState::Settled
            })
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "paired upload settlement timed out"
        );
        std::thread::yield_now();
    }
    for offer in offers {
        peer.image_forget(offer).unwrap();
    }
}

/// One peer's whole protocol loop: paired ring fills on odd frames, then
/// demand, permit, candidate, two outcomes, oracle, and retirement of the
/// predecessor. The link decides how turns and the oracle reach the owner.
pub(super) fn peer_frames(
    mut peer: ShellConnection,
    grant: ContentGrant,
    height: u32,
    frames: u64,
    warmup: u64,
    link: &mut impl PeerLink,
) -> ShellConnection {
    for generation in 1..=frames {
        if generation == warmup + 1 {
            link.turn(PeerTurn::Measure);
            cost::register_thread(Role::Peer);
        }
        if generation % 2 == 1 {
            upload_pair(&mut peer, generation, height, link);
        }
        peer.send_content(
            TransactionId::from_raw(generation * 10),
            &ShellContentRecord::FrameDemand(ContentFrameDemand {
                grant,
                output: OUTPUT,
                allocation: ALLOCATION,
                demand_id: generation,
                reason: 1,
            }),
        )
        .unwrap();
        let ShellContentRecord::FramePermit(permit) = next(&mut peer) else {
            panic!("permit required")
        };
        assert_eq!(permit.demand_id, generation);
        let records = [
            ShellContentRecord::CandidateBegin(ContentCandidateBegin {
                grant,
                output: OUTPUT,
                candidate_generation: generation,
                facts_generation: 1,
                pacing_permit: permit.permit_id,
                interaction_generation: 1,
                surface_count: 1,
                placement_count: 1,
                target_count: 0,
            }),
            ShellContentRecord::CandidateChunk(ContentCandidateChunk {
                grant,
                candidate_generation: generation,
                chunk_ordinal: 0,
                surfaces: vec![ContentSurface {
                    allocation: ALLOCATION,
                    scale_generation: 1,
                    role: 1,
                    edge: 1,
                    margins: ContentMargins::default(),
                    reservation_extent: height,
                    parent_surface_index: u16::MAX,
                    anchor_parent_rect: ContentPixelRect::default(),
                }],
                placements: vec![ContentPlacement {
                    resource: resource(generation),
                    surface_index: 0,
                    destination_x_px: 0,
                    destination_y_px: 0,
                }],
                targets: vec![],
            }),
            ShellContentRecord::CandidateEnd(ContentCandidateEnd {
                grant,
                candidate_generation: generation,
                surface_count: 1,
                placement_count: 1,
                target_count: 0,
            }),
        ];
        peer.enqueue_content_group_tracked(TransactionId::from_raw(generation * 10 + 1), &records)
            .unwrap();
        for kind in [1, 2] {
            let ShellContentRecord::CandidateOutcome(outcome) = next(&mut peer) else {
                panic!("outcome required")
            };
            assert_eq!(
                (outcome.candidate_generation, outcome.kind),
                (generation, kind)
            );
        }
        link.verify(generation, height);
        // Retirement is explicit; the backend must have released the predecessor.
        if generation > 1 {
            peer.send_content(
                TransactionId::from_raw(generation * 10 + 2),
                &ShellContentRecord::ResourceRetire(ContentResourceRetire {
                    grant,
                    resource: resource(generation - 1),
                }),
            )
            .unwrap();
            let ShellContentRecord::ResourceReleased(released) = next(&mut peer) else {
                panic!("release required")
            };
            assert_eq!(released.resource, resource(generation - 1));
        }
    }
    peer
}
