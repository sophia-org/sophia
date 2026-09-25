impl LiveWmSession {
    fn overview_publication(&self) -> Option<sophia_engine::PolicyOverviewPublication> {
        self.public.as_ref().filter(|p| p.configured && p.selected_capabilities
            & sophia_protocol::SOPHIA_WM_CAPABILITY_OVERVIEW != 0)
            .map(|p| p.reducer.overview_publication())
    }

    fn enqueue_overview(&mut self, selection: Option<sophia_engine::OverviewSelection>)
        -> Result<LiveWmRequestAdmission, Box<dyn std::error::Error>> {
        let public = self.public.as_mut().ok_or("overview WM unavailable")?;
        if public.selected_capabilities & sophia_protocol::SOPHIA_WM_CAPABILITY_OVERVIEW == 0 {
            return Err("WM does not support overview".into());
        }
        let cause = match selection {
            None => sophia_protocol::PolicyRequestCause::OverviewQuery,
            Some(selection) => sophia_protocol::PolicyRequestCause::OverviewSelection {
                activation_serial: public.mint_transaction()?.raw(),
                output: selection.output, output_generation: selection.output_generation,
                workspace: selection.workspace, target: selection.target,
            },
        };
        Ok(public.queue_cause(LivePublicPolicyCause {
            source: LiveWmProposalSource::Action(SHELL_OVERVIEW_SHORTCUT_ACTION), cause,
            affected_outputs: public.all_outputs(public.active_output),
        }))
    }
}
