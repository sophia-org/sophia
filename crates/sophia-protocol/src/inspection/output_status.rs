//! Bounded observation of the output owner, separate from retained WM geometry.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputAvailability {
    Unavailable,
    Waiting,
    Recovering,
    Available,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputStatus {
    pub availability: OutputAvailability,
    pub topology_generation: u64,
    pub owner_generation: u64,
    /// Completed native retirements for this owner, not a capture grant or a
    /// per-output frame identity. It restarts when owner_generation changes.
    pub owner_retirements: u64,
    pub recovery_stage: &'static str,
    pub retry_at_session_msec: Option<u64>,
    pub failure_stage: &'static str,
    pub failure_code: &'static str,
    pub failure_errno: u32,
}

/// A fixed vocabulary of fields with bounded tokens; never format a backend
/// error, connector path or application metadata into this public view.
pub fn encode_output_status(value: &OutputStatus) -> Option<Vec<u8>> {
    fn token(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 48
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
    }
    if value.topology_generation == 0
        || !token(value.recovery_stage)
        || !token(value.failure_stage)
        || !token(value.failure_code)
        || (value.availability == OutputAvailability::Available && value.owner_generation == 0)
        || (value.owner_generation == 0 && value.owner_retirements != 0)
    {
        return None;
    }
    let availability = match value.availability {
        OutputAvailability::Unavailable => "unavailable",
        OutputAvailability::Waiting => "waiting",
        OutputAvailability::Recovering => "recovering",
        OutputAvailability::Available => "available",
    };
    let retry = value
        .retry_at_session_msec
        .map_or_else(|| "none".into(), |v| v.to_string());
    Some(format!(
        "sophia-output-status version=1 availability={availability} topology_generation={} owner_generation={} owner_retirements={} recovery_stage={} retry_at_session_msec={retry} failure_stage={} failure_code={} failure_errno={} capture_authority=none\n",
        value.topology_generation, value.owner_generation, value.owner_retirements,
        value.recovery_stage, value.failure_stage, value.failure_code, value.failure_errno,
    ).into_bytes())
}
