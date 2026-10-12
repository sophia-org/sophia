//! Endpoint visibility is not authentication or permission. Records here carry
//! no grants; a selected stream still needs its protocol's admission exchange.

#[cfg(target_os = "linux")]
mod directory;
#[cfg(target_os = "linux")]
pub use directory::Namespace;

pub const DIRECTORY_ENV: &str = "SOPHIA_NAMESPACE_DIR";
pub const DISCOVERY_FILE: &str = "discovery";
pub const MAX_BYTES: usize = 131072;
pub const MAX_ENDPOINTS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protocol {
    NineP2000L,
    X11,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Availability {
    Available,
    Waiting,
    Recovering,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Endpoint {
    pub path: String,
    pub protocol: Protocol,
    pub interface: String,
    pub version: u32,
    pub service: u64,
    pub generation: u64,
    pub availability: Availability,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Discovery {
    pub revision: u64,
    pub endpoints: Vec<Endpoint>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Invalid(pub &'static str);
impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid namespace discovery: {}", self.0)
    }
}
impl std::error::Error for Invalid {}

impl Discovery {
    pub fn parse(bytes: &[u8]) -> Result<Self, Invalid> {
        if bytes.len() > MAX_BYTES {
            return Err(Invalid("byte limit"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| Invalid("UTF-8"))?;
        let mut lines = text
            .strip_suffix('\n')
            .ok_or(Invalid("truncated file"))?
            .split('\n');
        let h = fields::<4>(lines.next().ok_or(Invalid("header"))?)?;
        if h[0] != "sophia-namespace" || h[1] != "1" {
            return Err(Invalid("format version"));
        }
        let revision = number(h[2], false)?;
        let count = number(h[3], true)?;
        if count > MAX_ENDPOINTS as u64 {
            return Err(Invalid("endpoint limit"));
        }
        let mut endpoints: Vec<Endpoint> = Vec::with_capacity(count as usize);
        for line in lines {
            if endpoints.len() >= count as usize {
                return Err(Invalid("endpoint count"));
            }
            let f = fields::<8>(line)?;
            if f[0] != "endpoint" {
                return Err(Invalid("record kind"));
            }
            path(f[1])?;
            for old in &endpoints {
                if old.path == f[1]
                    || old
                        .path
                        .strip_prefix(f[1])
                        .is_some_and(|s| s.starts_with('/'))
                    || f[1]
                        .strip_prefix(&old.path)
                        .is_some_and(|s| s.starts_with('/'))
                {
                    return Err(Invalid("conflicting paths"));
                }
            }
            let protocol = match f[2] {
                "9p2000.L" => Protocol::NineP2000L,
                "x11" => Protocol::X11,
                _ => return Err(Invalid("protocol")),
            };
            if f[3].is_empty()
                || f[3].len() > 255
                || !f[3]
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err(Invalid("interface"));
            }
            let version =
                u32::try_from(number(f[4], false)?).map_err(|_| Invalid("interface version"))?;
            let service = number(f[5], false)?;
            let generation = number(f[6], false)?;
            let availability = match f[7] {
                "available" => Availability::Available,
                "waiting" => Availability::Waiting,
                "recovering" => Availability::Recovering,
                _ => return Err(Invalid("availability")),
            };
            endpoints.push(Endpoint {
                path: f[1].into(),
                protocol,
                interface: f[3].into(),
                version,
                service,
                generation,
                availability,
            });
        }
        if endpoints.len() != count as usize {
            return Err(Invalid("endpoint count"));
        }
        Ok(Self {
            revision,
            endpoints,
        })
    }
    pub fn endpoint(&self, path: &str) -> Option<&Endpoint> {
        self.endpoints.iter().find(|e| e.path == path)
    }
}
fn fields<const N: usize>(line: &str) -> Result<[&str; N], Invalid> {
    let mut fields = line.split('\t');
    let mut out = [""; N];
    for item in &mut out {
        *item = fields.next().ok_or(Invalid("field count"))?;
    }
    if fields.next().is_some() {
        return Err(Invalid("field count"));
    }
    Ok(out)
}
fn number(text: &str, zero: bool) -> Result<u64, Invalid> {
    if text.is_empty()
        || !text.bytes().all(|b| b.is_ascii_digit())
        || (text.starts_with('0') && (text.len() != 1 || !zero))
    {
        return Err(Invalid("decimal"));
    }
    text.parse().map_err(|_| Invalid("decimal overflow"))
}
fn path(text: &str) -> Result<(), Invalid> {
    if text.len() > 1023 || text.chars().any(char::is_control) {
        return Err(Invalid("path"));
    }
    for (i, part) in text.split('/').enumerate() {
        if part.is_empty()
            || part.len() > 255
            || matches!(part, "." | "..")
            || (i == 0 && part == DISCOVERY_FILE)
        {
            return Err(Invalid("path"));
        }
    }
    Ok(())
}
