impl LiveWmSession {
    fn update_public_work_areas_at(
        &mut self,
        layout: &PersistentLiveLayout,
        outputs: &[sophia_engine::HeadlessOutput],
        full_bounds: &[(sophia_protocol::OutputId, Rect)],
        primary: sophia_engine::HeadlessOutput,
    ) -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
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
            return Err("public WM output topology has no valid root bounds".into());
        };
        let reduced = sophia_engine::reduce_output_work_areas(
            root,
            full_bounds.iter().copied(),
            &layout.active_output_reservations(),
            &self.shell_reservation_bands,
        );
        let chrome_style = self.candidate_chrome_style();
        let public = self.public.as_mut().expect("public WM state is present");
        let next_live = outputs
            .iter()
            .map(|output| output.id)
            .collect::<BTreeSet<_>>();
        let mut next_generations = public.output_generations.clone();
        let mut generation_live = public.live_output_ids.clone();
        observe_public_output_generations(
            &mut next_generations,
            &mut generation_live,
            outputs,
        )?;
        if generation_live != next_live {
            return Err("public WM output-generation projection is incomplete".into());
        }
        let next_active = if next_live.contains(&public.active_output) {
            public.active_output
        } else {
            primary.id
        };
        let next_bounds = full_bounds.iter().copied().collect::<BTreeMap<_, _>>();
        let mut next_work_areas = public.work_areas.clone();
        next_work_areas.retain(|output, _| next_live.contains(output));
        for area in reduced {
            let Some(work) = area.work else {
                continue;
            };
            next_work_areas.insert(area.output, work);
        }
        let changed = public.outputs != outputs
            || public.live_output_ids != next_live
            || public.output_bounds != next_bounds
            || public.work_areas != next_work_areas
            || public.active_output != next_active;
        if !changed {
            return Ok(LiveWmRequestAdmission::Duplicate);
        }
        let mut affected_outputs = next_live.iter().copied().collect::<Vec<_>>();
        if let Some(index) = affected_outputs
            .iter()
            .position(|output| *output == primary.id)
        {
            affected_outputs.swap(0, index);
        }
        let cause = LivePublicPolicyCause {
            source: LiveWmProposalSource::Relayout,
            cause: sophia_protocol::PolicyRequestCause::SceneChanged,
            affected_outputs,
        };
        let mut next_queue = public.queue.clone();
        let admission = enqueue_public_policy_cause(
            &mut next_queue,
            public.in_flight_source,
            public.in_flight_request.is_some(),
            cause,
        );
        if admission == LiveWmRequestAdmission::RejectedCapacity {
            return Ok(admission);
        }
        if admission == LiveWmRequestAdmission::Duplicate {
            // A queued relayout still names the previous live set. Dropping the
            // replacement would leave a cause pointing at an output that no
            // longer exists, and issuing that cause fails the session. Merge
            // instead, the same way owner-observed dirty outputs fold in.
            let mut dirty = next_live.iter().copied().collect::<BTreeSet<_>>();
            materialize_public_dirty_cause(&mut next_queue, &mut dirty, public.in_flight_source);
        }
        public.outputs = outputs.to_vec();
        public.output_generations = next_generations;
        public.live_output_ids = next_live;
        public.output_bounds = next_bounds;
        for (output, work) in &next_work_areas {
            if public.work_areas.get(output) != Some(work) {
                crate::session_println!(
                    "sophia_live_work_area schema=1 output={} x={} y={} width={} height={} app_reservations={} shell_reservations={}",
                    output.raw(), work.x, work.y, work.width, work.height,
                    layout.active_output_reservations().len(), self.shell_reservation_bands.len(),
                );
            }
        }
        public.work_areas = next_work_areas;
        public.active_output = next_active;
        public.queue = next_queue;
        // Advance the reducer scene at the owner-observation boundary, before
        // a replacement request is issued. An in-flight response derived from
        // the retired output set is stale as soon as the owner accepts the new
        // topology; waiting until the next cycle would leave a click-through
        // window in which that response could still stage.
        let scene = public.snapshot(layout, chrome_style)?;
        if scene.generation > public.reducer.scene().generation {
            public.reducer.observe_scene(scene)?;
        }
        Ok(admission)
    }
}
