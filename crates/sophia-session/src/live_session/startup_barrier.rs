//! Admitting one client's first transaction before a second client starts.

use std::sync::mpsc::Receiver;
use std::time::Duration;

use sophia_x_authority::XAuthorityObservedTransactionBatch;

use super::PersistentXtermSessionConfig;

/// How long to wait for the primary client's first committed surface.
const STARTUP_FRAME_DEADLINE: Duration = Duration::from_secs(5);

/// Holds the second client back until the first has committed a surface.
///
/// Optimized startup otherwise lets two clients race for the first committed
/// surface, and initial focus lands on whichever wins. Two clients starting is
/// the whole hazard, so that is the whole condition for waiting.
///
/// Whether missing the frame is fatal is a second and separate question, and
/// the startup-proof predicate was quietly answering both. A proof run that
/// never saw a startup frame has lost the thing it was measuring and should
/// say so. An ordinary session has only lost determinism, which is what it had
/// before this barrier existed, so it proceeds. Making the deadline fatal for
/// every session would put a five second clock on the ordinary path, where
/// load alone could end a session.
pub(super) fn await_primary_startup_frame(
    config: &PersistentXtermSessionConfig,
    authority_receiver: &Receiver<XAuthorityObservedTransactionBatch>,
) -> Result<Option<XAuthorityObservedTransactionBatch>, String> {
    if !(config.secondary_terminal || config.applications.startup.len() > 1) {
        return Ok(None);
    }
    match authority_receiver.recv_timeout(STARTUP_FRAME_DEADLINE) {
        Ok(batch) => Ok(Some(batch)),
        Err(error) if config.startup_proof_requested() => Err(format!(
            "primary xterm did not publish a startup frame: {error}"
        )),
        Err(error) => {
            crate::session_println!(
                "sophia_live_session_lifecycle schema=1 status=startup_barrier_elapsed reason={error}"
            );
            Ok(None)
        }
    }
}
