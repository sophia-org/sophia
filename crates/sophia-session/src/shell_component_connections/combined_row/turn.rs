//! G3-5d gate 4 C2: the split combined turn as production code of the row.
//! Moved text-unchanged from tests/support/combined_row_steps.rs (d-2b1):
//! the lending `with_connection_and_row`, step 1 `turn_combined_images`
//! (the turn's one hooked image-owner service per slot) and step 3
//! `turn_combined_walk` (every slot's walk, then its `take_error`). A
//! private child of `combined_row`, under the parent's feature gate, so it
//! reaches the row's private items as the steps mount does; the only test
//! field it reads, `image_fault`, is read under `cfg(test)`. Gate 4 C3
//! calls all three from the owner pass.
use super::*;

impl ShellComponentConnections {
    /// `with_connection` that also lends the slot's row (G3-5c C2): the same
    /// Connected-phase check, and the row borrowed beside the transport.
    pub(crate) fn with_connection_and_row<R>(
        &mut self,
        key: ComponentConnectionKey,
        service: impl FnOnce(&mut ShellTransportConnection<'_>, &mut CombinedRow) -> R,
    ) -> Result<R, ComponentConnectionError> {
        self.require_phase(key, ComponentConnectionPhase::Connected)?;
        let connection = &mut self.connections[key.slot];
        Ok(service(
            &mut connection.transport.connection(&mut self.epochs),
            &mut connection.combined,
        ))
    }

    /// Step 1: the turn's one hooked image-owner service per slot, through
    /// `turn_with` exactly as `turn_combined` runs it, without the walk.
    pub(crate) fn turn_combined_images<N: ShellContentNative>(
        &mut self,
        turn: &mut CombinedTurn<'_, N>,
    ) -> Failures {
        self.turn_with(|slot, connection, epochs| {
            let instant = turn.instant;
            let Some(owner) = connection.images.as_mut() else {
                return Ok(());
            };
            owner.service_combined(
                &mut connection.transport,
                epochs,
                content_now_msec(),
                instant,
                &mut CombinedHooks {
                    row: &mut connection.combined,
                    turn: &mut *turn,
                    slot,
                },
            )?;
            #[cfg(test)]
            if let Some(reason) = connection.combined.image_fault.take() {
                return Err(ShellTransportError::Io(reason.into()));
            }
            Ok(())
        })
    }

    /// Step 3: every slot's walk, whatever its phase, then its `take_error`.
    /// An error of the slot's current, unrevoked attempt is that slot's
    /// failure; any other becomes a cleanup diagnostic (R1).
    pub(crate) fn turn_combined_walk<N: ShellContentNative>(
        &mut self,
        native: &mut N,
        instant: Instant,
    ) -> Failures {
        let mut failures: Failures = std::array::from_fn(|_| None);
        for (slot, (connection, failure)) in self
            .connections
            .iter_mut()
            .zip(failures.iter_mut())
            .enumerate()
        {
            let current = connection
                .attempt
                .filter(|(_, phase)| *phase != ComponentConnectionPhase::Revoked)
                .map(|(grant, _)| grant);
            let view = connection.transport.connection(&mut self.epochs);
            connection.combined.walk(
                &mut *native,
                current,
                &|binding: FrameCopyBinding, job: u64| {
                    view.content_frame_candidate_state(binding, job)
                },
                instant,
            );
            let error = connection.combined.take_error(current);
            if let (Some(grant), Some(error)) = (current, error) {
                *failure = Some((ComponentConnectionKey { slot, grant }, error));
            }
        }
        failures
    }
}
