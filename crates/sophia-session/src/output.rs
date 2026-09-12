//! Host-owned presentation boundary for production session evidence.

use std::fmt::Arguments;
use std::sync::OnceLock;

/// Host callbacks for exact line-oriented session evidence.
#[derive(Clone, Copy)]
pub struct SessionOutput {
    // Written only by the live session's printer, which a default build does
    // not compile.
    #[cfg_attr(not(feature = "native-session"), allow(dead_code))]
    stdout: fn(&str),
    stderr: fn(&str),
}

impl SessionOutput {
    pub const fn new(stdout: fn(&str), stderr: fn(&str)) -> Self {
        Self { stdout, stderr }
    }
}

static OUTPUT: OnceLock<SessionOutput> = OnceLock::new();

/// Installs the process-wide output owner before starting a session.
pub fn install(output: SessionOutput) -> Result<(), &'static str> {
    OUTPUT
        .set(output)
        .map_err(|_| "Sophia session output is already installed")
}

#[cfg_attr(not(feature = "native-session"), allow(dead_code))]
pub(crate) fn stdout(arguments: Arguments<'_>) {
    if let Some(output) = OUTPUT.get() {
        let line = arguments.to_string();
        (output.stdout)(&line);
    }
}

pub(crate) fn stderr(arguments: Arguments<'_>) {
    if let Some(output) = OUTPUT.get() {
        let line = arguments.to_string();
        (output.stderr)(&line);
    }
}
