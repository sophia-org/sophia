use super::*;

/// The operator-approved interval for clients sampled by no visible output.
/// This delays Skip completion and Idle, not the physical MSC clock.
pub(super) const BACKGROUND_PRESENT_INTERVAL: Duration = Duration::from_secs(1);

struct BackgroundOutputVisibility {
    output: OutputId,
    viewport: Rect,
    replaces_applications: bool,
    previewed: BTreeMap<SurfaceId, Vec<Rect>>,
}

impl LiveProductionVisualRuntime {
    /// Capacity is shared by GPU, software, first-visibility and layout-held
    /// work. Reclaim background debt before intake can make the registry full,
    /// including a batch of latest-deferred groups released by a timer tick.
    pub(super) fn reclaim_background_presentation_capacity(&mut self, incoming: usize) {
        let resources = self.presentation_feedback.resources();
        let needed = resources
            .presentation_count()
            .saturating_add(incoming)
            .saturating_sub(resources.presentation_capacity());
        for transaction in self.present_scheduler.release_frame_tick_pressure(needed) {
            self.reject_gpu_presentation(transaction);
        }
    }

    pub(super) fn service_background_presentations(&mut self, now: Instant) {
        if self.present_scheduler.frame_tick_deadline().is_none() {
            return;
        }
        let visible =
            self.background_visible_surfaces(self.present_scheduler.awaiting_frame_tick());
        for transaction in self
            .present_scheduler
            .release_frame_tick_or_visible(now, &visible)
        {
            self.reject_gpu_presentation(transaction);
        }
    }

    pub(super) fn background_visible_surfaces(
        &self,
        candidates: impl IntoIterator<Item = (SurfaceId, Rect)>,
    ) -> Vec<SurfaceId> {
        let geometries = self
            .production
            .committed_surfaces()
            .iter()
            .map(|state| (state.surface, state.geometry))
            .collect::<BTreeMap<_, _>>();
        let outputs = self.background_output_visibility(&geometries);
        let time = self.translation_time();
        // Multiple parked buffers of one surface share the same observation.
        candidates
            .into_iter()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .filter_map(|(surface, fallback)| {
                self.background_visible_on_outputs(
                    surface,
                    geometries.get(&surface).copied().unwrap_or(fallback),
                    time,
                    &outputs,
                )
                .then_some(surface)
            })
            .collect()
    }

    fn background_output_visibility(
        &self,
        geometries: &BTreeMap<SurfaceId, Rect>,
    ) -> Vec<BackgroundOutputVisibility> {
        self.outputs
            .logical_viewports()
            .map(|(output, viewport)| {
                // OutputComposition withholds an entire tier if any of that
                // output's instance sources is absent. Ordinary applications then
                // return. Compute this once per output, not once per parked buffer.
                let tier = self.policy_presentation.as_ref().filter(|publication| {
                    publication
                        .presentation
                        .instances
                        .iter()
                        .filter(|instance| instance.output == output)
                        .all(|instance| geometries.contains_key(&instance.source))
                });
                let mut previewed = BTreeMap::<SurfaceId, Vec<Rect>>::new();
                for instance in tier
                    .into_iter()
                    .flat_map(|p| &p.presentation.instances)
                    .filter(|instance| instance.output == output && instance.opacity_millis != 0)
                {
                    let sampled = crate::presentation::intersect_rects(
                        crate::presentation::intersect_rects(instance.destination, instance.clip),
                        viewport,
                    );
                    if !sampled.is_empty() {
                        previewed.entry(instance.source).or_default().push(sampled);
                    }
                }
                BackgroundOutputVisibility {
                    output,
                    viewport,
                    previewed,
                    replaces_applications: tier.is_some_and(|p| p.replaces_applications(output)),
                }
            })
            .collect()
    }

    /// Per-new-request clock selection from current sampling, in logical
    /// desktop coordinates. Overlapping ordinary/preview samples count once.
    /// A tie chooses the configured primary output, then the lowest output ID.
    /// Session prefers that logical output's primary mirror head, falling
    /// back to an active member. Replicas share its logical viewport and never
    /// create a second clock for one request.
    /// None means the explicit one-second fake clock. This only constructs
    /// visibility when demanded; it does not query a device or schedule work.
    pub fn present_clock_outputs(
        &self,
        candidates: impl IntoIterator<Item = (SurfaceId, Rect)>,
        primary_output: Option<OutputId>,
    ) -> Vec<(SurfaceId, Option<OutputId>)> {
        let geometries = self
            .production
            .committed_surfaces()
            .iter()
            .map(|state| (state.surface, state.geometry))
            .collect::<BTreeMap<_, _>>();
        let outputs = self.background_output_visibility(&geometries);
        let time = self.translation_time();
        candidates
            .into_iter()
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .map(|(surface, fallback)| {
                let geometry = geometries.get(&surface).copied().unwrap_or(fallback);
                let selected = outputs
                    .iter()
                    .filter_map(|view| {
                        let mut rects = view.previewed.get(&surface).cloned().unwrap_or_default();
                        if self.presentation_order.contains(&surface)
                            && !view.replaces_applications
                            && live_surface_routes_to_output(
                                surface,
                                &self.surface_outputs,
                                &self.geometry_routed_surfaces,
                                view.output,
                            )
                        {
                            let sampled = crate::presentation::intersect_rects(
                                self.translations
                                    .geometry(surface, view.output, geometry, time),
                                view.viewport,
                            );
                            if !sampled.is_empty() {
                                rects.push(sampled);
                            }
                        }
                        let area = sampled_union_area(&rects);
                        (area != 0).then_some((
                            area,
                            primary_output == Some(view.output),
                            std::cmp::Reverse(view.output),
                        ))
                    })
                    .max()
                    .map(|(_, _, output)| output.0);
                (surface, selected)
            })
            .collect()
    }

    /// Use every logical output (including mirrored outputs), the current
    /// translation position, and clipped policy previews. A hidden workspace
    /// with a visible preview is a visible source. Ordinary surfaces hidden by
    /// ReplaceApplications count only through an instance of that source.
    fn background_visible_on_outputs(
        &self,
        surface: SurfaceId,
        geometry: Rect,
        time: f64,
        outputs: &[BackgroundOutputVisibility],
    ) -> bool {
        outputs.iter().any(|view| {
            view.previewed.contains_key(&surface)
                || (self.presentation_order.contains(&surface)
                    && live_surface_routes_to_output(
                        surface,
                        &self.surface_outputs,
                        &self.geometry_routed_surfaces,
                        view.output,
                    )
                    && !view.replaces_applications
                    && !crate::presentation::intersect_rects(
                        self.translations
                            .geometry(surface, view.output, geometry, time),
                        view.viewport,
                    )
                    .is_empty())
        })
    }

    #[cfg(test)]
    pub(super) fn background_surface_is_visible(
        &self,
        surface: SurfaceId,
        geometry: Rect,
        time: f64,
    ) -> bool {
        let geometries = self
            .production
            .committed_surfaces()
            .iter()
            .map(|state| (state.surface, state.geometry))
            .collect();
        self.background_visible_on_outputs(
            surface,
            geometry,
            time,
            &self.background_output_visibility(&geometries),
        )
    }
}

/// Rectangles are already clipped to one bounded viewport. Sweep disjoint
/// x strips and merge y intervals so duplicated previews do not bias a tie.
fn sampled_union_area(rects: &[Rect]) -> u64 {
    match rects {
        [] => return 0,
        [one] => return one.width.max(0) as u64 * one.height.max(0) as u64,
        _ => {}
    }
    let mut xs = rects
        .iter()
        .flat_map(|r| [i64::from(r.x), i64::from(r.x) + i64::from(r.width)])
        .collect::<Vec<_>>();
    xs.sort_unstable();
    xs.dedup();
    xs.windows(2)
        .map(|x| {
            let mut ys = rects
                .iter()
                .filter(|r| i64::from(r.x) < x[1] && i64::from(r.x) + i64::from(r.width) > x[0])
                .map(|r| (i64::from(r.y), i64::from(r.y) + i64::from(r.height)))
                .collect::<Vec<_>>();
            ys.sort_unstable();
            let mut end = i64::MIN;
            let mut height = 0_u64;
            for (lo, hi) in ys {
                if hi > end {
                    height += (hi - lo.max(end)) as u64;
                    end = hi;
                }
            }
            (x[1] - x[0]) as u64 * height
        })
        .sum()
}
