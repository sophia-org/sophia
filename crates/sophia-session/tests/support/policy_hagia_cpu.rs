//! Real CPU production cycle below the real Hagia/Session layout settlement.
//! Historical admission and frontend ACKs remain supplied fixture inputs.
//! Pending checks use empty cycles without competing content. No queued
//! Present or deferred-Present rejection is exercised by these plain CPU batches.
use super::*;
use sophia_backend_live::{
    LiveProductionCursorPresentation, LiveProductionCycleRequest, LiveProductionVisualRuntime,
};
use sophia_renderer_live::LiveProductionCpuScene;
use sophia_x_authority::{
    XAuthorityCpuBufferSnapshot, XAuthorityCpuBufferUpdate, XAuthorityObservedTransactionBatch,
    XResourceId,
};
use std::sync::Arc;

const OLD_RGB: [u8; 4] = [0x19, 0x53, 0x91, 0xff];
const NEW_RGB: [u8; 4] = [0x71, 0x31, 0xb7, 0xff];
const OLD_CONTENT_GENERATION: u64 = 11;
const NEW_CONTENT_GENERATION: u64 = 23;

fn cpu_batch(
    transaction: u64,
    handle: u64,
    previous: u64,
    content_generation: u64,
    geometry: Rect,
    rgba: [u8; 4],
) -> XAuthorityObservedTransactionBatch {
    let size = Size {
        width: geometry.width,
        height: geometry.height,
    };
    let bytes = Arc::new(rgba.repeat(usize::try_from(size.width * size.height).unwrap()));
    let transaction = TransactionId::from_raw(transaction);
    let mut batch = crate::live_session::wm_update_coordinator_batch(transaction);
    batch.client = Some(sophia_x_authority::XServerFrontendClientId::from_raw(1));
    batch.transactions.push(SurfaceTransaction {
        transaction,
        authority: AuthorityKind::SophiaX,
        surface: SURFACE,
        namespace: None,
        target_geometry: geometry,
        presentation_extent: size,
        content: SurfaceContentSet::singleton(BufferSource::CpuBuffer { handle }, size),
        damage: Region::single(Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        }),
        readiness: SurfaceTransactionReadiness::Ready,
        timeout_msec: 250,
        previous_committed_generation: previous,
        input_region: None,
    });
    batch
        .cpu_buffer_updates
        .push(XAuthorityCpuBufferUpdate::Replace(
            XAuthorityCpuBufferSnapshot {
                handle,
                drawable: XResourceId::new(3, 1),
                size,
                stride: u32::try_from(size.width * 4).unwrap(),
                format: sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888,
                generation: content_generation,
                bytes,
            },
        ));
    // No synthetic Present completion, software Present or native receipt.
    batch
}

struct CpuOwner {
    runtime: LiveProductionVisualRuntime,
    scene: LiveProductionCpuScene,
    output: sophia_engine::HeadlessOutput,
}

impl CpuOwner {
    fn new(output: sophia_engine::HeadlessOutput) -> Self {
        Self {
            runtime: LiveProductionVisualRuntime::new(&[output], None).unwrap(),
            scene: LiveProductionCpuScene::new(output.size),
            output,
        }
    }

    fn cycle(
        &mut self,
        wm: &LiveWmSession,
        layout: &mut PersistentLiveLayout,
        batch: &XAuthorityObservedTransactionBatch,
        update: Option<sophia_engine::WmTransactionUpdate>,
    ) -> bool {
        // Owner order: owner_loop/authority.rs -> layout.projected_batch and
        // presentation.rs::production_authority_batch; authority_production.rs
        // applies commit/abort then invokes backend cpu_cycle.rs. This fixture
        // calls those owners; it does not execute the whole owner-loop macros.
        let mut staged = Vec::new();
        layout.write_pending_cpu_buffer_handles(&mut staged);
        let (projected, released) = layout.projected_batch(batch);
        assert!(
            released.is_empty(),
            "historical managed baseline has no admission release"
        );
        assert_eq!(projected.transactions.len(), batch.transactions.len());
        for (source, projected) in batch.transactions.iter().zip(&projected.transactions) {
            assert_eq!(
                projected.previous_committed_generation,
                source.previous_committed_generation
            );
            assert_eq!(projected.content, source.content);
            assert_eq!(projected.readiness, source.readiness);
            assert_eq!(projected.transaction, source.transaction);
        }
        let production =
            crate::live_session::production_authority_batch(&projected, &released, layout).unwrap();
        if let Some(update) = &update {
            match update.commit.outcome {
                TransactionOutcome::Committed => {
                    self.runtime.commit_layout_epoch(update.commit.transaction);
                }
                TransactionOutcome::TimedOut => {
                    self.runtime.abort_layout_epoch(update.commit.transaction);
                }
                _ => panic!("fixture only covers committed and timed-out actual WM results"),
            }
        }
        if layout.pending.is_none() {
            self.runtime.release_layout_deferred_presentations();
        }
        let layers = layout
            .layers
            .values()
            .filter(|layer| {
                wm.surface_visible_on_any_output(layer.surface, &[self.output])
                    .unwrap()
            })
            .cloned()
            .collect::<Vec<_>>();
        let chrome = layers.iter().map(|layer| layer.surface).collect::<Vec<_>>();
        let (submission, committed, _) = self
            .runtime
            .run_cpu_production_cycle(LiveProductionCycleRequest {
                batch: &production,
                scene: &mut self.scene,
                raised_surface: None,
                focused_surface: None,
                cursor_presentation: LiveProductionCursorPresentation::Software(None),
                defer_frame: false,
                output_descriptors: &[self.output],
                native_scanout: None,
                wm_update: update,
                presentation_layout: &layers,
                geometry_routed_surfaces: &[],
                chrome_surfaces: &chrome,
                indicator_publication: None,
                staged_cpu_buffer_handles: &staged,
            })
            .unwrap();
        assert_eq!(committed, self.runtime.committed_surfaces());
        assert_eq!(self.runtime.output_committed(0).unwrap(), committed);
        submission.composed
    }

    fn assert_source(
        &self,
        geometry: Rect,
        generation: u64,
        handle: u64,
        content_generation: u64,
        rgba: [u8; 4],
    ) {
        let committed = self.runtime.committed_surfaces();
        assert_eq!(committed.len(), 1);
        assert_eq!(committed[0].geometry, geometry);
        assert_eq!(committed[0].committed_generation, generation);
        assert_eq!(committed[0].buffer(), BufferSource::CpuBuffer { handle });
        let sources = self.scene.presentation_layers(committed, &[SURFACE]);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].buffer.handle, handle);
        assert_eq!(sources[0].buffer.generation, content_generation);
        assert!(
            sources[0]
                .buffer
                .bytes
                .chunks_exact(4)
                .all(|pixel| pixel == rgba)
        );
    }
}

// DRM XRGB8888 is the little-endian 0x00RRGGBB word: bytes B,G,R,X.
// Match the renderer's solid.rs/scaled.rs interpretation; ignore X padding.
fn rgb_at(frame: &sophia_renderer_live::LiveCpuComposedFrame, x: i32, y: i32) -> u32 {
    assert_eq!(
        frame.format,
        sophia_renderer_live::LIVE_RENDERER_SCANOUT_FORMAT_XRGB8888
    );
    assert!((0..frame.size.width).contains(&x) && (0..frame.size.height).contains(&y));
    let offset = usize::try_from(y).unwrap() * usize::try_from(frame.stride).unwrap()
        + usize::try_from(x).unwrap() * 4;
    u32::from_le_bytes(frame.bytes[offset..offset + 4].try_into().unwrap()) & 0x00ff_ffff
}

fn assert_composed_regions(
    frame: &sophia_renderer_live::LiveCpuComposedFrame,
    target: Rect,
    old: Rect,
) -> usize {
    // Stay away from the chrome edge. The probe is bounded to 16x16 actual
    // pixels at the center of the visible interior, rather than a frame-wide
    // search which could accidentally find an unrelated copy of the color.
    let left = (target.x + 8).max(0);
    let top = (target.y + 8).max(0);
    let right = (target.x + target.width - 8).min(frame.size.width);
    let bottom = (target.y + target.height - 8).min(frame.size.height);
    assert!(right - left >= 16 && bottom - top >= 16);
    let x0 = left + (right - left - 16) / 2;
    let y0 = top + (bottom - top - 16) / 2;
    let expected = u32::from_le_bytes(NEW_RGB) & 0x00ff_ffff;
    for y in y0..y0 + 16 {
        for x in x0..x0 + 16 {
            assert_eq!(
                rgb_at(frame, x, y),
                expected,
                "successor interior ({x},{y})"
            );
        }
    }
    let old_color = u32::from_le_bytes(OLD_RGB) & 0x00ff_ffff;
    assert!(old.width * old.height <= 160 * 120);
    let mut old_only = 0;
    for y in old.y.max(0)..(old.y + old.height).min(frame.size.height) {
        for x in old.x.max(0)..(old.x + old.width).min(frame.size.width) {
            if x < target.x
                || x >= target.x + target.width
                || y < target.y
                || y >= target.y + target.height
            {
                old_only += 1;
                assert_ne!(
                    rgb_at(frame, x, y),
                    old_color,
                    "old-only pixel remains ({x},{y})"
                );
            }
        }
    }
    old_only
}

fn cpu_join(timeout_action: bool) {
    let case = if timeout_action {
        "cpu-timeout"
    } else {
        "cpu-commit"
    };
    with_normal_hagia(case, |wm, layout, _, output, checkpoint, identity| {
        let peer = wm.supervisor.peer_id();
        retain_existing_surface(layout);
        let old = layout.layers[&SURFACE].geometry;
        let mut cpu = CpuOwner::new(output);
        let baseline = cpu_batch(700, 700, 0, OLD_CONTENT_GENERATION, old, OLD_RGB);
        cpu.cycle(wm, layout, &baseline, None);
        cpu.assert_source(old, 1, 700, OLD_CONTENT_GENERATION, OLD_RGB);
        // Independently supplied Session history agrees with the actual
        // backend baseline committed from batch700; it is not backend output
        // installed into Session and is not original admission evidence.
        let retained = &layout.layers[&SURFACE];
        let committed = &cpu.runtime.committed_surfaces()[0];
        assert_eq!(retained.geometry, committed.geometry);
        assert_eq!(retained.source, committed.buffer());
        assert_eq!(retained.generation, committed.committed_generation);

        wm.enqueue_relayout(layout, output).unwrap();
        let proposal = next_proposal(wm, layout, output);
        let first = proposal.policy_settlement.unwrap();
        assert!(!proposal.requested_sizes.is_empty());
        let mut controls = crate::session_control::SessionControlQueue::default();
        assert!(layout.stage(proposal, &mut controls).unwrap().is_none());
        acknowledge_frontend_controls(layout, &mut controls);
        assert!(!layout.pending_is_ready());
        let empty = crate::live_session::wm_update_coordinator_batch(TransactionId::from_raw(702));
        cpu.cycle(wm, layout, &empty, None);
        cpu.assert_source(old, 1, 700, OLD_CONTENT_GENERATION, OLD_RGB);
        let target = layout
            .pending
            .as_ref()
            .unwrap()
            .layers
            .iter()
            .find(|layer| layer.surface == SURFACE)
            .unwrap()
            .geometry;
        let resized = cpu_batch(701, 701, 1, NEW_CONTENT_GENERATION, target, NEW_RGB);
        assert!(
            !layout
                .observe_authority_batch(&resized)
                .client_route_invalid
        );
        assert!(layout.pending_is_ready());
        assert!(wm.prepare_public_layout_commit(layout).unwrap());
        let result = layout.resolve_pending().unwrap();
        assert_eq!(result.update.commit.outcome, TransactionOutcome::Committed);
        let actual = wm.apply_commit_result(result, None, output.id).unwrap();
        writeln!(
            identity,
            "committed_wm_update_transaction={}\ncommitted_wm_request={}",
            actual.update.commit.transaction.raw(),
            first.request_id
        )
        .unwrap();
        assert!(
            cpu.cycle(wm, layout, &resized, Some(actual.update)),
            "successor pixel claim requires an actual composed frame"
        );
        cpu.assert_source(target, 2, 701, NEW_CONTENT_GENERATION, NEW_RGB);
        let retained_frame = cpu.scene.last_report().unwrap().frame.clone();
        let old_only_pixels = assert_composed_regions(&retained_frame, target, old);
        await_ready(wm, layout, output);
        let (checkpoint_before, checkpoint_id) = await_checkpoint(checkpoint);

        let settled = if timeout_action {
            let before = cpu.runtime.committed_surfaces().to_vec();
            assert_managed_baseline(layout);
            assert!(
                wm.public
                    .as_ref()
                    .unwrap()
                    .session_operations
                    .iter()
                    .any(|operation| operation.slot == 1)
            );
            let action = wm
                .public
                .as_ref()
                .unwrap()
                .actions
                .iter()
                .find(|action| action.session_operation_slot == Some(1))
                .unwrap()
                .action;
            wm.enqueue_action(action, layout, output).unwrap();
            let bounds = wm_output_bounds(&[output])[0].1;
            wm.set_shell_reservation_bands(vec![OutputReservation {
                edge: OutputEdge::Top,
                depth: 80,
                span: AxisSpan {
                    start: bounds.x,
                    end: bounds.x + bounds.width,
                },
            }]);
            wm.update_output_work_areas(layout, &[output], output)
                .unwrap();
            let proposal = next_proposal(wm, layout, output);
            let settlement = proposal.policy_settlement.unwrap();
            assert!(settlement.expect_session_operation);
            assert!(!proposal.requested_sizes.is_empty());
            assert!(layout.stage(proposal, &mut controls).unwrap().is_none());
            acknowledge_frontend_controls(layout, &mut controls);
            assert!(!layout.pending_is_ready());
            cpu.cycle(wm, layout, &empty, None);
            assert_eq!(cpu.runtime.committed_surfaces(), before);
            assert_eq!(cpu.scene.last_report().unwrap().frame, retained_frame);
            assert!(wm.public.as_ref().unwrap().prepared.is_none());
            layout.force_pending_timeout();
            let result = layout.expire_pending(&mut controls).unwrap().unwrap();
            assert_eq!(result.update.commit.outcome, TransactionOutcome::TimedOut);
            assert_managed_baseline(layout);
            assert_managed_rollback(&mut controls, &layout.layers[&SURFACE]);
            let actual = wm.apply_commit_result(result, None, output.id).unwrap();
            writeln!(
                identity,
                "timed_out_wm_update_transaction={}\ntimed_out_wm_request={}\ndeadline=forced_now",
                actual.update.commit.transaction.raw(),
                settlement.request_id
            )
            .unwrap();
            assert!(actual.session_action.is_none());
            cpu.cycle(wm, layout, &empty, Some(actual.update));
            cpu.assert_source(target, 2, 701, NEW_CONTENT_GENERATION, NEW_RGB);
            assert_eq!(cpu.runtime.committed_surfaces(), before);
            assert_eq!(cpu.scene.last_report().unwrap().frame, retained_frame);
            assert_eq!(layout.layers[&SURFACE].geometry, target);
            await_ready(wm, layout, output);
            settlement
        } else {
            wm.enqueue_relayout(layout, output).unwrap();
            first
        };
        let next = next_proposal(wm, layout, output);
        let next_id = next.policy_settlement.unwrap();
        assert!(next_id.request_id > settled.request_id);
        assert!(next_id.transaction.raw() > settled.transaction.raw());
        assert_eq!(wm.supervisor.peer_id(), peer);
        assert_eq!(next_id.connection_epoch, 1);
        if timeout_action {
            assert_eq!(std::fs::read(checkpoint).unwrap(), checkpoint_before);
            let after = std::fs::metadata(checkpoint).unwrap();
            assert_eq!((after.dev(), after.ino()), checkpoint_id);
            assert!(layout.layout_epochs.rollback_pending(SURFACE));
        }
        writeln!(identity, "cpu_join=true\nbaseline_authority_tx=700\nsuccessor_authority_tx=701\nbackend_surface_generations=1,2\nbuffer_content_generations=11,23\nbaseline_equality=supplied Session history versus actual CPU Engine commit\npending_cycle=empty; no competing content\ndeferred_present_rejection=false\ncomposed_interior_pixels=256\nold_only_pixels={old_only_pixels}\nwm_result=actual Session owner result\nnext_request={}\nwhole_owner_loop=false\nnative_retirement=false\nrollback_ack_supplied=false", next_id.request_id).unwrap();
    });
}

#[test]
#[ignore = "requires frozen normal Hagia and explicit fresh evidence inputs"]
fn real_hagia_resize_reaches_cpu_production_engine_commit() {
    cpu_join(false);
}

#[test]
#[ignore = "requires frozen normal Hagia and explicit fresh evidence inputs"]
fn real_hagia_timeout_keeps_cpu_production_state() {
    cpu_join(true);
}
