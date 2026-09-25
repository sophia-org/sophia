impl LiveProductionNativeScanout {
    /// Resolves a provisional IPC topology against the live DRM master without
    /// mutating the currently published head table or any scanout ownership.
    pub fn plan_output_topology(
        &self,
        resolved: &crate::LiveResolvedOutputTopology,
    ) -> Result<LiveProductionNativeTopologyPlan, LiveProductionNativeTopologyPlanError> {
        let current = self
            .heads
            .iter()
            .map(|head| {
                LiveProductionNativeTopologyCurrentHead::new_with_target(
                    head.head,
                    head.enabled,
                    head.group,
                    head.output.id,
                    head.selection,
                    head.target_generation,
                    head.scale,
                    head.refresh_millihz,
                    head.transform,
                    head.mapping,
                    head.vrr,
                )
            })
            .collect::<Vec<_>>();
        plan_live_production_native_topology(&current, resolved, |current, timing| {
            crate::resolve_native_connector_mode(
                self.groups[current.card_index].session.card(),
                current.selection.connector_handle(),
                timing,
            )
            .map_err(|error| LiveProductionNativeTopologyPlanError::Native(error.to_string()))
        })
    }

    /// Reconstructs the still-published topology as render targets for a
    /// rollback pool. This never consults the provisional candidate: logical
    /// positions come from the published authority snapshot, while native sizes
    /// and generations come from the live head owner.
    pub fn published_output_topology(
        &self,
        snapshot: &sophia_protocol::OutputAuthoritySnapshot,
    ) -> Result<crate::LiveResolvedOutputTopology, LiveProductionNativeTopologyPlanError> {
        let capabilities = self
            .output_capabilities()
            .map_err(|error| LiveProductionNativeTopologyPlanError::Native(error.to_string()))?;
        let capability_by_head = capabilities
            .iter()
            .filter_map(|capability| capability.head().map(|head| (head, capability)))
            .collect::<BTreeMap<_, _>>();
        let current = self
            .heads
            .iter()
            .map(|head| {
                LiveProductionNativeTopologyCurrentHead::new_with_target(
                    head.head,
                    head.enabled,
                    head.group,
                    head.output.id,
                    head.selection,
                    head.target_generation,
                    head.scale,
                    head.refresh_millihz,
                    head.transform,
                    head.mapping,
                    head.vrr,
                )
            })
            .collect::<Vec<_>>();
        project_live_production_published_topology(&current, snapshot, |native| {
            capability_by_head
                .get(&native.head)
                .map(|capability| capability.selected_mode())
                .ok_or(LiveProductionNativeTopologyPlanError::PublishedSnapshotMismatch)
        })
    }
}
