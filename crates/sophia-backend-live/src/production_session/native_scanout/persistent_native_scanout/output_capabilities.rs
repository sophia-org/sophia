use super::*;

impl LiveProductionNativeScanout {
    pub fn output_capabilities(&self) -> std::io::Result<Vec<crate::LibdrmNativeOutputCapability>> {
        let mut capabilities = Vec::new();
        for (group_index, group) in self.groups.iter().enumerate() {
            for capability in group.session.output_capabilities()? {
                let head = self
                    .head_table
                    .records()
                    .iter()
                    .find(|record| {
                        record.card_index == group_index
                            && record.connector_id == capability.connector_id()
                    })
                    .map(|record| record.head)
                    .ok_or_else(|| {
                        std::io::Error::other(
                            "native capability has no card-qualified opaque head identity",
                        )
                    })?;
                capabilities.push(capability.bind_head(head)?);
            }
        }
        capabilities.sort_by_key(|capability| {
            (
                capability.output().raw(),
                capability
                    .head()
                    .map_or(0, sophia_engine::RenderHeadId::raw),
            )
        });
        Ok(capabilities)
    }

    /// Projects the backend-private head table into the exclusive output
    /// authority's bounded, connector-neutral snapshot. The public head IDs are
    /// opaque aliases of Engine head IDs; card, connector, CRTC, and plane
    /// identities remain in `head_table`.
    pub fn output_authority_snapshot(
        &self,
        topology_epoch: u64,
    ) -> Result<sophia_protocol::OutputAuthoritySnapshot, crate::LiveOutputAuthorityProjectionError>
    {
        let capabilities = self.output_capabilities().map_err(|error| {
            crate::LiveOutputAuthorityProjectionError::NativeCapability(error.to_string())
        })?;
        let mut snapshot = crate::project_live_output_authority_snapshot(
            &capabilities,
            &self.outputs(),
            topology_epoch,
        )?;
        let mappings = self
            .heads
            .iter()
            .map(|head| (head.head, head.mapping))
            .collect();
        crate::apply_live_output_authority_head_mappings(&mut snapshot, &mappings)?;
        Ok(snapshot)
    }

    pub fn head_render_targets(
        &self,
        output: sophia_protocol::OutputId,
    ) -> Vec<sophia_engine::HeadRenderTarget> {
        native_facts::head_render_targets(&self.heads, output)
    }
}
