use sophia_protocol::*;
use sophia_runtime::{frame_credits::FrameStreamKey, *};

#[path = "content_isb.rs"]
mod isb;

pub fn grant() -> ContentGrant {
    ContentGrant {
        connection_epoch: 7,
        content_grant_epoch: 9,
    }
}
pub fn tx(id: u64) -> TransactionId {
    TransactionId::from_raw(id)
}
pub fn output() -> ContentOutputId {
    ContentOutputId {
        id: 2,
        generation: 3,
    }
}
pub fn resource(id: u64) -> ContentResourceId {
    ContentResourceId { id, generation: 1 }
}
pub fn key(id: u64) -> FrameStreamKey {
    FrameStreamKey {
        grant: grant(),
        output: output(),
        allocation: ContentAllocationId {
            id: 2,
            generation: 1,
        },
        facts_generation: 6,
        stream_id: id,
    }
}
pub fn begin(generation: u64, stream: u64) -> ContentCandidateBegin {
    ContentCandidateBegin {
        grant: grant(),
        candidate_generation: generation,
        output: output(),
        facts_generation: 6,
        pacing_permit: stream,
        interaction_generation: 8,
        surface_count: 1,
        placement_count: 1,
        target_count: 0,
    }
}
pub fn retire(id: u64) -> ContentRetireOnPresent {
    ContentRetireOnPresent {
        retire_transaction: tx(50),
        resource: resource(id),
    }
}

pub struct Fixture {
    pub pool: ContentEpochPool,
    pub allocations: [ContentAllocationSnapshot; 1],
    pub now: u64,
}

impl Fixture {
    pub fn new(records: u32) -> Self {
        Self::with_slots(
            records,
            ContentLimits::prototype(grant()).max_open_candidates_total,
        )
    }

    pub fn with_slots(records: u32, slots: u32) -> Self {
        let mut limits = ContentLimits::prototype(grant());
        limits.max_open_candidates_total = slots;
        limits.max_control_records = 64;
        let mut pool = ContentEpochPool::new(limits.max_session_retiring_bytes).unwrap();
        pool.admit(limits.clone()).unwrap();
        let allocation = ContentAllocationSnapshot {
            native_opening: None,
            output: output(),
            allocation: key(1).allocation,
            scale_generation: 4,
            scale_numerator: 1,
            scale_denominator: 1,
            role: 1,
            edge: 1,
            margins: ContentMargins::default(),
            logical: ContentLogicalRect {
                x: 0,
                y: 0,
                width: 64,
                height: 32,
            },
            pixel: ContentPixelRect {
                x: 0,
                y: 0,
                width: 64,
                height: 32,
            },
            parent: ContentAllocationId::default(),
            anchor_parent_rect: ContentPixelRect::default(),
            allowed_reservation_extent: 32,
        };
        let mut f = Self {
            pool,
            allocations: [allocation],
            now: 0,
        };
        for id in 1..=3 {
            let resources = f.pool.active_mut().unwrap();
            isb::accept(
                resources,
                tx(1),
                ContentResourceBegin {
                    grant: grant(),
                    resource: resource(id),
                    width_px: 2,
                    height_px: 1,
                    rendered_scale_numerator: 1,
                    rendered_scale_denominator: 1,
                    pixel_format: 1,
                    chunk_count: 1,
                    total_bytes: 8,
                },
                &[0; 8],
                0,
            );
            while resources.take_event().is_some() {}
        }
        // Resource releases retain their own credits; test the candidate budget separately.
        limits.max_control_records = records;
        *f.pool.active_candidates_mut().unwrap() = ContentCandidateStore::new(limits).unwrap();
        f
    }

    pub fn candidates(&mut self) -> &mut ContentCandidateStore {
        self.pool.active_candidates_mut().unwrap()
    }

    pub fn tick(&mut self) -> u64 {
        self.now += 1;
        self.now
    }

    pub fn open(&mut self, stream: u64) -> Result<(), ContentCandidateError> {
        self.open_with_credits(stream, 2)
    }

    pub fn open_with_credits(
        &mut self,
        stream: u64,
        credits: u8,
    ) -> Result<(), ContentCandidateError> {
        let now = self.tick();
        let context = ContentCandidateContext {
            output: output(),
            facts_generation: 6,
            interaction_generation: 8,
            allocations: &self.allocations,
        };
        self.pool
            .active_candidates_mut()
            .unwrap()
            .open_frame_stream(tx(10), key(stream), credits, context, now)
    }

    pub fn open_drained(&mut self) {
        self.open(1).unwrap();
        assert!(matches!(
            self.candidates().take_frame_stream_event().unwrap().record,
            ContentFrameStreamRecord::Opened { .. }
        ));
    }

    pub fn frame(
        &mut self,
        generation: u64,
        draws: u64,
        intent: Option<ContentRetireOnPresent>,
    ) -> Result<(), ContentCandidateError> {
        let now = self.tick();
        self.candidates().begin(tx(11), begin(generation, 1), now)?;
        self.finish_frame(generation, draws, intent)
    }

    pub fn finish_frame(
        &mut self,
        generation: u64,
        draws: u64,
        intent: Option<ContentRetireOnPresent>,
    ) -> Result<(), ContentCandidateError> {
        let now = self.tick();
        self.candidates().chunk(
            tx(12),
            ContentCandidateChunk {
                grant: grant(),
                candidate_generation: generation,
                chunk_ordinal: 0,
                surfaces: vec![ContentSurface {
                    allocation: key(1).allocation,
                    scale_generation: 4,
                    role: 1,
                    edge: 1,
                    margins: ContentMargins::default(),
                    reservation_extent: 24,
                    parent_surface_index: u16::MAX,
                    anchor_parent_rect: ContentPixelRect::default(),
                }],
                placements: vec![ContentPlacement {
                    resource: resource(draws),
                    surface_index: 0,
                    destination_x_px: 3,
                    destination_y_px: 4,
                }],
                targets: Vec::new(),
            },
            now,
        )?;
        let now = self.tick();
        let context = ContentCandidateContext {
            output: output(),
            facts_generation: 6,
            interaction_generation: 8,
            allocations: &self.allocations,
        };
        let end = ContentCandidateEnd {
            grant: grant(),
            candidate_generation: generation,
            surface_count: 1,
            placement_count: 1,
            target_count: 0,
        };
        let (resources, candidates) = self.pool.active_pair_mut().unwrap();
        if let Some(intent) = intent {
            candidates.end_with_retire(tx(13), end, intent, context, resources, now)
        } else {
            candidates.end(tx(13), end, context, resources, now)
        }
    }

    pub fn submit(&mut self, generation: u64) -> ContentRenderBundle {
        let now = self.tick();
        let bundle = self
            .candidates()
            .begin_submission(output(), generation, now)
            .unwrap();
        let now = self.tick();
        self.candidates()
            .prepared(output(), generation, 20, 30, now)
            .unwrap();
        bundle
    }

    pub fn present(&mut self, generation: u64) {
        let now = self.tick();
        self.candidates()
            .presented_at(output(), generation, 40, 20, 30, now)
            .unwrap();
    }

    pub fn drain_outcomes(&mut self) -> Vec<ContentCandidateEvent> {
        let mut events = Vec::new();
        while let Some(event) = self.candidates().pending_event().cloned() {
            if event.frame_credit.is_some() {
                let now = self.now;
                let (resources, candidates) = self.pool.active_pair_mut().unwrap();
                assert!(
                    candidates
                        .transfer_frame_event_at(resources, now, |published| {
                            events.push(published.clone());
                            Ok(())
                        })
                        .unwrap()
                );
            } else {
                if let Some(owner) = event.retire {
                    self.pool.active_mut().unwrap().commit_retire(&owner);
                }
                assert_eq!(self.candidates().take_event(), Some(event.clone()));
                events.push(event);
            }
        }
        events
    }
}
