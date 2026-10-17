use crate::{Owner, RecordError, decimal, invocation::valid_name};

pub const CATALOG_MAX_COMMANDS: usize = 259;
pub const CATALOG_MAX_BYTES: usize = 39432;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Command<'a> {
    pub owner: Owner,
    pub name: &'a str,
}

/// Validated catalog with bounded inline rows and names borrowed from its snapshot.
/// Retain the input unchanged for this value's lifetime. Discovery is not permission.
#[derive(Debug)]
pub struct Catalog<'a> {
    generation: u64,
    count: usize,
    rows: [Command<'a>; CATALOG_MAX_COMMANDS],
}

impl<'a> Catalog<'a> {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn commands(&self) -> &[Command<'a>] {
        &self.rows[..self.count]
    }
    pub fn contains(&self, command: Command<'_>) -> bool {
        self.commands().binary_search(&command).is_ok()
    }
}

/// Validate the entire immutable snapshot before returning any catalog.
/// No heap allocation. EOF on the backing file remains the client's responsibility.
pub fn decode_catalog(bytes: &[u8]) -> Result<Catalog<'_>, RecordError> {
    if bytes.len() > CATALOG_MAX_BYTES {
        return Err(RecordError);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| RecordError)?;
    let mut lines = text.split_inclusive('\n');
    let mut header = lines
        .next()
        .ok_or(RecordError)?
        .strip_suffix('\n')
        .ok_or(RecordError)?
        .split(' ');
    if header.next() != Some("v1") || header.next() != Some("catalog") {
        return Err(RecordError);
    }
    let generation = decimal(header.next().ok_or(RecordError)?)?;
    let count = decimal(header.next().ok_or(RecordError)?)?;
    if generation == 0 || count > CATALOG_MAX_COMMANDS as u64 || header.next().is_some() {
        return Err(RecordError);
    }
    let mut catalog = Catalog {
        generation,
        count: count as usize,
        rows: [Command {
            owner: Owner::Policy,
            name: "",
        }; CATALOG_MAX_COMMANDS],
    };
    let mut policy_count = 0;
    for index in 0..catalog.count {
        let mut fields = lines
            .next()
            .ok_or(RecordError)?
            .strip_suffix('\n')
            .ok_or(RecordError)?
            .splitn(4, ' ');
        if fields.next() != Some("v1") || fields.next() != Some("command") {
            return Err(RecordError);
        }
        let owner = match fields.next() {
            Some("policy") => Owner::Policy,
            Some("session") => Owner::Session,
            _ => return Err(RecordError),
        };
        let (length, name) = fields
            .next()
            .ok_or(RecordError)?
            .split_once(':')
            .ok_or(RecordError)?;
        if decimal(length)? != name.len() as u64 || !valid_name(name) {
            return Err(RecordError);
        }
        match owner {
            Owner::Policy => {
                policy_count += 1;
                if policy_count > 256 {
                    return Err(RecordError);
                }
            }
            Owner::Session if !matches!(name, "logout" | "reload-profile" | "restart-wm") => {
                return Err(RecordError);
            }
            Owner::Session => (),
        }
        let command = Command { owner, name };
        if index > 0 && catalog.rows[index - 1] >= command {
            return Err(RecordError);
        }
        catalog.rows[index] = command;
    }
    if lines.next().is_some() {
        return Err(RecordError);
    }
    Ok(catalog)
}
