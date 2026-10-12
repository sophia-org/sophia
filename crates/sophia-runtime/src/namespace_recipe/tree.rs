use super::Error;
use sophia_protocol::namespace_view::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum Node {
    Directory(Vec<(String, Node)>),
    Endpoint(GrantedEndpoint),
}

pub(super) fn normalize(base: &[String], text: &str) -> Result<Vec<String>, Error> {
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err(Error::Path);
    }
    let mut parts = if text.starts_with('/') {
        Vec::new()
    } else {
        base.to_vec()
    };
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or(Error::Path)?;
            }
            _ if part.len() <= MAX_COMPONENT_BYTES => parts.push(part.into()),
            _ => return Err(Error::Limit),
        }
    }
    if parts.first().is_some_and(|p| p == DISCOVERY_NAME) {
        return Err(Error::Path);
    }
    // Include the leading slash in the normalized path bound.
    if parts.iter().map(|p| p.len() + 1).sum::<usize>().max(1) > MAX_PATH_BYTES {
        return Err(Error::Limit);
    }
    Ok(parts)
}

impl Node {
    pub(super) fn validate(&self, length: usize, count: &mut usize) -> Result<(), Error> {
        *count += 1;
        if length > MAX_PATH_BYTES || *count > super::MAX_TREE_NODES {
            return Err(Error::Limit);
        }
        if let Self::Directory(entries) = self {
            for (name, node) in entries {
                node.validate(length + name.len() + 1, count)?;
            }
        }
        Ok(())
    }
    pub(super) fn lookup(&self, parts: &[String]) -> Result<&Self, Error> {
        let Some((first, rest)) = parts.split_first() else {
            return Ok(self);
        };
        let Self::Directory(entries) = self else {
            return Err(Error::Kind);
        };
        entries
            .iter()
            .find(|(name, _)| name == first)
            .ok_or(Error::Missing)?
            .1
            .lookup(rest)
    }

    pub(super) fn install(
        &mut self,
        parts: &[String],
        value: Self,
        prepend: Option<bool>,
    ) -> Result<(), Error> {
        if parts.is_empty() {
            // Root composition is permitted; root replacement is not.
            return if let Some(prepend) = prepend {
                self.union(value, prepend)
            } else {
                Err(Error::Path)
            };
        }
        let Self::Directory(entries) = self else {
            return Err(Error::Kind);
        };
        let (first, rest) = parts.split_first().expect("nonempty");
        if !rest.is_empty() {
            let index = match entries.iter().position(|(name, _)| name == first) {
                Some(index) => index,
                None => {
                    entries.push((first.clone(), Self::Directory(Vec::new())));
                    entries.len() - 1
                }
            };
            return entries[index].1.install(rest, value, prepend);
        }
        if let Some((_, existing)) = entries.iter_mut().find(|(name, _)| name == first) {
            if let Some(prepend) = prepend {
                existing.union(value, prepend)
            } else if *existing == value {
                Ok(())
            } else {
                Err(Error::Conflict)
            }
        } else if prepend.is_some() {
            Err(Error::Missing)
        } else {
            entries.push((first.clone(), value));
            Ok(())
        }
    }

    fn union(&mut self, value: Self, prepend: bool) -> Result<(), Error> {
        let (Self::Directory(existing), Self::Directory(incoming)) = (self, value) else {
            return Err(Error::Kind);
        };
        let (mut first, second) = if prepend {
            (incoming, existing.clone())
        } else {
            (existing.clone(), incoming)
        };
        for (name, value) in second {
            if let Some((_, prior)) = first.iter_mut().find(|(n, _)| *n == name) {
                if matches!((&*prior, &value), (Self::Directory(_), Self::Directory(_))) {
                    prior.union(value, false)?;
                } else if *prior != value {
                    return Err(Error::Conflict);
                }
            } else {
                first.push((name, value));
            }
        }
        *existing = first;
        Ok(())
    }

    pub(super) fn remove(&mut self, parts: &[String]) -> Result<(), Error> {
        let (first, rest) = parts.split_first().ok_or(Error::Path)?;
        let Self::Directory(entries) = self else {
            return Err(Error::Kind);
        };
        let index = entries
            .iter()
            .position(|(name, _)| name == first)
            .ok_or(Error::Missing)?;
        if rest.is_empty() {
            entries.remove(index);
            Ok(())
        } else {
            entries[index].1.remove(rest)
        }
    }

    pub(super) fn endpoint_count(&self) -> usize {
        match self {
            Self::Endpoint(_) => 1,
            Self::Directory(entries) => entries.iter().map(|(_, node)| node.endpoint_count()).sum(),
        }
    }

    pub(super) fn flatten(&self, prefix: &str, endpoints: &mut Vec<NamespaceEndpoint>) {
        match self {
            Self::Endpoint(endpoint) => endpoints.push(NamespaceEndpoint {
                path: prefix.to_owned(),
                endpoint: endpoint.clone(),
            }),
            Self::Directory(entries) => {
                for (name, node) in entries {
                    let path = if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{prefix}/{name}")
                    };
                    node.flatten(&path, endpoints);
                }
            }
        }
    }
}
