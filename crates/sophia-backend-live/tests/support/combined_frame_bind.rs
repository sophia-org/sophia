//! G3-2: the renderer binds a combined job's validated pixels only after it
//! holds a slot, checks them against the job's admitted description first,
//! and reads the job's stop flag at dequeue and at every retry
//! (GATE3-SCOPE.md G3-2 and its source decisions). The slots are the
//! production `CombinedSlots`, a worker output's own pool and damage history,
//! seeded with a usable history; the render function is the deterministic
//! seam that production fills with `render_frame_in_slot`. No GPU.

use super::super::super::WorkerSlotDamage;
use super::super::super::combined::{
    CombinedPlaceholder, CombinedSlots, RenderInput, RenderTarget, admits, attempt, bind,
};
use super::super::super::service::WorkerOutputState;
use super::super::{correlated_facade, correlated_result, exported_outcome};
use super::{
    CombinedOutcome, CombinedRenderPlan, RenderAttempt, WAIT, WorkerOutcome, WorkerResult,
};
use crate::api::LiveGbmEglFrameTargetRecord;
use crate::{
    LiveRendererFrameSlotAcquire, LiveRendererFrameSlotId, LiveRendererFrameSlotRelease,
    LiveRendererFrameSlotToken,
};
use sophia_protocol::{
    ContentGrant, ContentResourceBegin, ContentResourceId, DRM_FORMAT_ARGB8888, Rect, Size,
};
use sophia_renderer_live::{
    LiveCompositionPlacement, LiveCpuPixelStorage, LiveOwnedMixedCompositionFrame,
    LiveOwnedMixedCompositionLayer, LiveRendererScanoutBufferExportDetail,
    LiveSharedCpuBufferSource,
};
use sophia_runtime::{CpuPixelValidation, ValidatedIsbPixels};
use std::io::Write;
use std::os::fd::OwnedFd;
use std::sync::{Arc, Weak};
use std::time::Instant;

// The stop flag, the owner's view of a mismatch and the ring description,
// through the real route.
#[path = "combined_frame_bind_serve.rs"]
mod serve;

/// Eight valid premultiplied pixels (no colour above its alpha), distinct so
/// a checksum tells one occurrence's bytes from another's.
const BYTES: [u8; 32] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32,
];

fn validated(bytes: &[u8]) -> ValidatedIsbPixels {
    let mut scan = CpuPixelValidation::from_owned(bytes.into()).unwrap();
    while !scan.step().unwrap() {}
    scan.finish().unwrap()
}

fn sealed(bytes: &[u8]) -> OwnedFd {
    use rustix::fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create};
    let mut file = std::fs::File::from(
        memfd_create(
            "combined-bind",
            MemfdFlags::CLOEXEC | MemfdFlags::ALLOW_SEALING,
        )
        .unwrap(),
    );
    file.write_all(bytes).unwrap();
    fcntl_add_seals(
        &file,
        SealFlags::WRITE | SealFlags::GROW | SealFlags::SHRINK | SealFlags::SEAL,
    )
    .unwrap();
    file.into()
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(17, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(u64::from(*byte))
    })
}

fn placement(x: i32, clip: Option<Rect>) -> LiveCompositionPlacement {
    LiveCompositionPlacement {
        target: Rect {
            x,
            y: 0,
            width: 2,
            height: 4,
        },
        clip,
        transform: sophia_protocol::Transform::IDENTITY,
        alpha: 1.0,
        sampling: sophia_engine::HeadSamplingClass::Exact,
    }
}

/// A placeholder for the admitted 2x4 ARGB description, at a final index.
fn occurrence(index: usize, node: u16, x: i32) -> CombinedPlaceholder {
    CombinedPlaceholder {
        index,
        node: sophia_engine::CompositorNodeId::ShellContent {
            grant: ContentGrant {
                connection_epoch: 1,
                content_grant_epoch: 1,
            },
            output: sophia_protocol::OutputId::from_raw(7),
            candidate: 1,
            surface: 0,
            placement: node,
        },
        placement: placement(
            x,
            Some(Rect {
                x,
                y: 1,
                width: 2,
                height: 2,
            }),
        ),
        size: Size {
            width: 2,
            height: 4,
        },
        stride: 8,
        format: DRM_FORMAT_ARGB8888,
        handle: 77,
        generation: 9,
    }
}

fn description(width_px: u32, height_px: u32, total_bytes: u64) -> ContentResourceBegin {
    ContentResourceBegin {
        grant: ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        },
        resource: ContentResourceId {
            id: 77,
            generation: 9,
        },
        width_px,
        height_px,
        rendered_scale_numerator: 1,
        rendered_scale_denominator: 1,
        pixel_format: 1,
        chunk_count: 1,
        total_bytes,
    }
}

/// A retained Cpu layer the owner lowered from its own resident image.
fn retained(handle: u64) -> LiveOwnedMixedCompositionLayer {
    LiveOwnedMixedCompositionLayer::Cpu {
        buffer: LiveSharedCpuBufferSource {
            pixel_version: std::num::NonZeroU64::new(handle),
            handle,
            size: Size {
                width: 1,
                height: 1,
            },
            stride: 4,
            format: DRM_FORMAT_ARGB8888,
            generation: 1,
            bytes: LiveCpuPixelStorage::Shared(Arc::new(vec![0, 0, 0, 255])),
        },
        placement: placement(100, None),
    }
}

fn frame(handles: &[u64]) -> super::super::super::PendingRenderedFrame {
    super::super::super::PendingRenderedFrame::Mixed(
        LiveOwnedMixedCompositionFrame {
            layers: handles.iter().map(|&handle| retained(handle)).collect(),
            ..Default::default()
        },
        None,
    )
}

fn layers(frame: &super::super::super::PendingRenderedFrame) -> &[LiveOwnedMixedCompositionLayer] {
    match frame {
        super::super::super::PendingRenderedFrame::Mixed(mixed, _) => &mixed.layers,
        _ => panic!("the fixture frame is Mixed"),
    }
}

fn plan(handles: &[u64], placeholders: Vec<CombinedPlaceholder>) -> CombinedRenderPlan {
    CombinedRenderPlan::new(
        Instant::now() + WAIT,
        LiveGbmEglFrameTargetRecord::new(Size {
            width: 16,
            height: 16,
        }),
        frame(handles),
        Vec::new(),
        None,
        placeholders.into_boxed_slice(),
    )
}

/// One bound occurrence as the deterministic renderer read it.
#[derive(Debug, Eq, PartialEq)]
struct Seen {
    index: usize,
    handle: u64,
    checksum: u64,
}

/// What a layer is, for order assertions: a retained handle or a bound one.
fn kinds(layers: &[LiveOwnedMixedCompositionLayer]) -> Vec<(bool, u64)> {
    layers
        .iter()
        .map(|layer| match layer {
            LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } => (
                matches!(buffer.bytes, LiveCpuPixelStorage::Validated(_)),
                buffer.handle,
            ),
            _ => panic!("the fixture has Cpu layers only"),
        })
        .collect()
}

fn validated_arc(layer: &LiveOwnedMixedCompositionLayer) -> Option<&Arc<ValidatedIsbPixels>> {
    match layer {
        LiveOwnedMixedCompositionLayer::Cpu {
            buffer:
                LiveSharedCpuBufferSource {
                    bytes: LiveCpuPixelStorage::Validated(pixels),
                    ..
                },
            ..
        } => Some(pixels),
        _ => None,
    }
}

const DAMAGE_SIZE: Size = Size {
    width: 64,
    height: 32,
};

fn slot(index: usize) -> LiveRendererFrameSlotId {
    LiveRendererFrameSlotId::from_index(index).unwrap()
}

fn damage_snapshot() -> sophia_engine::OutputFrameDamageSnapshot {
    let output = sophia_engine::HeadlessOutput {
        id: sophia_protocol::OutputId::from_raw(1),
        size: DAMAGE_SIZE,
        scale: 1,
    };
    let list = sophia_engine::CompositorDisplayList {
        output: output.id,
        commands: Vec::new(),
    };
    sophia_engine::output_frame_damage_snapshot(output, list, &[], None).unwrap()
}

/// A worker output's production state, every slot holding one recorded
/// export, so an invalidation is visible in its history and its counter.
pub(super) fn production() -> WorkerOutputState {
    let (facade, _commands, reply) = correlated_facade();
    let mut state = WorkerOutputState::new(reply, Default::default(), Arc::clone(&facade.claim));
    // The production default reads one opt-out variable; tests pin it on.
    state.slot_damage = WorkerSlotDamage::with_enabled(true);
    for index in 0..3 {
        state
            .slot_damage
            .settle(slot(index), true, Some(7), Some(damage_snapshot()));
    }
    assert_eq!(state.slot_damage.metrics().records, 3);
    assert!((0..3).all(|index| history_kept(&mut state, index)));
    assert_eq!(state.slot_damage.metrics().invalidations, 0);
    state
}

/// Whether the slot still has a history to repaint an age-one buffer against.
pub(super) fn history_kept(state: &mut WorkerOutputState, index: usize) -> bool {
    let table = state
        .slot_damage
        .repaint_table(slot(index), Some(&damage_snapshot()), DAMAGE_SIZE)
        .unwrap();
    table.full_reason_for_age(1) != Some(sophia_renderer_live::NativeFullRepaintReason::NoHistory)
}

/// A pool whose one forced answer is an exhausted incarnation; its releases
/// are the production ones.
struct Exhausted(WorkerOutputState);

impl CombinedSlots for Exhausted {
    fn try_acquire(&mut self) -> LiveRendererFrameSlotAcquire {
        // shortcut: the real pool's u64 incarnations cannot be exhausted in a
        // test, so this one answer is forced.
        LiveRendererFrameSlotAcquire::IncarnationExhausted
    }

    fn release_unused(&mut self, token: LiveRendererFrameSlotToken) {
        self.0.release_unused(token);
    }

    fn release_failed(&mut self, token: LiveRendererFrameSlotToken) {
        self.0.release_failed(token);
    }
}

/// Production returned the slot untouched: free, counted once, every
/// history kept.
fn assert_returned_untouched(state: &mut WorkerOutputState) {
    assert_eq!(state.frame_slots.leased(), 0, "the slot went back");
    assert_eq!(state.frame_slots.metrics().acquisitions, 1);
    assert_eq!(
        state.slot_damage.metrics().invalidations,
        0,
        "an untouched slot keeps its damage history"
    );
    assert!((0..3).all(|index| history_kept(state, index)));
}

fn result(outcome: WorkerOutcome) -> WorkerResult {
    correlated_result(
        super::super::super::frame_correlation(
            &frame(&[]),
            Some(super::super::super::combined::COMBINED_REQUEST),
        ),
        outcome,
    )
}

/// The deterministic renderer: reads every bound occurrence's bytes, then
/// exports into the slot it was handed.
fn render_and_record<S>(
    seen: &mut Vec<Seen>,
    counts: &mut Vec<usize>,
) -> impl FnOnce(&mut S, RenderInput, RenderTarget) -> WorkerResult {
    move |_, input, _| {
        let RenderInput::Bound(bound, token) = input else {
            panic!("an admitted job renders bound");
        };
        for (index, layer) in layers(bound.frame()).iter().enumerate() {
            if let Some(pixels) = validated_arc(layer) {
                let LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } = layer else {
                    unreachable!();
                };
                seen.push(Seen {
                    index,
                    handle: buffer.handle,
                    checksum: checksum(&buffer.bytes),
                });
                counts.push(Arc::strong_count(pixels));
            }
        }
        let mut outcome = exported_outcome();
        let WorkerOutcome::Exported { slot_token, .. } = &mut outcome else {
            unreachable!();
        };
        *slot_token = token;
        result(outcome)
    }
}

fn done(attempted: RenderAttempt) -> (CombinedOutcome, ValidatedIsbPixels) {
    match attempted {
        RenderAttempt::Done(outcome, pixels) => (outcome, pixels),
        RenderAttempt::NoSlot(..) => panic!("a free slot was expected"),
    }
}

#[test]
fn bind_fills_interleaved_and_repeated_occurrences_in_order() {
    let pixels = Arc::new(validated(&BYTES));
    let placeholders = [
        occurrence(0, 1, 0),
        occurrence(2, 2, 4),
        occurrence(3, 3, 8),
    ];
    let Ok(bound) = bind(frame(&[100, 101]), &placeholders, &pixels) else {
        panic!("every index fits five final layers");
    };
    let bound_layers = layers(bound.frame());
    assert_eq!(
        kinds(bound_layers),
        [
            (true, 77),
            (false, 100),
            (true, 77),
            (true, 77),
            (false, 101)
        ],
        "bound occurrences interleave with retained layers in final order"
    );
    for placeholder in &placeholders {
        let LiveOwnedMixedCompositionLayer::Cpu { buffer, placement } =
            &bound_layers[placeholder.index]
        else {
            panic!("a bound occurrence is a Cpu layer");
        };
        assert_eq!(buffer.pixel_version, None, "a bound layer always uploads");
        assert_eq!(
            (
                buffer.handle,
                buffer.size,
                buffer.stride,
                buffer.format,
                buffer.generation
            ),
            (
                placeholder.handle,
                placeholder.size,
                placeholder.stride,
                placeholder.format,
                placeholder.generation
            ),
            "descriptor fields come from the placeholder"
        );
        assert_eq!(
            (placement.target, placement.clip),
            (placeholder.placement.target, placeholder.placement.clip),
            "placement and clip come from the placeholder"
        );
        assert_eq!(&*buffer.bytes, BYTES.as_slice());
        assert!(
            std::ptr::eq(buffer.bytes.as_ptr(), pixels.bytes().as_ptr()),
            "every occurrence reads the one validated backing"
        );
    }
    for (index, handle) in [(1, 100), (4, 101)] {
        let LiveOwnedMixedCompositionLayer::Cpu { buffer, .. } = &bound_layers[index] else {
            panic!("retained layers keep their kind");
        };
        assert_eq!(buffer.pixel_version, std::num::NonZeroU64::new(handle));
    }
    assert_eq!(
        Arc::strong_count(&pixels),
        4,
        "three occurrences share the caller's Arc"
    );
    drop(bound);
    assert_eq!(Arc::strong_count(&pixels), 1);
}

#[test]
fn bind_returns_everything_unbound_when_any_occurrence_cannot_be_filled() {
    let pixels = Arc::new(validated(&BYTES));
    for indices in [[0, 3], [1, 0], [0, 0]] {
        let placeholders = indices.map(|index| occurrence(index, 1, 0));
        let Err(unbound) = bind(frame(&[100]), &placeholders, &pixels) else {
            panic!("{indices:?} cannot fill one retained layer");
        };
        assert_eq!(
            kinds(layers(&unbound)),
            [(false, 100)],
            "{indices:?}: the input frame comes back unchanged"
        );
        assert_eq!(
            Arc::strong_count(&pixels),
            1,
            "{indices:?}: no clone of the pixels survives a refused bind"
        );
    }
}

#[test]
fn admits_checks_extent_outside_the_loop_and_shape_per_occurrence() {
    let described = description(2, 4, 32);
    assert!(admits(&[occurrence(0, 1, 0)], Some(&described), 32));
    assert!(admits(&[], Some(&described), 32));
    assert!(
        !admits(&[], Some(&described), 24),
        "the byte extent is checked with no placeholders"
    );
    let mut wide = described.clone();
    wide.width_px = u32::MAX;
    assert!(
        !admits(&[occurrence(0, 1, 0)], Some(&wide), 32),
        "a stride that overflows is a mismatch"
    );
    let mut other_format = described.clone();
    other_format.pixel_format = 2;
    assert!(!admits(&[occurrence(0, 1, 0)], Some(&other_format), 32));
    let mut xrgb = occurrence(0, 1, 0);
    xrgb.format = sophia_protocol::DRM_FORMAT_XRGB8888;
    assert!(
        !admits(&[occurrence(0, 1, 0), xrgb], Some(&described), 32),
        "every occurrence is checked, not only the first"
    );
}

#[test]
fn the_renderer_returns_the_exact_pixels_with_a_strong_count_of_one() {
    let pixels = validated(&BYTES);
    let address = pixels.bytes().as_ptr();
    let mut slots = production();
    let (mut seen, mut counts) = (Vec::new(), Vec::new());
    let (outcome, pixels) = done(attempt(
        &mut slots,
        plan(&[100], vec![occurrence(0, 1, 0), occurrence(2, 2, 4)]),
        pixels,
        Some(&description(2, 4, 32)),
        render_and_record(&mut seen, &mut counts),
    ));
    assert_eq!(
        counts,
        [3, 3],
        "the attempt's Arc and one per bound occurrence while rendering"
    );
    assert!(matches!(
        outcome,
        CombinedOutcome::Rendered(WorkerResult {
            outcome: WorkerOutcome::Exported { .. },
            ..
        })
    ));
    assert!(
        std::ptr::eq(pixels.bytes().as_ptr(), address),
        "the same backing comes home, never a copy"
    );
    assert_eq!(pixels.bytes(), BYTES.as_slice());
    assert_eq!(
        slots.frame_slots.leased(),
        1,
        "an exported slot stays leased"
    );
    assert_eq!(slots.frame_slots.metrics().acquisitions, 1);
}

#[test]
fn the_deterministic_renderer_saw_every_bound_occurrence() {
    let mut slots = production();
    let (mut seen, mut counts) = (Vec::new(), Vec::new());
    let _ = done(attempt(
        &mut slots,
        plan(&[100, 101], vec![occurrence(1, 1, 0), occurrence(3, 2, 4)]),
        validated(&BYTES),
        Some(&description(2, 4, 32)),
        render_and_record(&mut seen, &mut counts),
    ));
    let expected = checksum(&BYTES);
    assert_eq!(
        seen,
        [
            Seen {
                index: 1,
                handle: 77,
                checksum: expected,
            },
            Seen {
                index: 3,
                handle: 77,
                checksum: expected,
            },
        ],
        "both occurrences, in final order, read the validated bytes"
    );
}

#[test]
fn three_busy_slots_park_the_job_unbound_and_one_release_renders_it_once() {
    let mut slots = production();
    let busy: Vec<_> = (0..3)
        .map(|_| match slots.frame_slots.try_acquire() {
            LiveRendererFrameSlotAcquire::Acquired(token) => token,
            _ => panic!("an empty pool leases three slots"),
        })
        .collect();
    let mut rendered = 0;
    let RenderAttempt::NoSlot(parked, pixels) = attempt(
        &mut slots,
        plan(&[100], vec![occurrence(0, 1, 0)]),
        validated(&BYTES),
        Some(&description(2, 4, 32)),
        |_, _, _| {
            rendered += 1;
            result(WorkerOutcome::Failed(
                LiveRendererScanoutBufferExportDetail::ComposeRefused,
            ))
        },
    ) else {
        panic!("no free slot defers");
    };
    assert_eq!(rendered, 0);
    assert_eq!(pixels.bytes(), BYTES.as_slice());
    assert_eq!(slots.frame_slots.metrics().deferrals, 1);

    assert_eq!(
        slots.frame_slots.release(busy[0]),
        LiveRendererFrameSlotRelease::Released
    );
    let (mut seen, mut counts) = (Vec::new(), Vec::new());
    let _ = done(attempt(
        &mut slots,
        parked,
        pixels,
        Some(&description(2, 4, 32)),
        render_and_record(&mut seen, &mut counts),
    ));
    assert_eq!(
        seen.iter().map(|seen| seen.index).collect::<Vec<_>>(),
        [0],
        "the released slot renders it once, its one occurrence bound"
    );
    assert_eq!(slots.frame_slots.metrics().acquisitions, 4);
    assert_eq!(slots.frame_slots.metrics().deferrals, 1);
}

#[test]
fn an_equal_length_different_description_fails_terminally_unbound() {
    let mut slots = production();
    let mut tall = occurrence(0, 1, 0);
    tall.size = Size {
        width: 4,
        height: 2,
    };
    tall.stride = 16;
    let (outcome, pixels) = done(attempt(
        &mut slots,
        plan(&[], vec![tall]),
        validated(&BYTES),
        Some(&description(2, 4, 32)),
        |_, _, _| panic!("a mismatch renders nothing"),
    ));
    assert!(
        matches!(outcome, CombinedOutcome::Mismatch),
        "4x2 is not 2x4"
    );
    assert_eq!(pixels.bytes(), BYTES.as_slice());
    assert_returned_untouched(&mut slots);
}

#[test]
fn a_byte_extent_different_from_the_admitted_total_fails_terminally_unbound() {
    for placeholders in [vec![occurrence(0, 1, 0)], Vec::new()] {
        let mut slots = production();
        let (outcome, pixels) = done(attempt(
            &mut slots,
            plan(&[], placeholders),
            validated(&BYTES[..24]),
            Some(&description(2, 4, 32)),
            |_, _, _| panic!("a mismatch renders nothing"),
        ));
        assert!(matches!(outcome, CombinedOutcome::Mismatch));
        assert_eq!(pixels.bytes(), &BYTES[..24]);
        assert_returned_untouched(&mut slots);
    }
}

#[test]
fn a_job_without_a_description_binds_nothing_and_any_placeholder_is_a_mismatch() {
    let mut slots = production();
    let (outcome, _) = done(attempt(
        &mut slots,
        plan(&[100], Vec::new()),
        validated(&BYTES),
        None,
        render_and_record(&mut Vec::new(), &mut Vec::new()),
    ));
    assert!(
        matches!(outcome, CombinedOutcome::Rendered(_)),
        "a sealed job with no placeholder renders as planned"
    );
    let mut slots = production();
    let (outcome, pixels) = done(attempt(
        &mut slots,
        plan(&[100], vec![occurrence(0, 1, 0)]),
        validated(&BYTES),
        None,
        |_, _, _| panic!("no description, no bound occurrence"),
    ));
    assert!(matches!(outcome, CombinedOutcome::Mismatch));
    assert_eq!(pixels.bytes(), BYTES.as_slice());
    assert_returned_untouched(&mut slots);
}

#[test]
fn an_exhausted_incarnation_returns_the_pixels_unbound() {
    let mut slots = Exhausted(production());
    let (outcome, pixels) = done(attempt(
        &mut slots,
        plan(&[100], vec![occurrence(0, 1, 0)]),
        validated(&BYTES),
        Some(&description(2, 4, 32)),
        |_, input, _| {
            let RenderInput::Failed(detail) = input else {
                panic!("an exhausted slot table renders nothing");
            };
            result(WorkerOutcome::Failed(detail))
        },
    ));
    assert!(matches!(
        outcome,
        CombinedOutcome::Rendered(WorkerResult {
            outcome: WorkerOutcome::Failed(
                LiveRendererScanoutBufferExportDetail::FrameSlotIncarnationExhausted
            ),
            ..
        })
    ));
    assert_eq!(pixels.bytes(), BYTES.as_slice());
    assert_eq!(slots.0.frame_slots.leased(), 0, "no slot was taken");
    assert_eq!(slots.0.slot_damage.metrics().invalidations, 0);
}

#[test]
fn a_failed_render_releases_and_invalidates_its_slot() {
    let mut slots = production();
    let mut used = None;
    let (outcome, pixels) = done(attempt(
        &mut slots,
        plan(&[], vec![occurrence(0, 1, 0)]),
        validated(&BYTES),
        Some(&description(2, 4, 32)),
        |_, input, _| {
            let RenderInput::Bound(_, token) = input else {
                panic!("an admitted job renders bound");
            };
            used = Some(token.slot_id().index());
            result(WorkerOutcome::Failed(
                LiveRendererScanoutBufferExportDetail::BackendDeviceUnavailable,
            ))
        },
    ));
    assert!(matches!(outcome, CombinedOutcome::Rendered(_)));
    assert_eq!(pixels.bytes(), BYTES.as_slice());
    let used = used.expect("the render was handed its slot");
    assert_eq!(slots.frame_slots.leased(), 0, "the failed slot went back");
    assert_eq!(
        slots.slot_damage.metrics().invalidations,
        1,
        "a failed render invalidates its slot's history"
    );
    for index in 0..3 {
        assert_eq!(history_kept(&mut slots, index), index != used);
    }
}

#[test]
fn a_bind_fault_is_compose_refused_and_releases_the_untouched_slot() {
    let mut slots = production();
    let (outcome, pixels) = done(attempt(
        &mut slots,
        plan(&[100], vec![occurrence(0, 1, 0), occurrence(3, 2, 4)]),
        validated(&BYTES),
        Some(&description(2, 4, 32)),
        |_, input, _| {
            let RenderInput::Failed(detail) = input else {
                panic!("an unbound plan never renders");
            };
            result(WorkerOutcome::Failed(detail))
        },
    ));
    assert!(matches!(
        outcome,
        CombinedOutcome::Rendered(WorkerResult {
            outcome: WorkerOutcome::Failed(LiveRendererScanoutBufferExportDetail::ComposeRefused),
            ..
        })
    ));
    assert_eq!(pixels.bytes(), BYTES.as_slice());
    assert_returned_untouched(&mut slots);
}

#[test]
#[should_panic(expected = "stopping condition 1")]
fn a_reference_that_outlives_the_render_stops_the_worker() {
    let mut kept = Vec::new();
    let _ = attempt(
        &mut production(),
        plan(&[], vec![occurrence(0, 1, 0)]),
        validated(&BYTES),
        Some(&description(2, 4, 32)),
        |_, input, _| {
            let RenderInput::Bound(bound, _) = input else {
                panic!("an admitted job renders bound");
            };
            kept.extend(validated_arc(&layers(bound.frame())[0]).cloned());
            result(WorkerOutcome::Failed(
                LiveRendererScanoutBufferExportDetail::ComposeRefused,
            ))
        },
    );
}

#[test]
fn unwinding_out_of_the_render_drops_every_bound_reference() {
    let mut watch: Option<Weak<ValidatedIsbPixels>> = None;
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        attempt(
            &mut production(),
            plan(&[], vec![occurrence(0, 1, 0), occurrence(1, 2, 4)]),
            validated(&BYTES),
            Some(&description(2, 4, 32)),
            |_, input, _| {
                let RenderInput::Bound(bound, _) = input else {
                    panic!("an admitted job renders bound");
                };
                watch = validated_arc(&layers(bound.frame())[0]).map(Arc::downgrade);
                panic!("the render unwinds");
            },
        )
    }));
    assert!(unwound.is_err());
    let watch = watch.expect("the render saw a bound occurrence");
    assert_eq!(
        watch.strong_count(),
        0,
        "unwinding drops the frame and the attempt's Arc: memory only"
    );
}
