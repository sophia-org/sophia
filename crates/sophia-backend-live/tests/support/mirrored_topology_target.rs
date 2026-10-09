//! Native topology facts supplied by the CPU mirror fixture. Runtime rebind,
//! resume lowering, installation and head retirement use their production paths.
//! No card, worker initialization, retained-image restore or KMS claim is made.
use super::*;

impl NativeTopologyTarget for MirroredTarget {
    fn adopt_output_runtimes(
        &mut self,
        outputs: &[HeadlessOutput],
        committed: &[CommittedSurfaceState],
    ) -> Result<LiveProductionOutputRuntimeSet, Box<dyn std::error::Error>> {
        assert_eq!(
            outputs.iter().map(|o| o.id).collect::<BTreeSet<_>>(),
            self.outputs.keys().copied().collect()
        );
        LiveProductionOutputRuntimeSet::new(outputs, committed, None)
    }

    fn stop_translation_motion(&mut self) {}

    fn handoff_topology_custody(
        &mut self,
        previous: &mut LiveProductionOutputRuntimeSet,
        next: &mut LiveProductionOutputRuntimeSet,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // This fixture's real affine owners are in its mirror head ledgers,
        // retired by teardown before the replacement target is constructed.
        // Singleton handoff itself has separate production custody controls.
        for output in previous.values().chain(next.values()) {
            assert!(!output.runtime.rendered_primary_plane_scanout_in_flight());
            assert!(!output.runtime.rendered_primary_plane_scanout_displayed());
            assert!(
                !output
                    .runtime
                    .rendered_primary_plane_scanout_cleanup_pending()
            );
        }
        Ok(())
    }

    fn initialize_output_composition(
        &mut self,
        outputs: &mut LiveProductionOutputRuntimeSet,
        output: OutputId,
        frames: Vec<LiveProductionHeadCompositionFrame>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        assert!(outputs.output_index(output).is_some());
        self.queue_retained_batch(vec![(output, frames)], &BTreeSet::new())?;
        self.install(output)?;
        self.prepare(output);
        // A completion is supplied explicitly by the test. Prepared content
        // must never stand for the all-head presentation witness.
        Ok(())
    }
}
