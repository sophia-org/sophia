use super::*;

/// The authentication agent a session lock is ended through. Without one a
/// lock is refused, since nobody could open it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::live_session) struct SessionFactotum {
    pub(in crate::live_session) agent: std::path::PathBuf,
    pub(in crate::live_session) pam_helper: std::path::PathBuf,
    pub(in crate::live_session) pam_service: String,
    /// The login user the agent verifies: its owner.
    pub(in crate::live_session) user: String,
}

/// `--factotum-agent=PATH --factotum-pam-helper=PATH
/// [--factotum-pam-service=NAME]`, both paths absolute. The user is the
/// login's own, from `LOGNAME` or `USER`.
pub(super) fn parse_factotum(args: &[String]) -> Result<Option<SessionFactotum>, Box<dyn std::error::Error>> {
    let agent = arg_value(args, "--factotum-agent");
    let pam_helper = arg_value(args, "--factotum-pam-helper");
    let pam_service = arg_value(args, "--factotum-pam-service");
    let (agent, pam_helper) = match (agent, pam_helper) {
        (None, None) if pam_service.is_none() => return Ok(None),
        (Some(agent), Some(pam_helper)) => (std::path::PathBuf::from(agent), std::path::PathBuf::from(pam_helper)),
        _ => return Err("--factotum-agent and --factotum-pam-helper are required together".into()),
    };
    if !agent.is_absolute() || !pam_helper.is_absolute() {
        return Err("factotum paths must be absolute".into());
    }
    let pam_service = pam_service.unwrap_or_else(|| "sophia-lock".to_owned());
    if pam_service.is_empty()
        || pam_service.len() > 64
        || !pam_service
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err("--factotum-pam-service must be 1-64 letters, digits, '-' or '_'".into());
    }
    let user = std::env::var("LOGNAME")
        .or_else(|_| std::env::var("USER"))
        .ok()
        .filter(|user| !user.is_empty() && user.len() <= 256 && !user.contains('\0'))
        .ok_or("a session lock needs the login user's name in LOGNAME or USER")?;
    Ok(Some(SessionFactotum {
        agent,
        pam_helper,
        pam_service,
        user,
    }))
}
