//! Passive records for the explicitly authorized developer-launch 9P export.
//! Recipe selection is a request; only Session policy grants launch authority.
use serde::{Deserialize, Serialize};

pub const DEVELOPER_LAUNCH_VERSION: u32 = 1;
pub const DEVELOPER_LAUNCH_MAX_REQUEST: usize = 32 * 1024;
pub const DEVELOPER_LAUNCH_MAX_ARGUMENTS: usize = 128;
pub const DEVELOPER_LAUNCH_MAX_STRING: usize = 4096;
pub const DEVELOPER_LAUNCH_MAX_RESULT: usize = 256;
pub const DEVELOPER_LAUNCH_SUBMIT_BYTES: usize = 4;
pub const DEVELOPER_LAUNCH_API: &str = "sophia_developer_launch_v1\ntransport=9P2000.L\nadmission=explicit_host_admin\nrequest_bytes=32768\nrequests_per_connection=1\nsubmit=le_u32_request_id\nresult=process_readiness_not_window_or_authentication\n";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeveloperLaunchRequest {
    pub version: u32,
    pub request: u32,
    pub recipe: String,
    pub executable: String,
    pub arguments: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeveloperLaunchStatus {
    Started,
    Denied,
    Unavailable,
    Failed,
    /// The response deadline passed after custody moved to Session. Retrying
    /// on a new connection could start another process; no automatic retry.
    Indeterminate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeveloperLaunchResult {
    pub version: u32,
    pub request: u32,
    pub status: DeveloperLaunchStatus,
    /// Diagnostic monitor PID, never principal, custody or application identity.
    #[serde(deserialize_with = "required_process")]
    pub process: Option<u32>,
}

fn required_process<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<Option<u32>, D::Error> {
    Option::<u32>::deserialize(decoder)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeveloperLaunchRecordError;
impl std::fmt::Display for DeveloperLaunchRecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid developer launch record")
    }
}
impl std::error::Error for DeveloperLaunchRecordError {}

impl DeveloperLaunchRequest {
    pub fn validate(&self) -> Result<(), DeveloperLaunchRecordError> {
        let recipe = self.recipe.as_bytes();
        if self.version != DEVELOPER_LAUNCH_VERSION
            || self.request == 0
            || recipe.is_empty()
            || recipe.len() > 64
            || !recipe
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
            || !self.executable.starts_with('/')
            || !valid_string(&self.executable)
            || self.arguments.len() > DEVELOPER_LAUNCH_MAX_ARGUMENTS
            || self.arguments.iter().any(|arg| !valid_string(arg))
        {
            return Err(DeveloperLaunchRecordError);
        }
        Ok(())
    }
}

impl DeveloperLaunchResult {
    pub fn validate(&self) -> Result<(), DeveloperLaunchRecordError> {
        let process_valid = match self.status {
            DeveloperLaunchStatus::Started => self.process.is_some_and(|pid| pid != 0),
            _ => self.process.is_none(),
        };
        if self.version != DEVELOPER_LAUNCH_VERSION || self.request == 0 || !process_valid {
            return Err(DeveloperLaunchRecordError);
        }
        Ok(())
    }
}

fn valid_string(text: &str) -> bool {
    text.len() <= DEVELOPER_LAUNCH_MAX_STRING && !text.as_bytes().contains(&0)
}

pub fn encode_developer_launch_request(
    value: &DeveloperLaunchRequest,
) -> Result<Vec<u8>, DeveloperLaunchRecordError> {
    value.validate()?;
    encode(value, DEVELOPER_LAUNCH_MAX_REQUEST)
}
pub fn decode_developer_launch_request(
    bytes: &[u8],
) -> Result<DeveloperLaunchRequest, DeveloperLaunchRecordError> {
    let value: DeveloperLaunchRequest = decode(bytes, DEVELOPER_LAUNCH_MAX_REQUEST)?;
    value.validate()?;
    Ok(value)
}
pub fn encode_developer_launch_result(
    value: &DeveloperLaunchResult,
) -> Result<Vec<u8>, DeveloperLaunchRecordError> {
    value.validate()?;
    encode(value, DEVELOPER_LAUNCH_MAX_RESULT)
}
pub fn decode_developer_launch_result(
    bytes: &[u8],
) -> Result<DeveloperLaunchResult, DeveloperLaunchRecordError> {
    let value: DeveloperLaunchResult = decode(bytes, DEVELOPER_LAUNCH_MAX_RESULT)?;
    value.validate()?;
    Ok(value)
}
fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, DeveloperLaunchRecordError> {
    let bytes = serde_json::to_vec(value).map_err(|_| DeveloperLaunchRecordError)?;
    if bytes.len() > limit {
        return Err(DeveloperLaunchRecordError);
    }
    Ok(bytes)
}
fn decode<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
    limit: usize,
) -> Result<T, DeveloperLaunchRecordError> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err(DeveloperLaunchRecordError);
    }
    serde_json::from_slice(bytes).map_err(|_| DeveloperLaunchRecordError)
}
