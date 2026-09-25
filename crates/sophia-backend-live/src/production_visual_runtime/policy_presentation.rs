use super::*;

/// An admitted WM presentation, qualified by the WM connection epoch that
/// admitted it (t244). Admission, validation and the policy reducer belong to
/// the protocol seam (t243); the renderer draws only what it is handed, and
/// resolves every source's committed generation itself at frame capture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LivePolicyPresentation {
    pub owner_epoch: u64,
    pub presentation: PolicyPresentation,
}

impl LivePolicyPresentation {
    /// This output's instances as display commands in z order. The source
    /// generation is left for [`resolve_surface_instance_sources`], so a
    /// value the WM could influence never reaches a frame.
    pub(super) fn instance_commands(&self, output: OutputId) -> Vec<CompositorDisplayCommand> {
        let mut instances = self
            .presentation
            .instances
            .iter()
            .filter(|instance| instance.output == output)
            .collect::<Vec<_>>();
        instances.sort_by_key(|instance| instance.z_index);
        instances
            .into_iter()
            .map(|instance| {
                CompositorDisplayCommand::SurfaceInstance(CompositorSurfaceInstance {
                    owner_epoch: self.owner_epoch,
                    id: instance.id,
                    generation: instance.generation,
                    source: instance.source,
                    source_generation: 0,
                    destination: instance.destination,
                    clip: instance.clip,
                    opacity_millis: instance.opacity_millis,
                })
            })
            .collect()
    }

    fn sources(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        self.presentation
            .instances
            .iter()
            .map(|instance| instance.source)
    }
}

impl LiveProductionVisualRuntime {
    /// Installs or withdraws the admitted WM presentation and queues a
    /// retained repaint when native scanout owns presentation. The source
    /// surfaces keep their allocation, placement and output ownership.
    pub fn set_policy_presentation(
        &mut self,
        presentation: Option<LivePolicyPresentation>,
        scene: &LiveProductionCpuScene,
        native_scanout: Option<&mut LiveProductionNativeScanout>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if self.policy_presentation == presentation {
            return Ok(false);
        }
        let previous = std::mem::replace(&mut self.policy_presentation, presentation);
        if let Some(native_scanout) = native_scanout
            && let Err(error) = self.queue_retained_projection(scene, native_scanout)
        {
            self.policy_presentation = previous;
            return Err(error);
        }
        Ok(true)
    }

    pub fn policy_presentation(&self) -> Option<&LivePolicyPresentation> {
        self.policy_presentation.as_ref()
    }

    /// The surfaces a frame samples: the presentation order, then every
    /// instance source it does not already name. A preview-only source is
    /// sampled like any other; it is not added to the presentation order, so
    /// it is neither presented at its own placement nor an input layer.
    pub(super) fn sampled_surface_order(&self) -> Vec<SurfaceId> {
        let mut order = self.presentation_order.clone();
        if let Some(presentation) = &self.policy_presentation {
            for source in presentation.sources() {
                if !order.contains(&source) {
                    order.push(source);
                }
            }
        }
        order
    }
}
