use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::{
    BROWSER_APPLICATION_ID, LAUNCHER_APPLICATION_ID, TERMINAL_APPLICATION_ID, WmSessionAction,
};

pub(super) fn session_action_evidence_name(action: WmSessionAction) -> &'static str {
    match action {
        WmSessionAction::LaunchApplication { application }
            if application == TERMINAL_APPLICATION_ID =>
        {
            "LaunchTerminal"
        }
        WmSessionAction::LaunchApplication { application }
            if application == LAUNCHER_APPLICATION_ID =>
        {
            "LaunchApplicationMenu"
        }
        WmSessionAction::LaunchApplication { application }
            if application == BROWSER_APPLICATION_ID =>
        {
            "LaunchBrowser"
        }
        WmSessionAction::LaunchApplication { .. } => "LaunchApplication",
        WmSessionAction::CloseFocused => "CloseFocused",
        WmSessionAction::Logout => "Logout",
        WmSessionAction::ReloadProfile => "ReloadProfile",
        WmSessionAction::RestartWm => "RestartWm",
    }
}

pub(super) use crate::session_actions::SessionLaunchCommand as SessionApplicationSpec;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SessionApplicationConfig {
    pub(super) applications: BTreeMap<String, SessionApplicationSpec>,
    pub(super) command_names: BTreeSet<String>,
    pub(super) startup: Vec<String>,
    pub(super) terminal: Option<String>,
    pub(super) launcher: Option<String>,
    pub(super) application_catalog: Option<String>,
    pub(super) browser: Option<String>,
    pub(super) logout_enabled: bool,
}

impl Default for SessionApplicationConfig {
    fn default() -> Self {
        Self {
            applications: BTreeMap::new(),
            command_names: BTreeSet::new(),
            startup: Vec::new(),
            terminal: None,
            launcher: None,
            application_catalog: None,
            browser: None,
            logout_enabled: true,
        }
    }
}

impl SessionApplicationConfig {
    pub(super) fn named_command(&self, name: &str) -> Option<&SessionApplicationSpec> {
        self.command_names
            .contains(name)
            .then(|| self.applications.get(name))
            .flatten()
    }

    fn application_for_profile_name(
        &self,
        name: &str,
    ) -> Result<Option<String>, SessionApplicationConfigError> {
        if self.applications.contains_key(name) {
            return Ok(Some(name.to_owned()));
        }
        let mut matches = self
            .applications
            .values()
            .filter(|application| {
                application
                    .executable
                    .file_name()
                    .is_some_and(|file| file == std::ffi::OsStr::new(name))
            })
            .map(|application| application.id.clone());
        let selected = matches.next();
        if matches.next().is_some() {
            return Err(SessionApplicationConfigError::AmbiguousProfileIdentity(
                name.to_owned(),
            ));
        }
        Ok(selected)
    }

    pub(super) fn apply_desktop_candidate(
        &mut self,
        candidate: &sophia_config::DesktopSessionCandidate,
        terminal_overridden: bool,
        browser_overridden: bool,
        startup_overridden: bool,
    ) -> Result<(), SessionApplicationConfigError> {
        self.application_catalog = candidate.application_catalog.clone();
        if !terminal_overridden && let Some(terminal) = candidate.terminal.as_deref() {
            self.terminal = self.application_for_profile_name(terminal)?;
        }
        if !browser_overridden && let Some(browser) = candidate.browser.as_deref() {
            self.browser = self.application_for_profile_name(browser)?;
        }
        if !startup_overridden && let Some(startup) = candidate.startup.as_deref() {
            let mut selected = Vec::with_capacity(startup.len());
            for name in startup {
                let id = self.application_for_profile_name(name)?.ok_or_else(|| {
                    SessionApplicationConfigError::UnknownApplication(name.clone())
                })?;
                if selected.contains(&id) {
                    return Err(SessionApplicationConfigError::DuplicateStartup(id));
                }
                selected.push(id);
            }
            self.startup = selected;
        }
        if let Some(enabled) = candidate.logout_enabled {
            self.logout_enabled = enabled;
        }
        Ok(())
    }

    /// Which of a profile's shortcuts this session can actually perform.
    ///
    /// A binding naming a capability the session does not have would do
    /// nothing when pressed, so an author who wrote it wants to hear about it
    /// and the session refuses. A *compiled default* profile has no author: it
    /// is the fallback loaded whenever no user profile is found, so its
    /// bindings describe a full desktop rather than this session's intent.
    /// Refusing on those made every single-application session unstartable --
    /// `--no-config`, and any machine with no `~/.config/sophia` at all -- for
    /// nineteen days, because nothing that ran regularly took that path.
    ///
    /// So a default's unsatisfiable bindings are dropped and reported, and an
    /// explicit profile's are still an error.
    pub(super) fn validate_shortcuts(
        &self,
        shortcuts: &sophia_config::DesktopShortcutCandidate,
        shell_enabled: bool,
        profile_is_compiled_default: bool,
    ) -> Result<Vec<sophia_config::DesktopSessionShortcut>, SessionApplicationConfigError> {
        let mut dropped = Vec::new();
        for binding in &shortcuts.bindings {
            let available = match binding.target {
                sophia_config::DesktopShortcutTarget::LaunchApplication(ref name) => {
                    self.named_command(name).is_some()
                }
                sophia_config::DesktopShortcutTarget::PolicyAction(_) => true,
                sophia_config::DesktopShortcutTarget::Session(
                    sophia_config::DesktopSessionShortcut::CloseFocused,
                ) => true,
                sophia_config::DesktopShortcutTarget::Session(
                    sophia_config::DesktopSessionShortcut::Logout,
                ) => self.logout_enabled,
                sophia_config::DesktopShortcutTarget::Session(
                    sophia_config::DesktopSessionShortcut::LaunchTerminal,
                ) => self.terminal.is_some(),
                sophia_config::DesktopShortcutTarget::Session(
                    sophia_config::DesktopSessionShortcut::LaunchBrowser,
                ) => self.browser.is_some(),
                sophia_config::DesktopShortcutTarget::Session(
                    sophia_config::DesktopSessionShortcut::WindowSwitcher
                    | sophia_config::DesktopSessionShortcut::ShortcutHelp,
                ) => shell_enabled,
                sophia_config::DesktopShortcutTarget::Session(
                    sophia_config::DesktopSessionShortcut::ApplicationLauncher,
                ) => shell_enabled && self.application_catalog.is_some(),
                // Always available. Neither needs a configured application,
                // and a desktop whose configuration is wrong is the one that
                // needs them most.
                sophia_config::DesktopShortcutTarget::Session(
                    sophia_config::DesktopSessionShortcut::ReloadProfile
                    | sophia_config::DesktopSessionShortcut::RestartWm,
                ) => true,
                // A lock needs an authenticator to end it. Until the Session
                // authenticator lands (t293) a profile asking for one is
                // refused rather than offered a lock nobody could open.
                sophia_config::DesktopShortcutTarget::Session(
                    sophia_config::DesktopSessionShortcut::Lock,
                ) => false,
            };
            if available {
                continue;
            }
            if !profile_is_compiled_default {
                return Err(SessionApplicationConfigError::UnavailableShortcutCapability);
            }
            if let sophia_config::DesktopShortcutTarget::Session(shortcut) = binding.target
                && !dropped.contains(&shortcut)
            {
                dropped.push(shortcut);
            }
        }
        Ok(dropped)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SessionApplicationOverrides {
    defaults: Vec<SessionApplicationSpec>,
    additions: Vec<SessionApplicationSpec>,
    argument_extensions: Vec<(String, String)>,
    startup: Option<Vec<String>>,
    startup_default: Option<String>,
    terminal: Option<String>,
    launcher: Option<String>,
    browser: Option<String>,
    terminal_default: Option<String>,
    browser_default: Option<String>,
}

impl SessionApplicationOverrides {
    pub(super) fn parse(args: &[String]) -> Result<Self, SessionApplicationConfigError> {
        let defaults = args
            .iter()
            .filter_map(|arg| arg.strip_prefix("--session-start-default="))
            .collect::<Vec<_>>();
        if defaults.len() > 1 {
            return Err(SessionApplicationConfigError::DuplicateApplication(
                "startup default".to_owned(),
            ));
        }
        let startup_default = defaults.first().map(|id| (*id).to_owned());
        if let Some(id) = &startup_default {
            validate_session_app_id(id)?;
        }
        let mut additions = Vec::new();
        let mut addition_ids = BTreeSet::new();
        for value in args
            .iter()
            .filter_map(|argument| argument.strip_prefix("--session-app="))
        {
            let (id, executable) =
                value
                    .split_once('=')
                    .ok_or(SessionApplicationConfigError::InvalidCli(
                        "--session-app expects ID=/absolute/executable",
                    ))?;
            validate_session_app_id(id)?;
            let executable = std::path::PathBuf::from(executable);
            if !executable.is_absolute() || executable.as_os_str().is_empty() {
                return Err(SessionApplicationConfigError::InvalidCli(
                    "--session-app executable must be an absolute path",
                ));
            }
            if !addition_ids.insert(id.to_owned()) {
                return Err(SessionApplicationConfigError::DuplicateApplication(
                    id.to_owned(),
                ));
            }
            additions.push(SessionApplicationSpec {
                id: id.to_owned(),
                executable,
                arguments: Vec::new(),
                placement_classification: None,
            });
        }

        let mut defaults = Vec::new();
        let mut default_ids = BTreeSet::new();
        for value in args
            .iter()
            .filter_map(|arg| arg.strip_prefix("--session-app-default="))
        {
            let (id, executable) =
                value
                    .split_once('=')
                    .ok_or(SessionApplicationConfigError::InvalidCli(
                        "--session-app-default expects ID=EXECUTABLE",
                    ))?;
            validate_session_app_id(id)?;
            if executable.is_empty() || executable.len() > 4096 || executable.contains('\0') {
                return Err(SessionApplicationConfigError::InvalidCli(
                    "invalid default executable",
                ));
            }
            if defaults.len() >= 32 || !default_ids.insert(id.to_owned()) {
                return Err(SessionApplicationConfigError::DuplicateApplication(
                    id.to_owned(),
                ));
            }
            defaults.push(SessionApplicationSpec {
                id: id.to_owned(),
                executable: executable.into(),
                arguments: Vec::new(),
                placement_classification: None,
            });
        }
        let mut terminal_default = None;
        let mut browser_default = None;
        for value in args
            .iter()
            .filter_map(|arg| arg.strip_prefix("--session-action-default="))
        {
            let (role, id) =
                value
                    .split_once('=')
                    .ok_or(SessionApplicationConfigError::InvalidCli(
                        "--session-action-default expects terminal|browser=ID",
                    ))?;
            validate_session_app_id(id)?;
            let target = match role {
                "terminal" => &mut terminal_default,
                "browser" => &mut browser_default,
                _ => {
                    return Err(SessionApplicationConfigError::InvalidCli(
                        "invalid default launch role",
                    ));
                }
            };
            if target.replace(id.to_owned()).is_some() {
                return Err(SessionApplicationConfigError::DuplicateAction(
                    role.to_owned(),
                ));
            }
        }

        let mut argument_extensions = Vec::new();
        for value in args
            .iter()
            .filter_map(|argument| argument.strip_prefix("--session-app-arg="))
        {
            let (id, argument) =
                value
                    .split_once('=')
                    .ok_or(SessionApplicationConfigError::InvalidCli(
                        "--session-app-arg expects ID=ARG",
                    ))?;
            if argument.len() > 4_096 {
                return Err(SessionApplicationConfigError::InvalidCli(
                    "--session-app-arg accepts at most 4096 bytes",
                ));
            }
            argument_extensions.push((id.to_owned(), argument.to_owned()));
        }

        let startup_values = args
            .iter()
            .filter_map(|argument| argument.strip_prefix("--session-start="))
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        let startup = if startup_values.is_empty() {
            None
        } else {
            let mut unique = BTreeSet::new();
            for id in &startup_values {
                validate_session_app_id(id)?;
                if !unique.insert(id.clone()) {
                    return Err(SessionApplicationConfigError::DuplicateStartup(id.clone()));
                }
            }
            Some(startup_values)
        };

        let mut terminal = None;
        let mut launcher = None;
        let mut browser = None;
        for value in args
            .iter()
            .filter_map(|argument| argument.strip_prefix("--session-action-app="))
        {
            let (action, id) =
                value
                    .split_once('=')
                    .ok_or(SessionApplicationConfigError::InvalidCli(
                        "--session-action-app expects terminal|launcher|browser=ID",
                    ))?;
            let slot = match action {
                "terminal" => &mut terminal,
                "launcher" => &mut launcher,
                "browser" => &mut browser,
                _ => {
                    return Err(SessionApplicationConfigError::InvalidCli(
                        "--session-action-app expects terminal, launcher, or browser",
                    ));
                }
            };
            if slot.replace(id.to_owned()).is_some() {
                return Err(SessionApplicationConfigError::DuplicateAction(
                    action.to_owned(),
                ));
            }
        }

        Ok(Self {
            defaults,
            additions,
            argument_extensions,
            startup,
            startup_default,
            terminal,
            launcher,
            browser,
            terminal_default,
            browser_default,
        })
    }

    pub(super) fn prepare(
        &self,
        applications: SessionApplicationConfig,
        candidate: &sophia_config::DesktopSessionCandidate,
    ) -> Result<SessionApplicationConfig, SessionApplicationConfigError> {
        self.prepare_with_startup(applications, candidate, None)
    }

    pub(super) fn prepare_live_reload(
        &self,
        applications: SessionApplicationConfig,
        candidate: &sophia_config::DesktopSessionCandidate,
        startup: &[String],
    ) -> Result<SessionApplicationConfig, SessionApplicationConfigError> {
        self.prepare_with_startup(applications, candidate, Some(startup))
    }

    fn prepare_with_startup(
        &self,
        mut applications: SessionApplicationConfig,
        candidate: &sophia_config::DesktopSessionCandidate,
        retained_startup: Option<&[String]>,
    ) -> Result<SessionApplicationConfig, SessionApplicationConfigError> {
        let core = applications.applications.clone();
        for declared in &candidate.applications {
            if !applications.command_names.insert(declared.name.clone()) {
                return Err(SessionApplicationConfigError::DuplicateApplication(
                    declared.name.clone(),
                ));
            }
            let command = match &declared.command {
                sophia_config::DesktopApplicationCommand::Exec {
                    executable,
                    arguments,
                } => SessionApplicationSpec {
                    id: declared.name.clone(),
                    executable: executable.clone(),
                    arguments: arguments.clone(),
                    placement_classification: None,
                },
                sophia_config::DesktopApplicationCommand::UseCore(name) => {
                    let mut command = core.get(name).cloned().ok_or_else(|| {
                        SessionApplicationConfigError::UnknownApplication(name.clone())
                    })?;
                    command.id.clone_from(&declared.name);
                    command
                }
            };
            insert_application(&mut applications, command)?;
        }
        for default in &self.defaults {
            if !applications.applications.contains_key(&default.id) {
                insert_application(&mut applications, default.clone())?;
            }
        }
        for addition in &self.additions {
            insert_application(&mut applications, addition.clone())?;
        }
        for (id, argument) in &self.argument_extensions {
            let application = applications
                .applications
                .get_mut(id)
                .ok_or_else(|| SessionApplicationConfigError::UnknownApplication(id.clone()))?;
            if application.arguments.len() >= 32 {
                return Err(SessionApplicationConfigError::ArgumentLimit(id.clone()));
            }
            application.arguments.push(argument.clone());
        }

        applications.apply_desktop_candidate(
            candidate,
            self.terminal.is_some(),
            self.browser.is_some(),
            self.startup.is_some() || retained_startup.is_some(),
        )?;
        if retained_startup.is_none()
            && let Some(startup) = &self.startup
        {
            for id in startup {
                require_application(&applications, id)?;
            }
            applications.startup.clone_from(startup);
        }
        if retained_startup.is_none()
            && self.startup.is_none()
            && candidate.startup.is_none()
            && applications.startup.is_empty()
            && let Some(id) = &self.startup_default
        {
            require_application(&applications, id)?;
            applications.startup.push(id.clone());
        }
        // Startup already ran; its identifiers describe existing child bookkeeping.
        if let Some(startup) = retained_startup {
            applications.startup = startup.to_vec();
        }
        for id in [&self.terminal, &self.launcher, &self.browser]
            .into_iter()
            .flatten()
        {
            require_application(&applications, id)?;
        }
        if let Some(terminal) = &self.terminal {
            applications.terminal = Some(terminal.clone());
        }
        if let Some(launcher) = &self.launcher {
            applications.launcher = Some(launcher.clone());
        }
        if let Some(browser) = &self.browser {
            applications.browser = Some(browser.clone());
        }
        for (role, fallback, profile_role, explicit) in [
            (
                &mut applications.terminal,
                &self.terminal_default,
                &candidate.terminal,
                &self.terminal,
            ),
            (
                &mut applications.browser,
                &self.browser_default,
                &candidate.browser,
                &self.browser,
            ),
        ] {
            if role.is_none()
                && profile_role.is_none()
                && explicit.is_none()
                && let Some(fallback) = fallback
                && applications.applications.contains_key(fallback)
            {
                *role = Some(fallback.clone());
            }
        }
        Ok(applications)
    }
}

fn insert_application(
    applications: &mut SessionApplicationConfig,
    command: SessionApplicationSpec,
) -> Result<(), SessionApplicationConfigError> {
    if !applications.applications.contains_key(&command.id) && applications.applications.len() >= 32
    {
        return Err(SessionApplicationConfigError::ApplicationLimit);
    }
    applications
        .applications
        .insert(command.id.clone(), command);
    Ok(())
}

fn validate_session_app_id(id: &str) -> Result<(), SessionApplicationConfigError> {
    if id.is_empty()
        || id.len() > 32
        || !id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
    {
        return Err(SessionApplicationConfigError::InvalidCli(
            "session application IDs accept 1-32 lowercase ASCII letters, digits, '-' or '_'",
        ));
    }
    Ok(())
}

fn require_application(
    applications: &SessionApplicationConfig,
    id: &str,
) -> Result<(), SessionApplicationConfigError> {
    if applications.applications.contains_key(id) {
        Ok(())
    } else {
        Err(SessionApplicationConfigError::UnknownApplication(
            id.to_owned(),
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SessionApplicationConfigError {
    InvalidCli(&'static str),
    ApplicationLimit,
    ArgumentLimit(String),
    DuplicateApplication(String),
    DuplicateStartup(String),
    DuplicateAction(String),
    UnknownApplication(String),
    AmbiguousProfileIdentity(String),
    UnavailableShortcutCapability,
}

impl fmt::Display for SessionApplicationConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCli(message) => formatter.write_str(message),
            Self::ApplicationLimit => {
                formatter.write_str("--session-app accepts at most 32 applications")
            }
            Self::ArgumentLimit(id) => {
                write!(formatter, "session app {id:?} accepts at most 32 arguments")
            }
            Self::DuplicateApplication(id) => {
                write!(formatter, "duplicate --session-app ID {id:?}")
            }
            Self::DuplicateStartup(id) => write!(formatter, "duplicate --session-start ID {id:?}"),
            Self::DuplicateAction(action) => {
                write!(formatter, "duplicate session action mapping {action:?}")
            }
            Self::UnknownApplication(id) => {
                write!(
                    formatter,
                    "session configuration references unknown app {id:?}"
                )
            }
            Self::AmbiguousProfileIdentity(name) => write!(
                formatter,
                "desktop session application identity {name:?} is ambiguous"
            ),
            Self::UnavailableShortcutCapability => {
                formatter.write_str("desktop shortcut references an unavailable session capability")
            }
        }
    }
}

impl std::error::Error for SessionApplicationConfigError {}
