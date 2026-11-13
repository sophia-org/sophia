//! G3-5c: the combined arm of `submit_bundle`, which joins a bundle with the
//! row a controlled caller lent (REV2 C1-C4, REV3). A bundle with no exact
//! entry takes the ordinary arm unchanged. A matched decision is marked
//! `Consumed` by the join, before any fallible step here. Every Release
//! comes before the candidate's terminal, through the row's one rule.
use super::*;
use crate::shell_component_connections::combined_row::{
    CombinedConsume, CombinedJoin, CombinedRow,
};
use sophia_backend_live::{
    CombinedAcceptError, CombinedRecompose, LiveProductionCpuScene, LiveProductionVisualRuntime,
    LiveShellContentLayer, ShellContentNative,
};

/// What the runtime step of `submit_bundle` does after the arm.
pub(super) enum CombinedEntry {
    /// The ordinary install, with this frame: no exact entry, a scene-only
    /// change, or F5's duplicate install.
    Ordinary(LiveShellContentFrame),
    /// The accept committed: Prepared and pending follow, with no install.
    Accepted,
    /// The candidate took its failure terminal: no Prepared, no pending.
    Finished,
}

/// What a refused consumption check or accept leads to, after the decline.
enum Refused {
    Error(Box<dyn std::error::Error>),
    Terminal,
    Ordinary,
}

fn invariant(job: u64, what: &str) -> Box<dyn std::error::Error> {
    ShellTransportError::CombinedRow(format!("combined job {job}: {what}")).into()
}

/// The accept table of design-01 section 3, for the check and the accept.
fn refused(job: u64, error: CombinedAcceptError) -> Refused {
    match error {
        CombinedAcceptError::Runtime(error) => Refused::Error(error),
        CombinedAcceptError::Output => Refused::Error(invariant(
            job,
            "the bundle's output is not the admitted plan's",
        )),
        CombinedAcceptError::RetirementClaim | CombinedAcceptError::Native(_) => Refused::Terminal,
        // F5: the defensive duplicate-install invariant, never a normal
        // success. A new generation cannot equal the installed frame.
        CombinedAcceptError::Unchanged => Refused::Ordinary,
    }
}

/// Releases a joined Held job; an entry that is not Held here fails closed.
fn decline<N: ShellContentNative>(
    row: &mut CombinedRow,
    native: &mut N,
    job: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    row.decline(native, job)
        .map(|_| ())
        .ok_or_else(|| invariant(job, "a joined Held entry is not Held"))
}

impl LiveContentSession {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn consume_combined<N: ShellContentNative>(
        &mut self,
        transport: &mut ShellTransportConnection<'_>,
        runtime: &mut LiveProductionVisualRuntime,
        scene: &LiveProductionCpuScene,
        native: Option<&mut N>,
        consume: &mut CombinedConsume<'_>,
        bundle: &ContentRenderBundle,
        frame: LiveShellContentFrame,
        layer: LiveShellContentLayer,
        now: u64,
    ) -> Result<CombinedEntry, Box<dyn std::error::Error>> {
        let (grant, output, generation) =
            (bundle.grant, bundle.output, bundle.candidate_generation);
        let row = &mut *consume.row;
        let job = match row.join(bundle) {
            CombinedJoin::Ordinary => return Ok(CombinedEntry::Ordinary(frame)),
            CombinedJoin::Invariant(job) => {
                return Err(invariant(job, "a bundle met an entry no bundle can meet"));
            }
            CombinedJoin::Failed(job) => {
                transport.content_renderer_failed_at(grant, output, generation, now)?;
                if !row.consumed_failed(job) {
                    return Err(invariant(job, "a joined Failed entry vanished"));
                }
                return Ok(CombinedEntry::Finished);
            }
            // The Debt stays until its Release transfers.
            CombinedJoin::Debt(_) => {
                transport.content_renderer_failed_at(grant, output, generation, now)?;
                return Ok(CombinedEntry::Finished);
            }
            CombinedJoin::Held(job) => job,
        };
        // No native authority: the decision is recorded, and the next walk
        // releases the Held lease (R2-2).
        let Some(native) = native else {
            transport.content_renderer_failed_at(grant, output, generation, now)?;
            return Ok(CombinedEntry::Finished);
        };
        let (head, dispatch, deadline) = row
            .consumption(job)
            .ok_or_else(|| invariant(job, "a joined Held entry vanished"))?;
        // C3: the admission's own deadline gates consumption.
        if consume.instant >= deadline {
            decline(row, native, job)?;
            transport.content_renderer_failed_at(grant, output, generation, now)?;
            return Ok(CombinedEntry::Finished);
        }
        // C1: authority before the scene; a scene change never excuses it.
        let checked =
            native.check_combined_consumption(runtime, &frame, layer, head, &dispatch.accept(job));
        let refusal = match checked {
            Err(error) => Some(refused(job, error)),
            Ok(()) => match native.classify_combined_frame(
                runtime,
                scene,
                layer,
                &frame,
                consume.cursor,
                consume.plane,
                dispatch,
            ) {
                Err(error) => Some(Refused::Error(error)),
                Ok(CombinedRecompose::TopologyChange | CombinedRecompose::CandidateFailure) => {
                    Some(Refused::Terminal)
                }
                Ok(CombinedRecompose::SceneChurn) => Some(Refused::Ordinary),
                Ok(CombinedRecompose::Match) => None,
            },
        };
        let (refusal, frame) = match refusal {
            Some(refusal) => (refusal, frame),
            None => match native.accept_combined_shell_content(
                runtime,
                frame,
                layer,
                head,
                dispatch.accept(job),
            ) {
                Ok(()) => {
                    if !row.accepted(job) {
                        return Err(invariant(job, "an accepted entry vanished"));
                    }
                    return Ok(CombinedEntry::Accepted);
                }
                Err(failed) => (refused(job, failed.error), failed.frame),
            },
        };
        decline(row, native, job)?;
        match refusal {
            Refused::Error(error) => Err(error),
            Refused::Terminal => {
                transport.content_renderer_failed_at(grant, output, generation, now)?;
                Ok(CombinedEntry::Finished)
            }
            Refused::Ordinary => Ok(CombinedEntry::Ordinary(frame)),
        }
    }
}
