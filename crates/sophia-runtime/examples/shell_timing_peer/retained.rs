//! Retained-content workload (t331 D3): N immutable sealed tiles are uploaded
//! once before the measured loop, then every frame places all N again at new
//! destinations. The tiles are never retired by the peer: the last presented
//! candidate keeps them leased, so they stay live through End and the hold
//! and are released at connection retirement, like the fresh workload's final
//! resource. Placement has no alpha and damage is engine-derived, so only
//! placement moves.
//! Resource id 1 stays reserved for the optional probe; tiles take ids
//! 2..=N+1 at generation 1 whatever the probe setting.
use sophia_protocol::{ContentLimits, ContentPlacement, ContentResourceId};

use super::Result;

pub const PROBE_ID: u64 = 1;
pub const FIRST_TILE_ID: u64 = PROBE_ID + 1;
pub const MIN_TILES: u64 = 2;
pub const MAX_TILES: u64 = 8;
/// Journal schema for retained runs; fresh runs keep schema 2 byte for byte.
pub const JOURNAL_SCHEMA: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    Fresh,
    Retained { tiles: u32 },
}

impl Workload {
    pub fn tiles(self) -> Option<u32> {
        match self {
            Self::Fresh => None,
            Self::Retained { tiles } => Some(tiles),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Retained { .. } => "retained",
        }
    }
}

/// `workload` absent or "fresh" is today's loop; "retained" needs
/// `retained_resources` in 2..=8 and the sealed file wire. The keys are
/// refused in every other combination so an old configuration cannot change
/// meaning silently.
pub fn parse(
    object: &serde_json::Map<String, serde_json::Value>,
    sealed_files: bool,
) -> Result<Workload> {
    let workload = match object.get("workload").map(|v| v.as_str()) {
        None | Some(Some("fresh")) => Workload::Fresh,
        Some(Some("retained")) => {
            let tiles = object
                .get("retained_resources")
                .and_then(|v| v.as_u64())
                .filter(|n| (MIN_TILES..=MAX_TILES).contains(n))
                .ok_or("retained workload requires retained_resources in 2..=8")?;
            if !sealed_files {
                return Err("retained workload requires the sealed file wire".into());
            }
            Workload::Retained {
                tiles: u32::try_from(tiles)?,
            }
        }
        _ => return Err("invalid workload".into()),
    };
    if workload == Workload::Fresh && object.contains_key("retained_resources") {
        return Err("retained_resources requires the retained workload".into());
    }
    Ok(workload)
}

/// Tiles split the allocation width evenly (rounded down to an even width)
/// and take its full height, so every tile fits at every scheduled slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub tiles: u32,
    pub tile_width: u32,
    pub height: u32,
}

impl Geometry {
    pub fn new(allocation_width: u32, allocation_height: u32, tiles: u32) -> Result<Self> {
        let tile_width = (allocation_width / tiles) & !1;
        if tile_width < 2 || allocation_height == 0 {
            return Err("allocation too narrow for the retained tiles".into());
        }
        Ok(Self {
            tiles,
            tile_width,
            height: allocation_height,
        })
    }

    pub fn tile_bytes(&self) -> Result<u64> {
        super::canvas::rgba_bytes(self.tile_width, self.height).ok_or("upload size overflow".into())
    }

    /// Resident limits must hold with every tile accepted at once, plus the
    /// probe while it is live; ids and placements must cover N tiles.
    pub fn check_limits(&self, limits: &ContentLimits, probe_bytes: u64) -> Result<()> {
        let tile_bytes = self.tile_bytes()?;
        let tiles = u64::from(self.tiles);
        let resident = tile_bytes
            .checked_mul(tiles)
            .and_then(|b| b.checked_add(probe_bytes))
            .ok_or("resident size overflow")?;
        let probe = u64::from(probe_bytes != 0);
        if tile_bytes > limits.max_resource_bytes
            || tile_bytes > limits.max_staging_bytes
            || resident > limits.max_resident_bytes
            || u64::from(limits.max_resource_ids) < tiles + 1
            || u64::from(limits.max_live_resources) < tiles + probe
            || u64::from(limits.max_candidate_placements) < tiles
            || limits.max_width_px < self.tile_width
            || limits.max_height_px < self.height
        {
            return Err("retained workload outside content limits".into());
        }
        Ok(())
    }
}

pub fn tile(index: u32) -> ContentResourceId {
    ContentResourceId {
        id: FIRST_TILE_ID + u64::from(index),
        generation: 1,
    }
}

/// Frame `f` (1-based) puts tile `i` in slot `(i + f - 1) mod N`, so every
/// tile moves every frame and no two tiles share a slot.
pub fn placements(geometry: &Geometry, frame: u64) -> Vec<ContentPlacement> {
    let tiles = u64::from(geometry.tiles);
    (0..geometry.tiles)
        .map(|index| {
            let slot = (u64::from(index) + frame - 1) % tiles;
            ContentPlacement {
                resource: tile(index),
                surface_index: 0,
                destination_x_px: i32::try_from(slot * u64::from(geometry.tile_width))
                    .expect("slot offset fits the allocation"),
                destination_y_px: 0,
            }
        })
        .collect()
}

pub fn journal_placements(placements: &[ContentPlacement]) -> Vec<super::journal::Placement> {
    placements
        .iter()
        .map(|p| super::journal::Placement {
            resource_id: p.resource.id,
            resource_generation: p.resource.generation,
            destination_x_px: p.destination_x_px,
            destination_y_px: p.destination_y_px,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(text: &str) -> serde_json::Map<String, serde_json::Value> {
        serde_json::from_str::<serde_json::Value>(text)
            .unwrap()
            .as_object()
            .unwrap()
            .clone()
    }

    #[test]
    fn fresh_is_the_default_and_refuses_a_stray_count() {
        assert_eq!(parse(&object("{}"), true).unwrap(), Workload::Fresh);
        assert_eq!(
            parse(&object(r#"{"workload":"fresh"}"#), false).unwrap(),
            Workload::Fresh
        );
        assert!(parse(&object(r#"{"retained_resources":4}"#), true).is_err());
        assert!(
            parse(
                &object(r#"{"workload":"fresh","retained_resources":4}"#),
                true
            )
            .is_err()
        );
        assert!(parse(&object(r#"{"workload":"other"}"#), true).is_err());
    }

    #[test]
    fn retained_needs_a_bounded_count_and_the_sealed_file_wire() {
        for n in 2..=8 {
            let text = format!(r#"{{"workload":"retained","retained_resources":{n}}}"#);
            assert_eq!(
                parse(&object(&text), true).unwrap(),
                Workload::Retained { tiles: n }
            );
            assert!(parse(&object(&text), false).is_err(), "bytes or ipc wire");
        }
        for text in [
            r#"{"workload":"retained"}"#,
            r#"{"workload":"retained","retained_resources":1}"#,
            r#"{"workload":"retained","retained_resources":9}"#,
            r#"{"workload":"retained","retained_resources":"4"}"#,
        ] {
            assert!(parse(&object(text), true).is_err(), "{text}");
        }
    }

    #[test]
    fn tiles_divide_the_allocation_evenly_and_stay_inside_it() {
        let geometry = Geometry::new(1920, 24, 8).unwrap();
        assert_eq!(geometry.tile_width, 240);
        assert_eq!(Geometry::new(1366, 24, 4).unwrap().tile_width, 340);
        assert!(Geometry::new(3, 24, 2).is_err());
        assert!(Geometry::new(1920, 0, 2).is_err());
        for frame in 1..=20 {
            let placements = placements(&geometry, frame);
            assert_eq!(placements.len(), 8);
            let mut slots: Vec<i32> = placements.iter().map(|p| p.destination_x_px).collect();
            slots.sort_unstable();
            slots.dedup();
            assert_eq!(slots.len(), 8, "frame {frame}: no two tiles share a slot");
            for (index, placement) in placements.iter().enumerate() {
                assert_eq!(placement.resource.id, 2 + index as u64);
                assert_eq!(placement.resource.generation, 1);
                assert_eq!(placement.surface_index, 0);
                assert_eq!(placement.destination_y_px, 0);
                assert!(placement.destination_x_px >= 0);
                assert!(placement.destination_x_px + 240 <= 1920);
            }
        }
    }

    #[test]
    fn every_tile_moves_every_frame_and_the_schedule_is_periodic() {
        let geometry = Geometry::new(1920, 24, 3).unwrap();
        for frame in 1..=9 {
            let now = placements(&geometry, frame);
            let next = placements(&geometry, frame + 1);
            for (a, b) in now.iter().zip(&next) {
                assert_ne!(a.destination_x_px, b.destination_x_px, "frame {frame}");
            }
            assert_eq!(
                now.iter().map(|p| p.destination_x_px).collect::<Vec<_>>(),
                placements(&geometry, frame + 3)
                    .iter()
                    .map(|p| p.destination_x_px)
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn limits_must_hold_for_all_tiles_at_once() {
        let geometry = Geometry::new(1920, 24, 4).unwrap();
        let mut limits = ContentLimits::prototype(sophia_protocol::ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        });
        limits.max_width_px = 1920;
        limits.max_height_px = 24;
        assert!(geometry.check_limits(&limits, 0).is_ok());
        assert!(geometry.check_limits(&limits, 4 * 1024 * 1024).is_ok());
        let mut small = limits.clone();
        small.max_candidate_placements = 3;
        assert!(geometry.check_limits(&small, 0).is_err());
        let mut small = limits.clone();
        small.max_resource_ids = 4;
        assert!(
            geometry.check_limits(&small, 0).is_err(),
            "ids 2..=5 need five ids"
        );
        let mut small = limits.clone();
        small.max_live_resources = 4;
        assert!(geometry.check_limits(&small, 0).is_ok());
        assert!(
            geometry.check_limits(&small, 4 * 1024 * 1024).is_err(),
            "probe is live too"
        );
        let mut small = limits;
        small.max_resident_bytes = geometry.tile_bytes().unwrap() * 4 - 1;
        assert!(geometry.check_limits(&small, 0).is_err());
    }
}
