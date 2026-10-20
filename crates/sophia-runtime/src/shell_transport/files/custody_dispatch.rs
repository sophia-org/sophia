//! The shell file owner observes once for its bounded control turn. Other
//! transports remain caller-fresh until they define their own dispatch scope.
use super::{ShellFileWire, ShellReadinessObservation, ShellTransportError};

struct Scope<'a>(&'a mut ShellFileWire);

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        // End exactly once, including begin/turn errors and unwinding, before
        // the caller resumes its final fresh check and image service.
        self.0.server.export_mut().end_custody_dispatch();
    }
}

pub(super) fn turn(
    wire: &mut ShellFileWire,
    flush_only: bool,
    readiness: Option<ShellReadinessObservation>,
) -> Result<bool, ShellTransportError> {
    // Install the guard before calling the owner, including a partially
    // initialized scope whose begin operation fails or panics.
    let scope = Scope(wire);
    scope
        .0
        .server
        .export_mut()
        .begin_custody_dispatch()
        .map_err(|_| ShellTransportError::NotConnected)?;
    scope.0.service_turn(flush_only, readiness)
}
