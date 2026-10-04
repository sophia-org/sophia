use crate::{
    LIVE_RENDERED_OUTPUT_CAPACITY, LiveBackendRuntimeAssembly, LiveProductionNativeScanout,
    LiveRendererImportPathStatus, LiveRendererImportStartupStatus, LiveRendererRuntimeObservation,
    LiveRendererSelectionObservation,
};
use sophia_engine::{HeadlessCompositorBackendAssembly, HeadlessOutput};
use sophia_protocol::{CommittedSurfaceState, OutputId, Rect};
use std::collections::BTreeMap;
use std::error::Error;

#[cfg(test)]
#[path = "../tests/support/output_runtime_startup.rs"]
mod startup_test_support;

pub struct LiveProductionOutputRuntime {
    pub runtime: LiveBackendRuntimeAssembly,
    native_initialized: bool,
}

pub struct LiveProductionOutputRuntimeSet {
    outputs: BTreeMap<OutputId, LiveProductionOutputRuntime>,
    logical_viewports: BTreeMap<OutputId, Rect>,
}

impl LiveProductionOutputRuntimeSet {
    pub fn new(
        outputs: &[HeadlessOutput],
        committed_surfaces: &[CommittedSurfaceState],
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<Self, Box<dyn Error>> {
        Self::build(outputs, committed_surfaces, native_scanout, false)
    }

    /// Rebinds runtime-only output state to buffers already established by a
    /// blocking topology commit. It must not issue another modeset.
    pub fn adopt_native_topology(
        outputs: &[HeadlessOutput],
        committed_surfaces: &[CommittedSurfaceState],
        native_scanout: &mut LiveProductionNativeScanout,
    ) -> Result<Self, Box<dyn Error>> {
        Self::build(outputs, committed_surfaces, Some(native_scanout), true)
    }

    fn build(
        outputs: &[HeadlessOutput],
        committed_surfaces: &[CommittedSurfaceState],
        mut native_scanout: Option<&mut LiveProductionNativeScanout>,
        adopted_native: bool,
    ) -> Result<Self, Box<dyn Error>> {
        if outputs.is_empty() || outputs.len() > LIVE_RENDERED_OUTPUT_CAPACITY {
            return Err("production output runtime requires 1-16 outputs".into());
        }
        let mut output_runtimes = BTreeMap::new();
        for output in outputs.iter().copied() {
            let assembly = HeadlessCompositorBackendAssembly::new(output)
                .with_committed_surfaces(committed_surfaces.to_vec());
            let renderer = LiveRendererRuntimeObservation::from_startup_status(
                LiveRendererImportStartupStatus::from_path_statuses(
                    LiveRendererImportPathStatus::Disabled,
                    LiveRendererImportPathStatus::Disabled,
                ),
                LiveRendererSelectionObservation::CpuFallback,
            );
            let mut runtime =
                LiveBackendRuntimeAssembly::from_ready_headless_scanout(assembly, output, renderer)
                    .with_persistent_rendered_primary_plane_scanout();
            let native_initialized = native_scanout.is_none() || adopted_native;
            if let Some(native_scanout) = native_scanout.as_deref_mut() {
                // Heads are addressed through the output, not through this loop's
                // position. They coincide only until a mirror group puts two heads
                // behind one logical output, and then the position would name the
                // wrong connector for every output after the group.
                let head_indices = native_scanout.head_indices(output.id);
                let Some(&primary_head) = head_indices.first() else {
                    return Err("production native output has no head".into());
                };
                let selection = native_scanout.selection(primary_head);
                if !runtime.configure_native_output_selection(output.id, selection) {
                    return Err("production native output selection was not registered".into());
                }
                // Every connector of the group, so retirement can wait for the last
                // head rather than the first.
                if !runtime.configure_native_output_heads(
                    output.id,
                    head_indices
                        .iter()
                        .map(|head| native_scanout.selection(*head)),
                ) {
                    return Err("production native output heads were not registered".into());
                }
                runtime.primary_output_state_mut().native_custody_scope =
                    native_scanout.singleton_custody_scope(output.id);
            }
            output_runtimes.insert(
                output.id,
                LiveProductionOutputRuntime {
                    runtime,
                    native_initialized,
                },
            );
        }
        let mut logical_x = 0i32;
        let logical_viewports = outputs
            .iter()
            .map(|output| {
                let scale = i32::try_from(output.scale.max(1)).unwrap_or(i32::MAX);
                let logical = Rect {
                    x: logical_x,
                    y: 0,
                    width: output.size.width.saturating_add(scale - 1) / scale,
                    height: output.size.height.saturating_add(scale - 1) / scale,
                };
                logical_x = logical_x.saturating_add(logical.width);
                (output.id, logical)
            })
            .collect();
        Ok(Self {
            outputs: output_runtimes,
            logical_viewports,
        })
    }

    pub fn native_initialized(&self, output: OutputId) -> bool {
        self.outputs
            .get(&output)
            .is_some_and(|output| output.native_initialized)
    }

    pub fn mark_native_initialized(&mut self, output: OutputId) -> Result<(), Box<dyn Error>> {
        let output = self
            .outputs
            .get_mut(&output)
            .ok_or("production output was not registered")?;
        output.native_initialized = true;
        Ok(())
    }

    pub fn initialize_native_head_composition(
        &mut self,
        native_scanout: &mut LiveProductionNativeScanout,
        output: OutputId,
        frames: Vec<crate::LiveProductionHeadCompositionFrame>,
    ) -> Result<crate::LiveProductionNativeFrameId, Box<dyn Error>> {
        let output_runtime = self
            .outputs
            .get_mut(&output)
            .ok_or("production output was not registered")?;
        if output_runtime.native_initialized {
            return Err("production output native scanout was already initialized".into());
        }
        let frame = native_scanout.initialize_head_composition(
            output,
            &mut output_runtime.runtime,
            frames,
        )?;
        output_runtime.native_initialized = true;
        Ok(frame)
    }

    pub fn run_output<R>(
        &mut self,
        index: usize,
        committed: &[CommittedSurfaceState],
        run: impl FnOnce(&mut LiveBackendRuntimeAssembly) -> Result<R, Box<dyn Error>>,
    ) -> Result<R, Box<dyn Error>> {
        let output = self
            .outputs
            .values_mut()
            .nth(index)
            .ok_or("production output index was not registered")?;
        output
            .runtime
            .assembly_mut()
            .replace_committed_surfaces(committed.to_vec());
        run(&mut output.runtime)
    }

    pub fn project_committed(&mut self, committed: &[CommittedSurfaceState]) {
        for output in self.outputs.values_mut() {
            output
                .runtime
                .assembly_mut()
                .replace_committed_surfaces(committed.to_vec());
        }
    }

    pub fn replace_output_projection(
        &mut self,
        index: usize,
        committed: Vec<CommittedSurfaceState>,
    ) -> bool {
        let Some(output) = self.outputs.values_mut().nth(index) else {
            return false;
        };
        output
            .runtime
            .assembly_mut()
            .replace_committed_surfaces(committed);
        true
    }

    pub fn output_committed(&self, index: usize) -> Option<&[CommittedSurfaceState]> {
        self.outputs
            .values()
            .nth(index)
            .map(|output| output.runtime.assembly().committed_surfaces())
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut LiveProductionOutputRuntime> {
        self.outputs.values_mut()
    }

    pub fn values(&self) -> impl Iterator<Item = &LiveProductionOutputRuntime> {
        self.outputs.values()
    }

    pub fn primary_output(&self) -> Option<OutputId> {
        self.outputs.keys().next().copied()
    }

    pub fn output_index(&self, output: OutputId) -> Option<usize> {
        self.outputs
            .keys()
            .position(|candidate| *candidate == output)
    }

    pub fn output_id(&self, index: usize) -> Option<OutputId> {
        self.outputs.keys().nth(index).copied()
    }

    pub fn output_descriptor(&self, index: usize) -> Option<HeadlessOutput> {
        self.outputs
            .values()
            .nth(index)
            .map(|output| output.runtime.assembly().engine().output())
    }

    pub fn logical_viewport(&self, output: OutputId) -> Option<Rect> {
        self.logical_viewports.get(&output).copied()
    }

    pub fn logical_viewports(&self) -> impl Iterator<Item = (OutputId, Rect)> + '_ {
        self.logical_viewports
            .iter()
            .map(|(output, logical)| (*output, *logical))
    }

    /// Replaces the complete root-space placement contract for this runtime
    /// set. The update is validated before mutation so a partial IPC topology
    /// can never leave composition and input projection on different layouts.
    pub fn replace_logical_viewports(
        &mut self,
        logical_viewports: &[(OutputId, Rect)],
    ) -> Result<(), Box<dyn Error>> {
        if logical_viewports.len() != self.outputs.len() {
            return Err("logical viewport replacement omitted an output".into());
        }
        let mut replacement = BTreeMap::new();
        for (output, logical) in logical_viewports {
            if !self.outputs.contains_key(output) || logical.is_empty() {
                return Err("logical viewport replacement is invalid".into());
            }
            if replacement.insert(*output, *logical).is_some() {
                return Err("logical viewport replacement duplicates an output".into());
            }
        }
        if replacement.keys().ne(self.outputs.keys()) {
            return Err("logical viewport replacement targets the wrong outputs".into());
        }
        self.logical_viewports = replacement;
        Ok(())
    }

    pub fn output_count(&self) -> usize {
        self.outputs.len()
    }

    pub fn native_scanout_in_flight(&self) -> bool {
        self.outputs
            .values()
            .any(|output| output.runtime.rendered_primary_plane_scanout_in_flight())
    }

    pub fn native_scanout_in_flight_count(&self) -> usize {
        self.outputs
            .values()
            .filter(|output| output.runtime.rendered_primary_plane_scanout_in_flight())
            .count()
    }

    pub fn output_native_scanout_in_flight(&self, index: usize) -> Option<bool> {
        self.outputs
            .values()
            .nth(index)
            .map(|output| output.runtime.rendered_primary_plane_scanout_in_flight())
    }

    pub fn output_native_cleanup_pending(&self, index: usize) -> Option<bool> {
        self.outputs.values().nth(index).map(|output| {
            output
                .runtime
                .rendered_primary_plane_scanout_cleanup_pending()
        })
    }

    pub fn native_cleanup_pending(&self) -> bool {
        self.outputs.values().any(|output| {
            output
                .runtime
                .rendered_primary_plane_scanout_cleanup_pending()
        })
    }

    pub fn diagnostic(&self) -> String {
        self.outputs
            .iter()
            .map(|(output, state)| {
                format!(
                    "{}:in_flight={}:ticks={}:cleanup={}",
                    output.raw(),
                    state.runtime.rendered_primary_plane_scanout_in_flight(),
                    state
                        .runtime
                        .rendered_primary_plane_scanout_in_flight_ticks(),
                    state
                        .runtime
                        .rendered_primary_plane_scanout_cleanup_pending(),
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    }
}
