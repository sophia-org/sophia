// The desktop SDK owns these passive values; keep the historical facade.
pub use sophia_shell_protocol::shell::launcher::{
    SOPHIA_SHELL_CAPABILITY_APPLICATION_CATALOG, SOPHIA_SHELL_CAPABILITY_APPLICATION_LAUNCHER,
    SOPHIA_SHELL_LAUNCHER_REVISION, SOPHIA_SHELL_MAX_QUERY_BYTES, ShellLaunchOutcome,
    ShellLaunchStatus, ShellLauncherActivation, ShellLauncherActivationAck, ShellLauncherCandidate,
    ShellLauncherOperation, ShellLauncherOutcome, ShellLauncherRequest,
};
