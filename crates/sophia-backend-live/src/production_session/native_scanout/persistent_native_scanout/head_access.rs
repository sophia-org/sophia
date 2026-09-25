impl LiveProductionNativeScanout {
        pub fn clone_render_device_file(&self) -> std::io::Result<std::fs::File> {
            self.groups
                .first()
                .ok_or_else(|| std::io::Error::other("native scanout has no DRM device group"))?
                .session
                .card()
                .try_clone_file()
        }

        /// Queries the fixed device inventory before frontend construction.
        /// Connector changes within these groups cannot broaden this intersection.
        pub fn dma_buf_import_formats(
            &self,
        ) -> Result<Vec<LiveDmaBufImportFormat>, LiveDmaBufCapabilityError> {
            if self.groups.is_empty() {
                return Err(LiveDmaBufCapabilityError::DeviceUnavailable);
            }
            let devices = self
                .groups
                .iter()
                .map(|group| {
                    let device = group
                        .session
                        .card()
                        .try_clone_file()
                        .map_err(|_| LiveDmaBufCapabilityError::DeviceUnavailable)?;
                    query_dma_buf_import_formats(device)
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(common_dma_buf_import_formats(&devices))
        }

        /// The desktop's logical outputs, one per `OutputId`.
        ///
        /// Heads are per connector and a mirror group has several sharing one
        /// logical output, so returning one entry per head would present a group as
        /// two outputs side by side. Everything above this is a topology, and a
        /// topology counts screens rather than cables.
        pub fn outputs(&self) -> Vec<sophia_engine::HeadlessOutput> {
            self.logical_outputs.clone()
        }

        /// The configured primary head driving a logical output.
        ///
        /// Named for what it returns. It was `output_index`, which read like a
        /// position in the output list and was passed one by four callers -- a
        /// coincidence that holds only while every output has exactly one head.
        ///
        /// Correct only for logical-output authority: reading the primary card,
        /// connector, or CRTC. A caller that submits, retires, or releases per
        /// head must use `head_indices` instead, or it will silently ignore the
        /// rest of a mirror group.
        pub fn primary_head_index(&self, output: OutputId) -> Option<usize> {
            if let Some(primary) = self
                .output_lifecycles
                .get(&output)
                .map(LiveProductionMirrorGroupLifecycle::primary_head)
                && let Some(index) = self.heads.iter().position(|head| {
                    head.enabled && head.output.id == output && head.head == primary
                })
            {
                return Some(index);
            }
            self.heads
                .iter()
                .position(|head| head.enabled && head.output.id == output)
        }

        /// The head whose refresh sets the composition cadence.
        ///
        /// Composition is one global tick, so exactly one head decides the rate
        /// every output is composed at. That head must be the desktop primary --
        /// the output `focus-at-startup` names, which the session publishes as
        /// `primary_output` -- and not whichever head enumerated first.
        ///
        /// `heads` is ordered by lowest `OutputId`, minted from the lowest
        /// connector object on the lowest card node. On a mixed-refresh desktop
        /// that order has nothing to do with which display the user is looking
        /// at: a 60Hz secondary enumerating ahead of a 120Hz primary capped
        /// composition for both, halving a double-buffered client to thirty
        /// frames a second.
        ///
        /// Falls back to the lowest *enabled* head when no primary is published
        /// yet, which is the case before the session's first output publication.
        /// A disabled head keeps its last refresh, so selecting one would pace
        /// the desktop from a display that is no longer scanning out.
        pub fn cadence_head(
            &self,
            primary_output: Option<OutputId>,
        ) -> Option<&LiveProductionNativeHead> {
            if let Some(output) = primary_output
                && let Some(index) = self.primary_head_index(output)
            {
                return self.heads.get(index);
            }
            let enabled = self
                .heads
                .iter()
                .map(|head| head.enabled)
                .collect::<Vec<_>>();
            super::refresh::fallback_cadence_index(&enabled).and_then(|index| self.heads.get(index))
        }

        /// The head driving a named connector.
        ///
        /// The one lookup that is exact for a mirror group: every head has its own
        /// connector even when several share a logical output, so a caller that must
        /// address one specific head asks by connector rather than by output.
        pub fn head_index_for_output_head(
            &self,
            output: OutputId,
            head_id: sophia_engine::RenderHeadId,
        ) -> Option<usize> {
            self.heads
                .iter()
                .position(|head| head.enabled && head.output.id == output && head.head == head_id)
        }

        /// Resolves a connector id when the caller has already established that
        /// the capability namespace is unambiguous.
        ///
        /// DRM connector ids are card-local, so callback and presentation paths
        /// must use the output-qualified lookup above. Startup topology mapping
        /// retains this facade because its named capability set is validated
        /// before it reaches this point.
        pub fn head_index_for_head(&self, head_id: sophia_engine::RenderHeadId) -> Option<usize> {
            self.heads.iter().position(|head| head.head == head_id)
        }

        /// Resolves a native connector id through the head table. This is the
        /// backend-boundary translation for callers that hold configuration or
        /// capability facts (connector names and ids) rather than heads.
        pub fn head_index_for_native_connector(&self, connector_id: u32) -> Option<usize> {
            let record = self
                .head_table
                .records()
                .iter()
                .find(|record| record.connector_id == connector_id)?;
            self.head_index_for_head(record.head)
        }

        /// Every head driving a logical output, in head order.
        pub fn head_indices(&self, output: OutputId) -> Vec<usize> {
            self.heads
                .iter()
                .enumerate()
                .filter(|(_, head)| head.enabled && head.output.id == output)
                .map(|(index, _)| index)
                .collect()
        }

        /// How many connectors drive each logical output, in output order.
        ///
        /// The topology owner compares this beside the output list: losing one head
        /// of a mirror group leaves the logical outputs unchanged, so a comparison
        /// on outputs alone would call that no change at all.
        pub fn head_fingerprint(&self) -> Vec<(OutputId, usize)> {
            let mut counts: BTreeMap<OutputId, usize> = BTreeMap::new();
            for head in self.heads.iter().filter(|head| head.enabled) {
                *counts.entry(head.output.id).or_default() += 1;
            }
            counts.into_iter().collect()
        }

        pub(crate) fn frame_owner(&self) -> crate::NativeFrameOwner {
            self.native_frame_owner
        }

        fn native_frame_identity(
            &self,
            index: usize,
            output: OutputId,
            frame: LiveProductionNativeFrameId,
        ) -> crate::LiveNativeFrameIdentity {
            let head = &self.heads[index];
            self.native_frame_owner
                .frame(output, head.head, head.target_generation, frame.raw())
        }

        fn arm_singleton_retirement(
            &self,
            index: usize,
            output: OutputId,
            runtime: &mut crate::LiveBackendRuntimeAssembly,
        ) {
            let expected = self.heads[index]
                .submitted_content
                .map(|content| self.native_frame_identity(index, output, content.frame()));
            runtime.set_native_retirement_witness(output, expected);
        }

        fn allocate_frame_id(&mut self) -> LiveProductionNativeFrameId {
            let frame = LiveProductionNativeFrameId::from_raw(self.next_frame_id);
            self.next_frame_id = self
                .next_frame_id
                .checked_add(1)
                .expect("native frame ID space exhausted");
            frame
        }

        fn allocate_head_candidate_id(&mut self) -> sophia_engine::HeadFrameCandidateId {
            let candidate =
                sophia_engine::HeadFrameCandidateId::from_raw(self.next_head_candidate_id);
            self.next_head_candidate_id = self
                .next_head_candidate_id
                .checked_add(1)
                .expect("native head candidate ID space exhausted");
            candidate
        }
}
