//! Strict discovery v1: LF lines and tab fields, no escaping or ambient paths.
//! The same parser validates encoded output; there is no permissive writer.
use super::*;
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryError {
    Limit,
    Syntax,
    Version,
    Identity,
    Path,
    Duplicate,
}
impl std::fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "namespace discovery refused: {self:?}")
    }
}
impl std::error::Error for DiscoveryError {}

pub fn encode_discovery(view: &NamespaceDiscovery) -> Result<Vec<u8>, DiscoveryError> {
    if view.endpoints.len() > MAX_ENDPOINTS {
        return Err(DiscoveryError::Limit);
    }
    let mut text = format!(
        "sophia-namespace\t1\t{}\t{}\n",
        view.revision,
        view.endpoints.len()
    );
    for endpoint in &view.endpoints {
        let s = &endpoint.service;
        let protocol = match s.protocol {
            EndpointProtocol::NineP2000L => "9p2000.L",
            EndpointProtocol::X11 => "x11",
        };
        let state = match endpoint.availability {
            ServiceAvailability::Available => "available",
            ServiceAvailability::Waiting => "waiting",
            ServiceAvailability::Recovering => "recovering",
        };
        // Check lengths before formatting untrusted strings into the buffer.
        if endpoint.path.len() >= MAX_PATH_BYTES || s.interface.len() > MAX_COMPONENT_BYTES {
            return Err(DiscoveryError::Limit);
        }
        text.push_str(&format!(
            "endpoint\t{}\t{protocol}\t{}\t{}\t{}\t{}\t{state}\n",
            endpoint.path, s.interface, s.version, s.identity, s.generation
        ));
    }
    if decode_discovery(text.as_bytes())? != *view {
        return Err(DiscoveryError::Syntax);
    }
    Ok(text.into_bytes())
}

pub fn decode_discovery(bytes: &[u8]) -> Result<NamespaceDiscovery, DiscoveryError> {
    if bytes.len() > MAX_DISCOVERY_BYTES {
        return Err(DiscoveryError::Limit);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| DiscoveryError::Syntax)?;
    if !text.ends_with('\n')
        || text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return Err(DiscoveryError::Syntax);
    }
    let mut lines = text[..text.len() - 1].split('\n');
    let header: Vec<_> = lines
        .next()
        .ok_or(DiscoveryError::Syntax)?
        .split('\t')
        .collect();
    let ["sophia-namespace", version, revision, count] = header.as_slice() else {
        return Err(DiscoveryError::Syntax);
    };
    if *version != "1" {
        return Err(DiscoveryError::Version);
    }
    let revision = nonzero(revision)?;
    let count = if *count == "0" { 0 } else { nonzero(count)? };
    if count > MAX_ENDPOINTS as u64 {
        return Err(DiscoveryError::Limit);
    }
    let mut endpoints = Vec::new();
    let mut names = BTreeSet::new();
    for line in lines {
        if endpoints.len() == MAX_ENDPOINTS {
            return Err(DiscoveryError::Limit);
        }
        let fields: Vec<_> = line.split('\t').collect();
        let [
            "endpoint",
            path,
            protocol,
            interface,
            version,
            identity,
            generation,
            state,
        ] = fields.as_slice()
        else {
            return Err(DiscoveryError::Syntax);
        };
        validate_path(path)?;
        if !names.insert(*path) {
            return Err(DiscoveryError::Duplicate);
        }
        if interface.is_empty()
            || interface.len() > MAX_COMPONENT_BYTES
            || !interface
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(DiscoveryError::Syntax);
        }
        let protocol = match *protocol {
            "9p2000.L" => EndpointProtocol::NineP2000L,
            "x11" => EndpointProtocol::X11,
            _ => return Err(DiscoveryError::Version),
        };
        let version = nonzero(version)?
            .try_into()
            .map_err(|_| DiscoveryError::Identity)?;
        let availability = match *state {
            "available" => ServiceAvailability::Available,
            "waiting" => ServiceAvailability::Waiting,
            "recovering" => ServiceAvailability::Recovering,
            _ => return Err(DiscoveryError::Syntax),
        };
        endpoints.push(DiscoveredEndpoint {
            path: (*path).into(),
            service: ServiceReference {
                identity: nonzero(identity)?,
                generation: nonzero(generation)?,
                protocol,
                interface: (*interface).into(),
                version,
            },
            availability,
        });
    }
    if endpoints.len() as u64 != count {
        return Err(DiscoveryError::Syntax);
    }
    // A leaf cannot also be a directory, regardless of record order.
    for endpoint in &endpoints {
        let mut prefix = endpoint.path.as_str();
        while let Some((parent, _)) = prefix.rsplit_once('/') {
            if names.contains(parent) {
                return Err(DiscoveryError::Path);
            }
            prefix = parent;
        }
    }
    Ok(NamespaceDiscovery {
        revision,
        endpoints,
    })
}

fn nonzero(text: &str) -> Result<u64, DiscoveryError> {
    if text.is_empty() || text.starts_with('0') || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(DiscoveryError::Identity);
    }
    text.parse().map_err(|_| DiscoveryError::Identity)
}

fn validate_path(path: &str) -> Result<(), DiscoveryError> {
    if path.is_empty() || path.len() >= MAX_PATH_BYTES || path.chars().any(char::is_control) {
        return Err(DiscoveryError::Path);
    }
    for (index, part) in path.split('/').enumerate() {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.len() > MAX_COMPONENT_BYTES
            || (index == 0 && part == DISCOVERY_NAME)
        {
            return Err(DiscoveryError::Path);
        }
    }
    Ok(())
}
