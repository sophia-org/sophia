impl LiveProductionVisualRuntime {
    fn apply_presentation_layout(
        &mut self,
        layout: &[LayerSnapshot],
        geometry_routed: &[SurfaceId],
    ) -> bool {
        let now = Instant::now();
        let time = self.translation_time();
        let translation_changed = self.translations.replace_targets(layout, time);
        if translation_changed {
            tracing::debug!(
                event = "translation_targets",
                active = self.translations.active(time),
                members = layout
                    .iter()
                    .filter(|layer| layer.translation.is_some())
                    .count(),
                "updated Engine presentation translation targets"
            );
        }
        for output in self.outputs.logical_viewports().map(|(id, _)| id) {
            if self.translations.active_on(output, time) {
                self.translation_deadlines.entry(output).or_insert(now);
            }
        }
        let order_changed = self.presentation_order.len() != layout.len()
            || self
                .presentation_order
                .iter()
                .zip(layout)
                .any(|(surface, layer)| *surface != layer.surface);
        self.presentation_order.clear();
        self.presentation_order
            .extend(layout.iter().map(|layer| layer.surface));
        if order_changed {
            // Input eligibility must not wait for the next accepted page flip.
            // On the native path nothing republishes the projection until a
            // frame retires, so a window unmapped now would keep answering the
            // pointer for the whole interval until then -- unbounded if the
            // flip stalls. Pruning here only ever removes what has left the
            // layout; pixels still on screen keep routing until they do.
            self.prune_input_projections_to_presentation_order();
        }
        // Which head composites each surface. A scrolling layout puts columns
        // past the edge of their own display on purpose, and with a second
        // display beside it, "past the edge" and "inside the neighbour" are
        // the same region -- so without this, geometry alone drew one
        // display's window on another.
        let routed = layout
            .iter()
            .filter(|layer| layer.output.is_none() && geometry_routed.contains(&layer.surface))
            .map(|layer| layer.surface)
            .collect::<BTreeSet<_>>();
        let routing_changed = self.geometry_routed_surfaces != routed
            || layout
                .iter()
                .any(|layer| self.surface_outputs.get(&layer.surface).copied() != layer.output);
        self.geometry_routed_surfaces = routed;
        self.surface_outputs.clear();
        for layer in layout {
            if let Some(output) = layer.output {
                self.surface_outputs.insert(layer.surface, output);
            }
        }
        for layer in layout {
            self.present_scheduler
                .reproject_surface(layer.surface, layer.geometry);
            if let Some(displayed) = self.displayed_surfaces.get_mut(&layer.surface) {
                displayed.layer.reproject(layer.geometry);
            }
        }
        // Restarted policy connections seed stationary translation targets, so
        // no animation deadline will repaint the pixels their old positions
        // occupied. Request the ordinary retained repaint even without a new
        // client frame; its existing admission barrier still owns retirement.
        order_changed || routing_changed || translation_changed
    }

    fn set_chrome_surfaces(&mut self, surfaces: &[SurfaceId]) -> bool {
        if self.chrome_surfaces == surfaces {
            return false;
        }
        self.chrome_surfaces.clear();
        self.chrome_surfaces.extend_from_slice(surfaces);
        true
    }
}
