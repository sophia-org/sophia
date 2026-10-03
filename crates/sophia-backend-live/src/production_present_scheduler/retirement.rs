// Join per-output retirement permissions and their real clock evidence.
use super::*;

impl LiveProductionPresentScheduler {
    pub fn mark_output_retired(
        &mut self,
        retirement: LiveProductionPageFlipRetirement,
    ) -> Result<Option<sophia_engine::TransactionPresentationTerminal>, &'static str> {
        self.mark_output_retired_with_clocks(retirement, Default::default())
    }

    pub fn mark_output_retired_with_clocks(
        &mut self,
        retirement: LiveProductionPageFlipRetirement,
        clocks: crate::LiveNativeRetirementClocks,
    ) -> Result<Option<sophia_engine::TransactionPresentationTerminal>, &'static str> {
        let Some(in_flight) = self.in_flight.as_mut() else {
            return Err("output retirement has no in-flight Present cohort");
        };
        let present = match in_flight {
            LiveProductionInFlightPresent::Rendering(present)
            | LiveProductionInFlightPresent::Submitted(present) => present,
        };
        use sophia_engine::TransactionPresentationTransition as Transition;
        match present
            .output_cohort
            .mark_retired(retirement.output, retirement.ust)
        {
            Transition::Accepted => {
                present
                    .retirements
                    .insert(retirement.output, (retirement, clocks));
                Ok(None)
            }
            Transition::PhaseReady => {
                present
                    .retirements
                    .insert(retirement.output, (retirement, clocks));
                Ok(present.output_cohort.terminal())
            }
            Transition::Duplicate => Ok(None),
            _ => Err("Present cohort rejected output retirement"),
        }
    }
}
