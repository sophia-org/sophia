impl LiveProductionNativeScanout {
        pub fn initialize_head_composition(
            &mut self,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
            frames: Vec<LiveProductionHeadCompositionFrame>,
        ) -> Result<LiveProductionNativeFrameId, Box<dyn std::error::Error>> {
            let has_head = !self.head_indices(output).is_empty();
            let initialized = self.initialize_semantic_head_transaction(output, runtime, frames);
            match initialized {
                Ok(frame) => Ok(frame),
                Err(error) => {
                    let error = match self.abort_semantic_startup_head_work(output) {
                        Ok(()) => error,
                        Err(abort) => format!(
                            "semantic startup failed: {error}; renderer abort failed: {abort}"
                        )
                        .into(),
                    };
                    finish_live_production_native_initialization(Err(error), has_head, || {
                        self.release_displayed_output(output, runtime)
                    })?;
                    unreachable!("failed initialization cannot settle successfully")
                }
            }
        }

        /// Whether outputs of one device group share a renderer thread.
        ///
        /// Opt-in until the shared worker is promoted on physical evidence.
        /// A head that renders alone cannot starve a sibling or misroute a
        /// result to one, so the failure modes this introduces do not exist
        /// until it is on, and the gate that proves them is the one that
        /// turns it on.
        fn shared_renderer_worker_enabled() -> bool {
            std::env::var("SOPHIA_ENABLE_SHARED_RENDERER_WORKER").is_ok_and(|value| value == "1")
        }

        /// Whether a head may hand an eligible client buffer straight to a
        /// plane instead of composing it.
        ///
        /// Opt-in until the row is promoted on physical evidence. Off, the
        /// exporter never even derives a candidate, so the session behaves
        /// exactly as it did before this row rather than taking a different
        /// path that happens to compose.
        fn direct_scanout_enabled() -> bool {
            std::env::var("SOPHIA_ENABLE_DIRECT_SCANOUT").is_ok_and(|value| value == "1")
        }

        /// Give one head a renderer worker: its group's shared thread when
        /// sharing is on, a thread of its own when it is not.
        ///
        /// Every path that brings a head up runs through here, because a head
        /// enabled the other way would quietly keep its own EGL display and
        /// its own copy of every imported image while the session reported
        /// itself as sharing.
        pub(crate) fn enable_head_renderer_worker(
            &mut self,
            index: usize,
        ) -> Result<(), Box<dyn std::error::Error>> {
            self.invalidate_layout_probes();
            // The head's own identity, so two exporters on one core never
            // collide in their replies, their slots, or their leases.
            // Group in the high bits, head in the low. Head identities repeat
            // across cards -- a two-card guest reports head=1 for both of its
            // outputs -- and while a key only has to be unique within the core
            // that holds it, uniqueness by construction beats uniqueness by an
            // argument about scope that a later change could quietly break.
            let group = u64::try_from(self.heads[index].group).unwrap_or(u64::MAX);
            self.exporters[index].set_output(crate::LiveRendererWorkerOutputKey::from_raw(
                (group << 32) | (self.heads[index].head.raw() & 0xFFFF_FFFF),
            ));
            // A mirror head never takes the direct path. Eligibility is proven
            // about one head's plan; a mirror cohort projects one scene into
            // several heads' own modes, so the buffer that would fill one head
            // exactly does not fill its siblings, and there is no single
            // client buffer that is the group's image. This is the first of
            // two refusals -- the mirror queue clears the verdict on the frame
            // itself -- because head membership can change after a head is
            // enabled, and neither check alone covers both orders.
            let mirrored = self.head_indices(self.heads[index].output.id).len() > 1;
            // Not yet, even when the session asked for it: `admit_direct_scanout`
            // turns it on once startup readiness has proven a picture reached
            // glass. A head enabled here would take the direct path before that
            // proof could be made, and the proof reads composed pixels.
            self.direct_scanout_admissible = Self::direct_scanout_enabled();
            self.exporters[index].set_direct_scanout_enabled(
                self.direct_scanout_admitted
                    && self.direct_scanout_admissible
                    && !mirrored
                    && !self.translation_motion_active,
            );
            if Self::shared_renderer_worker_enabled() {
                let group = self.heads[index].group;
                if self.groups[group].renderer_core.is_none() {
                    let discovery = self.groups[group].session.render_device_discovery()?;
                    self.groups[group].renderer_core = Some(
                        crate::NativeGbmRendererWorkerCore::spawn_with_image_import_devices(
                            crate::RenderDeviceDiscoveryBackend::open_render_device(&discovery),
                            self.image_import_device_fds()?,
                        )?,
                    );
                    self.render_devices.pending.remove(&group);
                    self.render_devices
                        .applied
                        .insert(group, self.render_devices.generation);
                }
                let core = self.groups[group]
                    .renderer_core
                    .as_ref()
                    .expect("group renderer core established above")
                    .clone();
                self.exporters[index].attach_shared_worker(&core);
            } else {
                let image_import_devices = self.image_import_device_fds()?;
                self.exporters[index]
                    .enable_worker_with_image_import_devices(image_import_devices)?;
                self.render_devices.pending.remove(&index);
                self.render_devices
                    .applied
                    .insert(index, self.render_devices.generation);
            }
            Ok(())
        }

        fn image_import_device_fds(&self) -> std::io::Result<Vec<std::os::fd::OwnedFd>> {
            self.image_import_devices
                .iter()
                .map(|device| rustix::io::fcntl_dupfd_cloexec(device, 0).map_err(Into::into))
                .collect()
        }

        pub fn enable_renderer_workers(&mut self) -> Result<usize, Box<dyn std::error::Error>> {
            let mut enabled = 0usize;
            for index in 0..self.exporters.len() {
                if !self.heads[index].enabled {
                    continue;
                }
                self.enable_head_renderer_worker(index)?;
                if !self.exporters[index].worker_enabled() {
                    return Err("native renderer worker was not established".into());
                }
                enabled = enabled.saturating_add(1);
            }
            Ok(enabled)
        }
}
