//! Generic reader for Sophia's evidence and session record lines.
//!
//! Sophia emits records as one line each: a marker such as
//! `sophia_live_session schema=18 ` followed by whitespace-separated
//! `key=value` fields. Values are never quoted, so a value containing
//! whitespace cannot be represented; an emitter that needs one must choose a
//! different encoding. This module is the public seam external verifiers use
//! to read those lines without re-deriving the grammar in shell.
//!
//! Records reach a session log two ways: printed bare by the session itself,
//! or through `tracing`, decorated with a timestamp, level and ANSI colour.
//! A reader anchored to the line start sees only the first kind, so every
//! reader here matches by marker instead.

use std::collections::BTreeMap;
use std::fmt;

/// The remainder of a record line after its marker, wherever the marker sits.
///
/// A match anywhere in the line is intended: it is what lets one reader accept
/// both bare and `tracing`-decorated records. The price is that the caller
/// must choose an exact marker -- include the record name, schema and the
/// trailing space -- so that one record is never mistaken for another whose
/// name merely contains it, or for a record quoted inside another line.
pub fn after_marker<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    line.find(marker).map(|start| &line[start + marker.len()..])
}

/// The remainder after the marker on the last line of `text` that carries it.
pub fn last_after_marker<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    text.lines()
        .rev()
        .find_map(|line| after_marker(line, marker))
}

/// Every remainder after the marker, in line order.
pub fn each_after_marker<'a>(text: &'a str, marker: &'a str) -> impl Iterator<Item = &'a str> + 'a {
    text.lines()
        .filter_map(move |line| after_marker(line, marker))
}

/// Why a record's fields could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RecordError {
    /// A token is not `key=value` with a nonempty key.
    MalformedField { token: String },
    /// A field name appears more than once.
    DuplicateField { name: String },
    /// A required field is absent.
    MissingField { name: String },
    /// A field is not a canonical unsigned decimal that fits in `u64`.
    InvalidUnsigned { name: String, value: String },
}

impl fmt::Display for RecordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedField { token } => write!(formatter, "malformed record field {token:?}"),
            Self::DuplicateField { name } => write!(formatter, "record repeats field {name}"),
            Self::MissingField { name } => write!(formatter, "record is missing field {name}"),
            Self::InvalidUnsigned { name, value } => write!(
                formatter,
                "record field {name} is not a canonical unsigned integer: {value:?}"
            ),
        }
    }
}

impl std::error::Error for RecordError {}

/// The fields of one record, keyed by name. Passive: it borrows the line.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecordFields<'a> {
    fields: BTreeMap<&'a str, &'a str>,
}

impl<'a> RecordFields<'a> {
    /// Strict: every whitespace-separated token must be `key=value` with a
    /// nonempty key, and no key may repeat. Use it on the remainder after a
    /// marker that already includes the record name.
    pub fn parse(record: &'a str) -> Result<Self, RecordError> {
        Self::parse_tokens(record.split_whitespace())
    }

    /// As [`Self::parse`], but the first token may be one bare record name,
    /// which is skipped. Only that one position is lenient: a later bare
    /// token, or a second leading one, is a [`RecordError::MalformedField`].
    pub fn parse_named(record: &'a str) -> Result<Self, RecordError> {
        let mut tokens = record.split_whitespace().peekable();
        if tokens.peek().is_some_and(|token| !token.contains('=')) {
            tokens.next();
        }
        Self::parse_tokens(tokens)
    }

    fn parse_tokens(tokens: impl Iterator<Item = &'a str>) -> Result<Self, RecordError> {
        let mut fields = BTreeMap::new();
        for token in tokens {
            let Some((name, value)) = token.split_once('=').filter(|(name, _)| !name.is_empty())
            else {
                return Err(RecordError::MalformedField {
                    token: token.to_owned(),
                });
            };
            if fields.insert(name, value).is_some() {
                return Err(RecordError::DuplicateField {
                    name: name.to_owned(),
                });
            }
        }
        Ok(Self { fields })
    }

    pub fn get(&self, name: &str) -> Option<&'a str> {
        self.fields.get(name).copied()
    }

    pub fn require(&self, name: &str) -> Result<&'a str, RecordError> {
        self.get(name).ok_or_else(|| RecordError::MissingField {
            name: name.to_owned(),
        })
    }

    /// A required field as canonical unsigned decimal: digits only, no sign,
    /// no leading zero except `0` itself, and within `u64`.
    pub fn unsigned(&self, name: &str) -> Result<u64, RecordError> {
        let value = self.require(name)?;
        let invalid = || RecordError::InvalidUnsigned {
            name: name.to_owned(),
            value: value.to_owned(),
        };
        if value.is_empty()
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err(invalid());
        }
        value.parse().map_err(|_| invalid())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'a str, &'a str)> + '_ {
        self.fields.iter().map(|(name, value)| (*name, *value))
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}
