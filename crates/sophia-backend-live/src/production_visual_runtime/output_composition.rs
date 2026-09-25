use super::*;

/// Immutable decoration inputs shared by policy cycles and retained repaints.
/// Shell images remain actual lease-bearing sources through lowering.
pub(super) struct OutputComposition<'a> {
    pub chrome_surfaces: &'a [SurfaceId],
    pub focused_surface: Option<SurfaceId>,
    pub surface_chrome_style: SurfaceChromeStyle,
    pub floating_outline: Option<LiveFloatingOutline>,
    pub indicator_publication: Option<&'a sophia_engine::PolicyIndicatorPublication>,
    pub tab_bars: &'a [sophia_engine::TabBarProjection],
    pub shell_content: &'a BTreeMap<ShellContentKey, AdmittedShellContent>,
    pub descriptor_overlay: Option<&'a sophia_engine::DescriptorOverlayProjection>,
    pub policy_presentation: Option<&'a LivePolicyPresentation>,
}

impl OutputComposition<'_> {
    pub fn display_list(
        &self,
        output: OutputId,
        committed_surfaces: &[CommittedSurfaceState],
        presentation_order: &[SurfaceId],
    ) -> Result<CompositorDisplayList, CompositorDisplayListError> {
        // The WM presentation tier: above ordinary application content and
        // its decorations, below shell content and the descriptor overlay.
        // Engine, never the WM, names the generation each instance samples.
        // The tier is drawn whole or not at all: admission refuses a
        // presentation whose source has no committed content and removal
        // revokes it, so a missing source here is a view that moved on, and
        // no instance is dropped from a presentation that is drawn.
        let tier = self.policy_presentation.and_then(|presentation| {
            let mut tier = CompositorDisplayList {
                output,
                commands: presentation.tier_commands(output, self.surface_chrome_style),
            };
            match sophia_engine::resolve_surface_instance_sources(&mut tier, committed_surfaces) {
                Ok(()) => Some(tier.commands),
                Err(missing) => {
                    tracing::warn!(
                        "sophia_wm_presentation status=withheld reason=missing_source owner_epoch={} generation={} source={:?}",
                        presentation.owner_epoch,
                        presentation.presentation.generation,
                        missing.source,
                    );
                    None
                }
            }
        });
        // ReplaceApplications substitutes the tier for this output's
        // ordinary application presentation: its surfaces, their chrome,
        // tab bars and the floating outline are not drawn here, and so are
        // not hit targets either. Clients keep their allocations and content.
        let replaces = tier.is_some()
            && self
                .policy_presentation
                .is_some_and(|presentation| presentation.replaces_applications(output));
        let mut display_list = surface_chrome_display_list_for_surfaces(
            output,
            if replaces { &[] } else { presentation_order },
            self.chrome_surfaces,
            committed_surfaces,
            self.focused_surface,
            self.surface_chrome_style,
        )?;
        if let Some(publication) = self.indicator_publication.filter(|_| !replaces) {
            sophia_engine::append_tab_bars(
                &mut display_list.commands,
                &publication.tab_groups,
                publication.generation,
                self.tab_bars,
                output,
            );
        }
        if let Some(outline) = self.floating_outline.filter(|_| !replaces) {
            if display_list.commands.len() >= MAX_COMPOSITOR_DISPLAY_COMMANDS {
                return Err(CompositorDisplayListError::CapacityExceeded);
            }
            let border = compositor_floating_outline(
                outline.surface,
                outline.geometry,
                self.surface_chrome_style.focus_ring.width.max(2),
                self.surface_chrome_style.focus_ring.color,
            )
            .ok_or(CompositorDisplayListError::InvalidSurface)?;
            display_list
                .commands
                .push(CompositorDisplayCommand::Border(border));
        }
        if let Some(tier) = tier {
            if display_list.commands.len().saturating_add(tier.len())
                > MAX_COMPOSITOR_DISPLAY_COMMANDS
            {
                return Err(CompositorDisplayListError::CapacityExceeded);
            }
            display_list.commands.extend(tier);
        }
        for (_, content) in self
            .shell_content
            .iter()
            .filter(|((id, _), _)| *id == output)
        {
            let content = &content.frame;
            if display_list
                .commands
                .len()
                .saturating_add(content.images.len())
                > MAX_COMPOSITOR_DISPLAY_COMMANDS
            {
                return Err(CompositorDisplayListError::CapacityExceeded);
            }
            display_list.commands.extend(
                content
                    .images
                    .iter()
                    .cloned()
                    .map(CompositorDisplayCommand::ContentImage),
            );
        }
        if let Some(overlay) = self
            .descriptor_overlay
            .as_ref()
            .filter(|overlay| overlay.output == output)
        {
            if display_list
                .commands
                .len()
                .saturating_add(overlay.commands.len())
                > MAX_COMPOSITOR_DISPLAY_COMMANDS
            {
                return Err(CompositorDisplayListError::CapacityExceeded);
            }
            display_list
                .commands
                .extend(overlay.commands.iter().cloned());
        }
        Ok(display_list)
    }
}

/// A policy cycle must retain its decoration sources while the coordinator and
/// output runtimes are mutably borrowed. This is a render owner, not history.
pub(super) struct OutputCompositionSnapshot {
    orders: BTreeMap<OutputId, Vec<SurfaceId>>,
    chrome_surfaces: Vec<SurfaceId>,
    focused_surface: Option<SurfaceId>,
    surface_chrome_style: SurfaceChromeStyle,
    floating_outline: Option<LiveFloatingOutline>,
    indicator_publication: Option<sophia_engine::PolicyIndicatorPublication>,
    tab_bars: Vec<sophia_engine::TabBarProjection>,
    shell_content: BTreeMap<ShellContentKey, AdmittedShellContent>,
    descriptor_overlay: Option<sophia_engine::DescriptorOverlayProjection>,
    policy_presentation: Option<LivePolicyPresentation>,
}

impl OutputCompositionSnapshot {
    pub fn capture(runtime: &LiveProductionVisualRuntime) -> Self {
        Self {
            orders: runtime.presentation_orders_by_output(),
            chrome_surfaces: runtime.chrome_surfaces.clone(),
            focused_surface: runtime.focused_surface,
            surface_chrome_style: runtime.surface_chrome_style,
            floating_outline: runtime.floating_outline,
            indicator_publication: runtime.indicator_publication.clone(),
            tab_bars: runtime.tab_bars.clone(),
            shell_content: runtime.shell_content.clone(),
            descriptor_overlay: runtime.descriptor_overlay.clone(),
            policy_presentation: runtime.policy_presentation.clone(),
        }
    }

    pub fn display_list(
        &self,
        output: OutputId,
        committed: &[CommittedSurfaceState],
    ) -> Result<CompositorDisplayList, CompositorDisplayListError> {
        OutputComposition {
            chrome_surfaces: &self.chrome_surfaces,
            focused_surface: self.focused_surface,
            surface_chrome_style: self.surface_chrome_style,
            floating_outline: self.floating_outline,
            indicator_publication: self.indicator_publication.as_ref(),
            tab_bars: &self.tab_bars,
            shell_content: &self.shell_content,
            descriptor_overlay: self.descriptor_overlay.as_ref(),
            policy_presentation: self.policy_presentation.as_ref(),
        }
        .display_list(
            output,
            committed,
            self.orders
                .get(&output)
                .ok_or(CompositorDisplayListError::InvalidOutput)?,
        )
    }
}
