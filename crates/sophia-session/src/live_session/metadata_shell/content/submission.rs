//! Common owned-bundle transfer for panel and native launcher candidates.
use super::*;

impl LiveContentSession {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn submit_bundle(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        runtime: &mut sophia_backend_live::LiveProductionVisualRuntime,
        scene: &sophia_backend_live::LiveProductionCpuScene,
        native_scanout: Option<&mut sophia_backend_live::LiveProductionNativeScanout>,
        outputs: &[HeadlessOutput],
        output_bounds: &[(OutputId, Rect)],
        root: Rect,
        bundle: ContentRenderBundle,
        layer: sophia_backend_live::LiveShellContentLayer,
        now: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let output = bundle.output;
        let generation = bundle.candidate_generation;
        let allocations = transport.content_allocation_snapshots();
        let descriptor = outputs
            .iter()
            .find(|descriptor| descriptor.id.raw() == output.id)
            .copied()
            .ok_or("content candidate targets a removed output")?;
        self.stage = ContentServiceStage::Projection;
        let frame = project_render_bundle(&bundle, descriptor, output, &allocations)?;
        let bands = candidate_bands(&bundle, &allocations, output_bounds, root)?;
        self.stage = ContentServiceStage::Runtime;
        runtime.set_shell_component_content(frame, layer, scene, native_scanout)?;
        let grant = transport.content_grant().ok_or("content grant vanished")?;
        self.stage = ContentServiceStage::Prepared;
        transport.content_prepared(grant, output, generation, 1, 1, now)?;
        let usage = transport.content_usage().unwrap_or_default();
        crate::session_println!(
            "sophia_live_shell_content schema=1 status=prepared output={} candidate_generation={} staging_bytes={} resident_bytes={} retiring_bytes={} backing_bytes={}",
            output.id,
            generation,
            usage.staging,
            usage.resident,
            usage.retiring,
            usage.backing,
        );
        let candidate_allocations = bundle
            .surfaces
            .iter()
            .map(|surface| surface.allocation)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        self.pending.push(PendingPresentation {
            catalog: bundle.persistent_catalog,
            grant,
            output,
            candidate_generation: generation,
            bands,
            allocations: candidate_allocations,
        });
        Ok(())
    }
}
