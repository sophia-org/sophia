#![cfg(all(feature = "gbm-probe", feature = "libdrm-events"))]

//! The model's `RepaintMatchesFullRepaint` made executable: an identical frame
//! sequence rendered damage-limited and rendered full must produce identical
//! captured pixels, frame by frame. The failure this hunts is a repaint whose
//! damage missed a region -- a frame that is presentable, self-consistent, and
//! stale in one corner, which no health check would catch.
//!
//! Needs a real render node and `SOPHIA_RUN_REAL_GBM_SMOKE=1`; no DRM master,
//! no display takeover. Silently skips otherwise, like the GBM probe smoke.

use std::sync::Arc;

use sophia_backend_live::LiveRendererFrameSlotId;
use sophia_backend_live::WorkerSlotDamage;
use sophia_renderer_live::NativeFrameTargetSetId;
use sophia_renderer_live::{
    LiveCompositionPlacement, LiveGbmEglFrameTargetRecord, LiveNativeCompositionRepaintOutcome,
    LiveOwnedMixedCompositionFrame, LiveOwnedMixedCompositionLayer, LiveSharedCpuBufferSource,
    NativeGbmRenderedScanoutContext,
};

const WIDTH: i32 = 320;
const HEIGHT: i32 = 200;
const FRAMES: usize = 12;
const SLOTS: usize = 3;

fn first_openable_render_node() -> Option<std::path::PathBuf> {
    let entries = std::fs::read_dir("/dev/dri").ok()?;
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("renderD") {
            candidates.push(entry.path());
        }
    }
    candidates.sort();
    // Read-write: the GPU maps buffers through this descriptor, and a
    // read-only open fails there with EACCES rather than at open.
    candidates
        .into_iter()
        .find(|path| open_render_node(path).is_ok())
}

fn open_render_node(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
}

fn output() -> sophia_engine::HeadlessOutput {
    sophia_engine::HeadlessOutput {
        id: sophia_protocol::OutputId::from_raw(1),
        size: sophia_protocol::Size {
            width: WIDTH,
            height: HEIGHT,
        },
        scale: 1,
    }
}

const TILES: [sophia_protocol::Rect; 3] = [
    sophia_protocol::Rect {
        x: 12,
        y: 20,
        width: 72,
        height: 48,
    },
    sophia_protocol::Rect {
        x: 120,
        y: 60,
        width: 84,
        height: 64,
    },
    sophia_protocol::Rect {
        x: 228,
        y: 110,
        width: 64,
        height: 72,
    },
];

/// The scene at one step: each tile at its committed generation. The colors a
/// frame draws are a function of exactly these generations, so the snapshot's
/// damage honestly describes every pixel that changes between steps.
fn snapshot(generations: [u64; 3]) -> sophia_engine::OutputFrameDamageSnapshot {
    let output = output();
    let committed: Vec<sophia_protocol::CommittedSurfaceState> = TILES
        .iter()
        .zip(generations)
        .enumerate()
        .map(
            |(index, (tile, generation))| sophia_protocol::CommittedSurfaceState {
                surface: sophia_protocol::SurfaceId::new(
                    u32::try_from(index + 1).expect("tile index"),
                    1,
                ),
                committed_generation: generation,
                geometry: *tile,
                content: sophia_protocol::SurfaceContentSet::singleton(
                    sophia_protocol::BufferSource::CpuBuffer {
                        handle: u64::try_from(index + 1).expect("tile index"),
                    },
                    sophia_protocol::Size {
                        width: tile.width,
                        height: tile.height,
                    },
                ),
                damage: sophia_protocol::Region::single(*tile),
            },
        )
        .collect();
    let display_list = sophia_engine::CompositorDisplayList {
        output: output.id,
        commands: committed
            .iter()
            .map(|state| sophia_engine::CompositorDisplayCommand::Surface {
                surface: state.surface,
            })
            .collect(),
    };
    sophia_engine::output_frame_damage_snapshot(output, display_list, &committed, None)
        .expect("equivalence snapshot")
}

fn generation_color(tile: usize, generation: u64) -> sophia_engine::CompositorRgb8 {
    // Distinct, deterministic, and different for every generation step, so a
    // stale tile differs from a repainted one in every byte that matters.
    let seed = (tile as u64 + 1)
        .wrapping_mul(97)
        .wrapping_add(generation.wrapping_mul(41));
    sophia_engine::CompositorRgb8 {
        red: (seed % 200) as u8 + 40,
        green: ((seed / 7) % 200) as u8 + 30,
        blue: ((seed / 13) % 200) as u8 + 20,
    }
}

fn placement(target: sophia_protocol::Rect) -> LiveCompositionPlacement {
    LiveCompositionPlacement {
        target,
        clip: None,
        transform: sophia_protocol::Transform::IDENTITY,
        alpha: 1.0,
        sampling: sophia_engine::HeadSamplingClass::Exact,
    }
}

/// A static CPU texture layer under the tiles, so the equivalence also covers
/// the textured draw path under an ambient clip, not only scissored clears.
fn cpu_backdrop() -> LiveSharedCpuBufferSource {
    let width = 96;
    let height = 64;
    let mut bytes = vec![0_u8; width * height * 4];
    for row in 0..height {
        for column in 0..width {
            let offset = (row * width + column) * 4;
            bytes[offset] = (column * 2) as u8;
            bytes[offset + 1] = (row * 3) as u8;
            bytes[offset + 2] = 96;
            bytes[offset + 3] = 255;
        }
    }
    LiveSharedCpuBufferSource {
        handle: 9,
        size: sophia_protocol::Size {
            width: width as i32,
            height: height as i32,
        },
        stride: (width * 4) as u32,
        format: 0x3432_5258,
        generation: 1,
        bytes: Arc::new(bytes).into(),
    }
}

fn frame_for(generations: [u64; 3]) -> LiveOwnedMixedCompositionFrame {
    let mut layers = vec![
        // Opaque background so every pixel of a full repaint is defined.
        LiveOwnedMixedCompositionLayer::Solid {
            geometry: sophia_protocol::Rect {
                x: 0,
                y: 0,
                width: WIDTH,
                height: HEIGHT,
            },
            color: sophia_engine::CompositorRgb8 {
                red: 16,
                green: 24,
                blue: 32,
            },
        },
        LiveOwnedMixedCompositionLayer::Cpu {
            buffer: cpu_backdrop(),
            placement: placement(sophia_protocol::Rect {
                x: 40,
                y: 96,
                width: 96,
                height: 64,
            }),
        },
    ];
    for (index, (tile, generation)) in TILES.iter().zip(generations).enumerate() {
        layers.push(LiveOwnedMixedCompositionLayer::Solid {
            geometry: *tile,
            color: generation_color(index, generation),
        });
    }
    LiveOwnedMixedCompositionFrame {
        layers,
        output_damage_snapshot: Some(snapshot(generations)),
        trace: None,
        direct_scanout: Default::default(),
    }
}

/// One tile advances per step, round-robin, so every frame has bounded damage
/// and slots are reacquired with content several generations old.
fn generations_at(step: usize) -> [u64; 3] {
    let mut generations = [1_u64; 3];
    for advanced in 0..step {
        generations[advanced % 3] += 1;
    }
    generations
}

struct Run {
    checksums: Vec<u64>,
    partial_frames: usize,
}

fn render_sequence(node: &std::path::Path, damage_enabled: bool) -> Run {
    let report =
        NativeGbmRenderedScanoutContext::from_backend_device_result(open_render_node(node));
    let mut context = report.context.expect("render context on a live node");
    context.force_composition_pixel_capture();
    let mut slot_damage = WorkerSlotDamage::with_enabled(damage_enabled);
    let target = LiveGbmEglFrameTargetRecord::new(sophia_protocol::Size {
        width: WIDTH,
        height: HEIGHT,
    });

    let mut checksums = Vec::with_capacity(FRAMES);
    let mut partial_frames = 0;
    for step in 0..FRAMES {
        let slot_index = step % SLOTS;
        let slot = LiveRendererFrameSlotId::from_index(slot_index).expect("slot index");
        let frame = frame_for(generations_at(step));
        let table =
            slot_damage.repaint_table(slot, frame.output_damage_snapshot.as_ref(), target.size);
        let report = context
            .export_owned_mixed_frame_with_modifiers_in_frame_slot(
                NativeFrameTargetSetId::DEFAULT,
                slot_index,
                target,
                &frame,
                &[],
                table.as_ref(),
            )
            .expect("mixed export on a live node");
        assert!(
            report.buffer.is_some(),
            "frame {step} did not export: {:?}",
            report.detail
        );
        if matches!(
            report.repaint,
            LiveNativeCompositionRepaintOutcome::Partial { .. }
        ) {
            partial_frames += 1;
        }
        slot_damage.settle(
            slot,
            report.buffer.is_some(),
            report.target_generation,
            frame.output_damage_snapshot.clone(),
        );
        let metrics = context
            .composition_pixel_metrics(NativeFrameTargetSetId::DEFAULT)
            .expect("forced capture reads every frame");
        checksums.push(metrics.checksum);
    }
    Run {
        checksums,
        partial_frames,
    }
}

#[test]
fn damage_limited_repaint_is_pixel_identical_to_full_repaint() {
    if std::env::var_os("SOPHIA_RUN_REAL_GBM_SMOKE").is_none() {
        return;
    }
    let Some(node) = first_openable_render_node() else {
        return;
    };

    let full = render_sequence(&node, false);
    let damage = render_sequence(&node, true);

    assert_eq!(
        full.partial_frames, 0,
        "the full-repaint control must never render partially"
    );
    for (step, (full_checksum, damage_checksum)) in
        full.checksums.iter().zip(&damage.checksums).enumerate()
    {
        assert_eq!(
            full_checksum, damage_checksum,
            "frame {step} diverged: a damage-limited repaint left different pixels than a full repaint"
        );
    }
    // Identical checksums prove nothing if no partial repaint ever ran. The
    // driver must support buffer age and the surface's swapchain must be no
    // deeper than the retained history for this to trigger; on the promoted
    // host it does, so a zero here is a regression, not an environment quirk.
    assert!(
        damage.partial_frames > 0,
        "no frame rendered partially, so the equivalence was vacuous"
    );
}

/// The comparison itself must be able to catch a stale region, or the
/// equivalence above proves nothing. A table that claims an aged buffer owes
/// nothing -- while a tile did change -- makes the renderer skip that tile's
/// repaint, and the checksums must diverge.
#[test]
fn a_lying_damage_table_is_caught_by_the_checksum() {
    if std::env::var_os("SOPHIA_RUN_REAL_GBM_SMOKE").is_none() {
        return;
    }
    let Some(node) = first_openable_render_node() else {
        return;
    };

    let full = render_sequence(&node, false);

    let report =
        NativeGbmRenderedScanoutContext::from_backend_device_result(open_render_node(&node));
    let mut context = report.context.expect("render context on a live node");
    context.force_composition_pixel_capture();
    let target = LiveGbmEglFrameTargetRecord::new(sophia_protocol::Size {
        width: WIDTH,
        height: HEIGHT,
    });

    // Claim that a buffer of any retained age owes nothing at all.
    let lying_table =
        sophia_renderer_live::NativeCompositionRepaintTable::from_ages(vec![Some(Vec::new()); 8]);
    let mut lied = false;
    let mut diverged = false;
    for step in 0..FRAMES {
        let slot_index = step % SLOTS;
        let frame = frame_for(generations_at(step));
        // Warm each slot once with a full render so the lie has an aged buffer
        // to leave stale; lie on every render after that.
        let table = (step >= SLOTS).then_some(&lying_table);
        let report = context
            .export_owned_mixed_frame_with_modifiers_in_frame_slot(
                NativeFrameTargetSetId::DEFAULT,
                slot_index,
                target,
                &frame,
                &[],
                table,
            )
            .expect("mixed export on a live node");
        assert!(report.buffer.is_some());
        if matches!(
            report.repaint,
            LiveNativeCompositionRepaintOutcome::Partial { .. }
        ) {
            lied = true;
        }
        let metrics = context
            .composition_pixel_metrics(NativeFrameTargetSetId::DEFAULT)
            .expect("forced capture reads every frame");
        if metrics.checksum != full.checksums[step] {
            diverged = true;
        }
    }
    assert!(
        lied,
        "the lying table was never consulted, so nothing was tested"
    );
    assert!(
        diverged,
        "a repaint that skipped changed tiles produced identical pixels, so the checksum cannot catch a stale region"
    );
}
