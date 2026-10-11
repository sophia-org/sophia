//! Which registered applications launch confined, and the command that does it.
//!
//! KEYED ON THE REGISTERED APPLICATION ID. Confinement attaches to the Session
//! application registry ID the operator mapped, never to an executable: a
//! scanned desktop entry is unmapped even when it runs the same file. A
//! mapped ID is confined on every Session launch path, and a mapped launch
//! that cannot be confined fails; it never falls back to the host.
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The catalog spells a registered application `registered:<id>`.
const REGISTERED_PREFIX: &str = "registered:";

/// Resolves a confined group's socket directory for one display.
pub(crate) type GroupDirectory = fn(u32, u32) -> std::io::Result<PathBuf>;

/// Session confinement for application launches. The default maps nothing,
/// so every launch is the host launch it always was.
#[derive(Clone, Copy, Default)]
pub struct ApplicationConfinement<'a> {
    target: Option<Target<'a>>,
}

#[derive(Clone, Copy)]
struct Target<'a> {
    applications: &'a BTreeMap<String, u32>,
    bubblewrap: &'a Path,
    cookie_file: &'a Path,
    display_number: Option<u32>,
    group_directory: GroupDirectory,
}

impl<'a> ApplicationConfinement<'a> {
    #[cfg(feature = "native-session")]
    pub(crate) fn new(
        applications: &'a BTreeMap<String, u32>,
        bubblewrap: &'a Path,
        cookie_file: &'a Path,
        display_number: Option<u32>,
        group_directory: GroupDirectory,
    ) -> Self {
        Self {
            target: (!applications.is_empty()).then_some(Target {
                applications,
                bubblewrap,
                cookie_file,
                display_number,
                group_directory,
            }),
        }
    }

    /// The registered application ID a catalog identity names, if any.
    pub(crate) fn registered_id(identity: &str) -> Option<&str> {
        identity.strip_prefix(REGISTERED_PREFIX)
    }

    /// `None` for an unmapped application: launch it exactly as before.
    /// `Some(Err)` for a mapped one that cannot be confined: the caller fails
    /// the launch and must not spawn it any other way.
    ///
    /// The command is the runtime's confined Bubblewrap command with this
    /// Session's process group and stdio. No host endpoint, environment or
    /// working directory is added: the sandbox has its own.
    pub(crate) fn command(
        &self,
        application: &str,
        executable: &Path,
        arguments: &[String],
        working_directory: Option<&Path>,
    ) -> Option<std::io::Result<Command>> {
        let target = self.target?;
        let group = *target.applications.get(application)?;
        Some((|| {
            if working_directory.is_some() {
                return Err(std::io::Error::other(
                    "a confined application has no host working directory",
                ));
            }
            let display_number = target
                .display_number
                .ok_or_else(|| std::io::Error::other("confined launch has no display number"))?;
            let launch = sophia_runtime::ConfinedXApplicationLaunch {
                executable: executable.to_path_buf(),
                args: arguments.iter().map(OsString::from).collect(),
                bubblewrap: target.bubblewrap.to_path_buf(),
                group_directory: (target.group_directory)(display_number, group)?,
                cookie_file: target.cookie_file.to_path_buf(),
                display_number,
            };
            let mut command = launch.command().map_err(std::io::Error::other)?;
            command
                .process_group(0)
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            Ok(command)
        })())
    }
}
