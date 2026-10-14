//! Passive administrative command data shared by socket and file transports.
use crate::BinaryCodecError;

pub const CONTROL_MAX_COMMANDS: usize = 259;
pub const CONTROL_SESSION_OPERATIONS: &[&str] = &["logout", "reload-profile", "restart-wm"];

pub fn control_session_operation(name: &str) -> bool {
    CONTROL_SESSION_OPERATIONS.contains(&name)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u16)]
pub enum ControlOwner {
    Policy = 1,
    Session = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum ControlOutcome {
    Committed = 1,
    Completed = 2,
    Unchanged = 3,
    Rejected = 4,
    Stale = 5,
    Denied = 6,
    Unavailable = 7,
    Overloaded = 8,
    TimedOut = 9,
    Indeterminate = 10,
}

impl ControlOutcome {
    pub fn from_wire(value: u16) -> Result<Self, BinaryCodecError> {
        outcome(value)
    }
    pub const fn name(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::Completed => "completed",
            Self::Unchanged => "unchanged",
            Self::Rejected => "rejected",
            Self::Stale => "stale",
            Self::Denied => "denied",
            Self::Unavailable => "unavailable",
            Self::Overloaded => "overloaded",
            Self::TimedOut => "timed-out",
            Self::Indeterminate => "indeterminate",
        }
    }
    pub const fn success(self) -> bool {
        matches!(self, Self::Committed | Self::Completed | Self::Unchanged)
    }
}

impl ControlOwner {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Policy => "policy",
            Self::Session => "session",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ControlCommand {
    pub owner: ControlOwner,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ControlCatalog {
    pub generation: u64,
    pub commands: Vec<ControlCommand>,
}

fn invalid() -> BinaryCodecError {
    BinaryCodecError::InvalidRecord("control v1")
}
fn require(ok: bool) -> Result<(), BinaryCodecError> {
    if ok { Ok(()) } else { Err(invalid()) }
}
fn outcome(raw: u16) -> Result<ControlOutcome, BinaryCodecError> {
    Ok(match raw {
        1 => ControlOutcome::Committed,
        2 => ControlOutcome::Completed,
        3 => ControlOutcome::Unchanged,
        4 => ControlOutcome::Rejected,
        5 => ControlOutcome::Stale,
        6 => ControlOutcome::Denied,
        7 => ControlOutcome::Unavailable,
        8 => ControlOutcome::Overloaded,
        9 => ControlOutcome::TimedOut,
        10 => ControlOutcome::Indeterminate,
        _ => return Err(invalid()),
    })
}

pub fn validate_control_name(name: &str) -> Result<(), BinaryCodecError> {
    require(
        !name.is_empty()
            && name.len() <= 128
            && name.trim() == name
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'-' | b'_' | b'.')),
    )
}

/// Refuse invalid owner catalogs before making them discoverable.
pub fn validate_control_catalog(catalog: &ControlCatalog) -> Result<(), BinaryCodecError> {
    require(catalog.generation != 0 && catalog.commands.len() <= CONTROL_MAX_COMMANDS)?;
    require(catalog.commands.windows(2).all(|pair| pair[0] < pair[1]))?;
    require(
        catalog
            .commands
            .iter()
            .filter(|c| c.owner == ControlOwner::Policy)
            .count()
            <= 256,
    )?;
    for command in &catalog.commands {
        validate_control_name(&command.name)?;
        require(
            command.owner != ControlOwner::Session || control_session_operation(&command.name),
        )?;
    }
    Ok(())
}
