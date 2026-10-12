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
    pub inspection_socket: Option<&'a Path>,
    /// Registered applications that launch confined; the default maps none.
    pub confinement: super::ApplicationConfinement<'a>,
}

/// Host endpoints come only from this Session's startup permissions, never
/// from an inherited environment or a protected role's grant set.
pub(crate) fn configure_host_application_environment(
    command: &mut Command,
    control_socket: Option<&Path>,
    inspection_socket: Option<&Path>,
) {
    for (name, socket) in [
        (sophia_runtime::SOPHIA_CONTROL_SOCKET_ENV, control_socket),
        (
            sophia_runtime::SOPHIA_WM_INSPECT_SOCKET_ENV,
            inspection_socket,
        ),
    ] {
        command.env_remove(name);
        if let Some(socket) = socket {
            command.env(name, socket);
        }
    }
}

pub fn spawn_catalog_process(
    command: &ApplicationLaunchCommand,
    environment: CatalogProcessEnvironment<'_>,
) -> std::io::Result<Child> {
    spawn_catalog_process_with_transaction(command, environment, None, None)
}

/// `identity` is the catalog identity of the entry this command was verified
/// from. A mapped registered application is spawned only confined; an
/// unmapped one, or a launch with no identity, takes the host path unchanged.
pub(crate) fn spawn_catalog_process_with_transaction(
    command: &ApplicationLaunchCommand,
    environment: CatalogProcessEnvironment<'_>,
    transaction: Option<u64>,
    identity: Option<&str>,
) -> std::io::Result<Child> {
    spawn_application_process(
        command,
        environment,
        identity,
        crate::diagnostics::application::LaunchContext {
            source: crate::diagnostics::application::LaunchSource::Catalog,
            transaction,
        },
    )
}

pub(crate) fn spawn_application_process(
    command: &ApplicationLaunchCommand,
    environment: CatalogProcessEnvironment<'_>,
    identity: Option<&str>,
    context: crate::diagnostics::application::LaunchContext,
) -> std::io::Result<Child> {
    // Recipe launches must enter the asynchronous namespace owner. Until that
    // owner consumes this command, neither catalog path may discard a request
    // and fall through to an ordinary host or legacy confined launch.
    if command.namespace_recipe.is_some() {
        return Err(std::io::Error::other(
            "namespace recipe requires the namespace launch owner",
        ));
    }
    if let Some(confined) = identity
        .and_then(super::ApplicationConfinement::registered_id)
        .and_then(|application| {
            environment.confinement.command(
                application,
                &command.executable,
                &command.arguments,
                command.working_directory.as_deref(),
            )
        })
    {
        return crate::diagnostics::application::spawn(&mut confined?, context);
    }
    let mut process = Command::new(&command.executable);
    configure_host_application_environment(
        &mut process,
        environment.control_socket,
        environment.inspection_socket,
    );
    process
        .args(&command.arguments)
        .env("DISPLAY", environment.display)
        .env("XAUTHORITY", environment.xauthority)
        .env_remove("ENV")
        .env_remove("BASH_ENV")
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if let Some(directory) = &command.working_directory {
        process.current_dir(directory);
    }
    crate::diagnostics::application::spawn(&mut process, context)
}

/// Returned together so the process cannot be attributed using just a numeric
/// transaction. The Session must move both fields into managed child custody.
pub struct NativeCatalogChild<C = Child> {
    pub child: C,
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
    spawn_native_catalog_with(
        connection,
        launches,
        launch,
        verified,
        |command, identity, transaction| {
            spawn_catalog_process_with_transaction(
                command,
                environment,
                Some(transaction),
                Some(identity),
            )
        },
    )
}

/// Consume exact catalog admission before invoking the Session's selected
/// process owner. The result may be asynchronous custody; it must be retained
/// immediately with the same origin. No deferred unowned execution is returned.
pub fn spawn_native_catalog_with<C>(
    connection: &ShellTransportConnection<'_>,
    launches: &mut SessionLaunchQueue,
    launch: Arc<NativeCatalogLaunch>,
    verified: Result<ApplicationLaunchCommand, String>,
    spawn: impl FnOnce(&ApplicationLaunchCommand, &str, u64) -> std::io::Result<C>,
) -> Result<NativeCatalogChild<C>, NativeCatalogSpawnError> {
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
    match spawn(&command, &launch.entry.identity, launch.transaction.raw()) {
        Ok(child) => Ok(NativeCatalogChild { child, launch }),
        Err(error) => {
            launches.cancel_native_catalog(&launch);
            Err(NativeCatalogSpawnError::Spawn(error))
        }
    }
}
