//! G3-5c: the combined arm of `submit_bundle`, which joins a bundle with the
//! row a controlled caller lent (REV2 C1-C4, REV3). A bundle with no exact
//! entry takes the ordinary arm unchanged. A matched decision is marked
//! `Consumed` by the join, before any fallible step here. Every Release
//! comes before the candidate's terminal, through the row's one rule.
//!
//! Gate 4 C3b: under W the call is witnessed by one `consume` record whose
//! fields are set at the statements that decide them (spec 03 3.1); the
//! records add no call, decision or retry, and every `?` is today's.
use super::*;
#[cfg(all(
    feature = "experimental-combined-frame-job",
    feature = "experimental-native-frame-transaction",
    feature = "native-session",
    feature = "diagnostic-combined-witness"
))]
use crate::shell_component_connections::combined_row::witness::ConsumeWitness;
use crate::shell_component_connections::combined_row::{
    CombinedConsume, CombinedJoin, CombinedRow,
};
use sophia_backend_live::{
    CombinedAcceptError, CombinedRecompose, LiveProductionCpuScene, LiveProductionVisualRuntime,
    LiveShellContentLayer, ShellContentNative,
};

/// Gate 4 C3b: the witness's identity of an ordinary entry; nothing
/// without the witness.
#[cfg(all(
    feature = "experimental-combined-frame-job",
    feature = "experimental-native-frame-transaction",
    feature = "native-session",
    feature = "diagnostic-combined-witness"
))]
// Boxed so the witness does not grow `CombinedEntry` past clippy's variant
// size limit: one allocation per W ordinary entry, freed at the submission read.
type EntryWitness = Box<crate::shell_component_connections::combined_row::witness::WitnessEntry>;
#[cfg(not(all(
    feature = "experimental-combined-frame-job",
    feature = "experimental-native-frame-transaction",
    feature = "native-session",
    feature = "diagnostic-combined-witness"
)))]
type EntryWitness = ();

/// What the runtime step of `submit_bundle` does after the arm.
pub(super) enum CombinedEntry {
    /// The ordinary install, with this frame: no exact entry, a scene-only
    /// change, or F5's duplicate install.
    Ordinary(LiveShellContentFrame, EntryWitness),
    /// The accept committed: Prepared and pending follow, with no install.
    Accepted,
    /// The candidate took its failure terminal: no Prepared, no pending.
    Finished,
}

#[cfg(all(
    feature = "experimental-combined-frame-job",
    feature = "experimental-native-frame-transaction",
    feature = "native-session",
    feature = "diagnostic-combined-witness"
))]
macro_rules! ordinary {
    ($witness:ident, $frame:expr) => {
        CombinedEntry::Ordinary($frame, Box::new($witness.entry()))
    };
}
#[cfg(not(all(
    feature = "experimental-combined-frame-job",
    feature = "experimental-native-frame-transaction",
    feature = "native-session",
    feature = "diagnostic-combined-witness"
)))]
macro_rules! ordinary {
    ($witness:ident, $frame:expr) => {
        CombinedEntry::Ordinary($frame, ())
    };
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
        crate::combined_witness!(let w = ConsumeWitness::new(consume.witness););
        let joined = row.join(bundle);
        crate::combined_witness!(w.joined(&joined, row););
        let job = match joined {
            CombinedJoin::Ordinary => {
                crate::combined_witness!(w.entry.set("ordinary"););
                return Ok(ordinary!(w, frame));
            }
            CombinedJoin::Invariant(job) => {
                return Err(invariant(job, "a bundle met an entry no bundle can meet"));
            }
            CombinedJoin::Failed(job) => {
                crate::combined_witness!(w.terminal_requested(););
                let terminal = transport.content_renderer_failed_at(grant, output, generation, now);
                crate::combined_witness!(w.terminal_result(terminal.is_ok()););
                terminal?;
                if !row.consumed_failed(job) {
                    return Err(invariant(job, "a joined Failed entry vanished"));
                }
                crate::combined_witness!(w.entry.set("finished"););
                return Ok(CombinedEntry::Finished);
            }
            // The Debt stays until its Release transfers.
            CombinedJoin::Debt(_) => {
                crate::combined_witness!(w.terminal_requested(););
                let terminal = transport.content_renderer_failed_at(grant, output, generation, now);
                crate::combined_witness!(w.terminal_result(terminal.is_ok()););
                terminal?;
                crate::combined_witness!(w.entry.set("finished"););
                return Ok(CombinedEntry::Finished);
            }
            CombinedJoin::Held(job) => job,
        };
        // No native authority: the decision is recorded, and the next walk
        // releases the Held lease (R2-2).
        let Some(native) = native else {
            crate::combined_witness!(w.native.set("absent"); w.terminal_requested(););
            let terminal = transport.content_renderer_failed_at(grant, output, generation, now);
            crate::combined_witness!(w.terminal_result(terminal.is_ok()););
            terminal?;
            crate::combined_witness!(w.entry.set("finished"););
            return Ok(CombinedEntry::Finished);
        };
        crate::combined_witness!(w.native.set("present"););
        let (head, dispatch, deadline) = row
            .consumption(job)
            .ok_or_else(|| invariant(job, "a joined Held entry vanished"))?;
        // C3: the admission's own deadline gates consumption.
        if consume.instant >= deadline {
            crate::combined_witness!(w.deadline.set("past"););
            let declined = decline(row, native, job);
            crate::combined_witness!(w.declined(&declined, row););
            declined?;
            crate::combined_witness!(w.terminal_requested(););
            let terminal = transport.content_renderer_failed_at(grant, output, generation, now);
            crate::combined_witness!(w.terminal_result(terminal.is_ok()););
            terminal?;
            crate::combined_witness!(w.entry.set("finished"););
            return Ok(CombinedEntry::Finished);
        }
        crate::combined_witness!(w.deadline.set("ok"););
        // C1: authority before the scene; a scene change never excuses it.
        let checked =
            native.check_combined_consumption(runtime, &frame, layer, head, &dispatch.accept(job));
        crate::combined_witness!(w.checked(&checked););
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
                Err(error) => {
                    crate::combined_witness!(w.classify.set("error"););
                    Some(Refused::Error(error))
                }
                Ok(CombinedRecompose::TopologyChange) => {
                    crate::combined_witness!(w.classify.set("topology"););
                    Some(Refused::Terminal)
                }
                Ok(CombinedRecompose::CandidateFailure) => {
                    crate::combined_witness!(w.classify.set("candidate_failure"););
                    Some(Refused::Terminal)
                }
                Ok(CombinedRecompose::SceneChurn) => {
                    crate::combined_witness!(w.classify.set("churn"););
                    Some(Refused::Ordinary)
                }
                Ok(CombinedRecompose::Match) => {
                    crate::combined_witness!(w.classify.set("match"););
                    None
                }
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
                    crate::combined_witness!(w.accept.set("ok"););
                    if !row.accepted(job) {
                        return Err(invariant(job, "an accepted entry vanished"));
                    }
                    crate::combined_witness!(w.entry.set("accepted"););
                    return Ok(CombinedEntry::Accepted);
                }
                Err(failed) => {
                    crate::combined_witness!(w.accept_refused(&failed.error););
                    (refused(job, failed.error), failed.frame)
                }
            },
        };
        let declined = decline(row, native, job);
        crate::combined_witness!(w.declined(&declined, row););
        declined?;
        match refusal {
            Refused::Error(error) => Err(error),
            Refused::Terminal => {
                crate::combined_witness!(w.terminal_requested(););
                let terminal = transport.content_renderer_failed_at(grant, output, generation, now);
                crate::combined_witness!(w.terminal_result(terminal.is_ok()););
                terminal?;
                crate::combined_witness!(w.entry.set("finished"););
                Ok(CombinedEntry::Finished)
            }
            Refused::Ordinary => {
                crate::combined_witness!(w.entry.set("ordinary"););
                Ok(ordinary!(w, frame))
            }
        }
    }
}
