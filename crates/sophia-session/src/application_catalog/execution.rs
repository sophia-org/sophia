//! Process creation shared by legacy and native catalog consumers. No display
//! connection is opened here; launched applications use the configured endpoint.
use super::ApplicationLaunchCommand;
use crate::session_actions::{CatalogLaunchCause, NativeCatalogLaunch, SessionLaunchQueue};
use sophia_runtime::ShellTransportConnection;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

pub struct CatalogProcessEnvironment<'a> {
    pub display: &'a str,
    pub xauthority: &'a Path,
    pub control_socket: Option<&'a Path>,
}

pub fn spawn_catalog_process(
    command: &ApplicationLaunchCommand,
    environment: CatalogProcessEnvironment<'_>,
) -> std::io::Result<Child> {
    spawn_catalog_process_with_transaction(command, environment, None)
}

pub(crate) fn spawn_catalog_process_with_transaction(
    command: &ApplicationLaunchCommand,
    environment: CatalogProcessEnvironment<'_>,
    transaction: Option<u64>,
) -> std::io::Result<Child> {
    let mut process = Command::new(&command.executable);
    process
        .args(&command.arguments)
        .env("DISPLAY", environment.display)
        .env("XAUTHORITY", environment.xauthority)
        .env_remove(sophia_runtime::SOPHIA_CONTROL_SOCKET_ENV)
        .env_remove("ENV")
        .env_remove("BASH_ENV")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(socket) = environment.control_socket {
        process.env(sophia_runtime::SOPHIA_CONTROL_SOCKET_ENV, socket);
    }
    if let Some(directory) = &command.working_directory {
        process.current_dir(directory);
    }
    crate::diagnostics::application::spawn(
        &mut process,
        crate::diagnostics::application::LaunchContext {
            source: crate::diagnostics::application::LaunchSource::Catalog,
            transaction,
        },
    )
}

/// Returned together so the process cannot be attributed using just a numeric
/// transaction. The Session must move both fields into managed child custody.
pub struct NativeCatalogChild {
    pub child: Child,
    pub launch: Arc<NativeCatalogLaunch>,
}

#[derive(Debug)]
pub enum NativeCatalogSpawnError {
    Refused,
    Spawn(std::io::Error),
}

pub(super) fn connection_permits_cause(
    connection: &ShellTransportConnection<'_>,
    cause: &CatalogLaunchCause,
) -> bool {
    connection.content_grant() == Some(cause.grant())
        && match cause {
            CatalogLaunchCause::Transient(_) => connection.supports_native_launcher(),
            CatalogLaunchCause::Persistent(_) => connection.supports_persistent_catalog(),
        }
}

/// Run directly after verification, without another deferred effect queue.
/// Returned spawn failure settles only this exact admission. There is no retry
/// of a successful or uncertain execution attempt through this function.
pub fn spawn_native_catalog(
    connection: &ShellTransportConnection<'_>,
    launches: &mut SessionLaunchQueue,
    launch: Arc<NativeCatalogLaunch>,
    verified: Result<ApplicationLaunchCommand, String>,
    environment: CatalogProcessEnvironment<'_>,
) -> Result<NativeCatalogChild, NativeCatalogSpawnError> {
    let current = connection.content_grant();
    let command = match (current, verified) {
        (Some(grant), Ok(command))
            if connection_permits_cause(connection, &launch.cause)
                && launches.begin_native_catalog_execution(&launch, grant, &command) =>
        {
            command
        }
        _ => {
            // A stale duplicate after execution must not cancel the live child.
            // Only the caller's pre-execution failure may settle that queue slot.
            launches.reject_native_before_execution(&launch);
            return Err(NativeCatalogSpawnError::Refused);
        }
    };
    match spawn_catalog_process_with_transaction(
        &command,
        environment,
        Some(launch.transaction.raw()),
    ) {
        Ok(child) => Ok(NativeCatalogChild { child, launch }),
        Err(error) => {
            launches.cancel_native_catalog(&launch);
            Err(NativeCatalogSpawnError::Spawn(error))
        }
    }
}
