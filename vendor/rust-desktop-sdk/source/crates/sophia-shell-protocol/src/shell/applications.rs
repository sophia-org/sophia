//! Wire-neutral typed model for the revision-4 shell application catalog.
use crate::InvalidRecord;

pub const SOPHIA_SHELL_MAX_APPLICATIONS: usize = 4096;
pub const SOPHIA_SHELL_MAX_LAUNCHER_ROWS: usize = 32;
/// The longest application label and keyword text a catalog entry carries.
pub const SOPHIA_SHELL_APPLICATION_LABEL_MAX_BYTES: usize = 128;
pub const SOPHIA_SHELL_APPLICATION_KEYWORDS_MAX_BYTES: usize = 256;

/// A catalog identity is a display reference, not permission to execute.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellApplicationDescriptor {
    pub slot: u16,
    pub available: bool,
    pub label: String,
    pub keywords: String,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShellApplicationCatalog {
    pub connection_epoch: u64,
    pub generation: u64,
    pub entries: Vec<ShellApplicationDescriptor>,
}

/// Rejects control characters and bidi override characters from launcher,
/// catalog and reference text. Descriptor labels have their own older rule.
pub fn shell_launcher_text_valid(s: &str, max: usize) -> bool {
    s.len() <= max
        && !s.chars().any(|c| {
            c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
}

pub fn validate_shell_application_catalog(
    s: &ShellApplicationCatalog,
) -> Result<(), InvalidRecord> {
    fn require(ok: bool) -> Result<(), InvalidRecord> {
        if ok {
            Ok(())
        } else {
            Err(InvalidRecord("shell_launcher"))
        }
    }
    require(
        s.connection_epoch > 0
            && s.generation > 0
            && s.entries.len() <= crate::SOPHIA_SHELL_MAX_APPLICATIONS,
    )?;
    let mut slots = std::collections::BTreeSet::new();
    for e in &s.entries {
        require(
            e.slot > 0
                && usize::from(e.slot) <= crate::SOPHIA_SHELL_MAX_APPLICATIONS
                && slots.insert(e.slot)
                && !e.label.is_empty()
                && shell_launcher_text_valid(&e.label, 128)
                && shell_launcher_text_valid(&e.keywords, 256),
        )?;
    }
    Ok(())
}
