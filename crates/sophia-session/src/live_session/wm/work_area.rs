impl LiveWmSession {
    fn has_current_relayout_request(&self, layout: &PersistentLiveLayout) -> bool {
        let _ = layout;
        self.public.as_ref().is_some_and(|public| {
            public.in_flight_source == Some(LiveWmProposalSource::Relayout)
                || public
                    .queue
                    .iter()
                    .any(|cause| cause.source == LiveWmProposalSource::Relayout)
        })
    }

    /// Adopts the shell's committed work-area claim.
    ///
    /// Returns whether the claim differs from the one already reduced, so the
    /// caller reprojects exactly when the work area actually moves. The claim
    /// arrives already admitted and already presented: this session does not
    /// re-validate it, because the coordinator that owns that decision has
    /// made it against the same realized topology.
    pub(super) fn set_shell_reservation_bands(
        &mut self,
        bands: Vec<sophia_protocol::OutputReservation>,
    ) -> bool {
        if self.shell_reservation_bands == bands {
            return false;
        }
        self.shell_reservation_bands = bands;
        true
    }

    /// How many bands the shell's claim currently contributes.
    pub(super) fn shell_reservation_band_count(&self) -> usize {
        self.shell_reservation_bands.len()
    }

    fn update_output_work_areas(
        &mut self,
        layout: &PersistentLiveLayout,
        outputs: &[sophia_engine::HeadlessOutput],
        primary: sophia_engine::HeadlessOutput,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        self.update_output_work_areas_at(layout, outputs, &wm_output_bounds(outputs), primary)
    }

    fn update_output_work_areas_at(
        &mut self,
        layout: &PersistentLiveLayout,
        outputs: &[sophia_engine::HeadlessOutput],
        full_bounds: &[(sophia_protocol::OutputId, Rect)],
        primary: sophia_engine::HeadlessOutput,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        self.update_output_work_areas_and_policy(layout, outputs, full_bounds, primary, None)
    }

    fn update_output_work_areas_for_realization(
        &mut self,
        layout: &PersistentLiveLayout,
        outputs: &[sophia_engine::HeadlessOutput],
        policy: &output_realization::OutputPolicyLayout,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        let primary = outputs
            .iter()
            .find(|output| output.id == policy.primary)
            .copied()
            .ok_or("resolved output layout has no primary descriptor")?;
        self.update_output_work_areas_and_policy(
            layout,
            outputs,
            &policy.bounds,
            primary,
            Some(policy),
        )
    }

    fn update_output_work_areas_and_policy(
        &mut self,
        layout: &PersistentLiveLayout,
        outputs: &[sophia_engine::HeadlessOutput],
        full_bounds: &[(sophia_protocol::OutputId, Rect)],
        primary: sophia_engine::HeadlessOutput,
        policy: Option<&output_realization::OutputPolicyLayout>,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        let output_ids = outputs
            .iter()
            .map(|output| output.id)
            .collect::<BTreeSet<_>>();
        let bound_ids = full_bounds
            .iter()
            .map(|(output, _)| *output)
            .collect::<BTreeSet<_>>();
        if output_ids != bound_ids || bound_ids.len() != full_bounds.len() {
            return Err("live WM output bounds do not cover the logical outputs".into());
        }
        let root = full_bounds.iter().try_fold(
            Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            |root, (_, bounds)| {
                Some(Rect {
                    x: 0,
                    y: 0,
                    width: root.width.max(bounds.x.checked_add(bounds.width)?),
                    height: root.height.max(bounds.y.checked_add(bounds.height)?),
                })
            },
        );
        let Some(root) = root.filter(|root| !root.is_empty()) else {
            return Err("live WM output topology has no valid root bounds".into());
        };
        let work_areas = sophia_engine::reduce_output_work_areas(
            root,
            full_bounds.iter().copied(),
            &layout.active_output_reservations(),
            &self.shell_reservation_bands,
        );
        let work_bounds = work_areas
            .iter()
            .map(|area| {
                area.work
                    .map(|work| (area.output, work))
                    .ok_or("live WM output work-area reduction rejected an output")
            })
            .collect::<Result<Vec<_>, _>>()?;
        let _ = work_bounds;
        self.update_public_work_areas_at(layout, outputs, full_bounds, primary, policy)
    }
}
