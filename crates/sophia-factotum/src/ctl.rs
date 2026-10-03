//! The `ctl` file: adding and deleting keys, and listing them; and the
//! `proto` file's list.
//!
//! Ported from 9front `sys/src/cmd/auth/factotum/rpc.c` (`ctlwrite`) and
//! `fs.c` (`readlist`, `keylist`, `protolist`); MIT, Copyright (c) 2021
//! Plan 9 Foundation and 9front authors.

use crate::attr::{Attr, AttrKind, AttrList};
use crate::keyring::{DeleteRefusal, Key, KeyLookup, KeyQuery, Keyring};
use crate::proto::{PROTOCOLS, ProtocolId};

/// A refused `ctl` write, with 9front's error text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CtlError {
    UnknownVerb,
    MultilineWrite,
    KeyWithoutProtos,
    UnknownProto(String),
    ProtoTakesNoKeys(String),
    Proto(String),
    PrivatePattern,
    NoKeysToDelete,
}

impl core::fmt::Display for CtlError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownVerb => formatter.write_str("unknown verb"),
            Self::MultilineWrite => formatter.write_str("multiline write not allowed"),
            Self::KeyWithoutProtos => formatter.write_str("key without protos"),
            Self::UnknownProto(name) => write!(formatter, "unknown proto {name}"),
            Self::ProtoTakesNoKeys(name) => write!(formatter, "proto {name} doesn't take keys"),
            Self::Proto(text) => formatter.write_str(text),
            Self::PrivatePattern => {
                formatter.write_str("only !private? patterns are allowed for private fields")
            }
            Self::NoKeysToDelete => formatter.write_str("found no keys to delete"),
        }
    }
}

/// What a successful write did besides changing keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CtlEffect {
    None,
    /// `debug` toggles tracing of every rpc to the log.
    ToggleDebug,
}

/// `ctlwrite`: one line. An empty write, or one starting with `#`, does
/// nothing; one trailing newline is allowed and more text after it is not.
/// The written bytes may carry secrets; they are borrowed only here.
pub fn write(ring: &mut Keyring, line: &str) -> Result<CtlEffect, CtlError> {
    if line.is_empty() || line.starts_with('#') {
        return Ok(CtlEffect::None);
    }
    let line = match line.split_once('\n') {
        Some((_, rest)) if !rest.is_empty() => return Err(CtlError::MultilineWrite),
        Some((first, _)) => first,
        None => line,
    };
    let (verb, argument) = line.split_once(' ').unwrap_or((line, ""));
    match verb {
        "debug" => Ok(CtlEffect::ToggleDebug),
        "delkey" => {
            ring.delete(&AttrList::parse(argument))
                .map_err(|refusal| match refusal {
                    DeleteRefusal::NothingMatched => CtlError::NoKeysToDelete,
                    DeleteRefusal::PrivateValue => CtlError::PrivatePattern,
                })?;
            Ok(CtlEffect::None)
        }
        "key" => add_keys(ring, argument).map(|()| CtlEffect::None),
        _ => Err(CtlError::UnknownVerb),
    }
}

/// `key`: one key per `proto=` named, each with every other public
/// attribute after its own `proto=` and every `!` attribute private. One
/// protocol refusing does not stop the others, but the write fails with the
/// last refusal.
fn add_keys(ring: &mut Keyring, argument: &str) -> Result<(), CtlError> {
    let mut attrs = AttrList::parse(argument);
    let protos = attrs.take_where(|attr| attr.name == "proto");
    if protos.is_empty() {
        return Err(CtlError::KeyWithoutProtos);
    }
    let private = attrs.take_where(Attr::is_private);
    let mut result = Ok(());
    for named in protos.iter() {
        let Some(proto) = ProtocolId::from_name(&named.value) else {
            result = Err(CtlError::UnknownProto(named.value.clone()));
            continue;
        };
        let mut public = attrs.clone();
        public.insert_front(Attr::new(AttrKind::Nameval, "proto", proto.name()));
        let key = Key {
            attrs: public,
            private: private.clone(),
            proto: proto.name(),
        };
        match proto.add_key(ring, key) {
            None => result = Err(CtlError::ProtoTakesNoKeys(proto.name().to_owned())),
            Some(Err(text)) => result = Err(CtlError::Proto(text)),
            Some(Ok(())) => {}
        }
    }
    result
}

/// A read of a list file that cannot hold its next line: `rpc too small`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ListTooSmall;

/// The per-open cursor 9front keeps for `ctl` and `proto` reads: each read
/// returns one line and ignores the 9P offset.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ListCursor(usize);

impl ListCursor {
    /// `keylist`: the next key's line, listing disabled keys and keys that
    /// want confirmation. An empty result is the end of the list.
    pub fn read_key(&mut self, ring: &Keyring, count: usize) -> Result<Vec<u8>, ListTooSmall> {
        let empty = AttrList::new();
        let lookup = ring.find(
            &KeyQuery {
                attrs: &empty,
                extra: AttrList::new(),
                skip: self.0,
                no_confirm: true,
                use_disabled: true,
            },
            |name| ProtocolId::from_name(name).is_some(),
        );
        let KeyLookup::Found(key) = lookup else {
            return Ok(Vec::new());
        };
        self.advance(key.ctl_line().into_bytes(), count)
    }

    /// `protolist`: the next protocol name and a newline.
    pub fn read_proto(&mut self, count: usize) -> Result<Vec<u8>, ListTooSmall> {
        let Some(proto) = PROTOCOLS.get(self.0) else {
            return Ok(Vec::new());
        };
        let mut line = proto.name().as_bytes().to_vec();
        line.push(b'\n');
        if line.len() > count {
            return Err(ListTooSmall);
        }
        self.0 += 1;
        Ok(line)
    }

    /// A key line fits only in fewer bytes than the read: 9front formats it
    /// with `snprint`, which keeps one byte for the terminator.
    fn advance(&mut self, line: Vec<u8>, count: usize) -> Result<Vec<u8>, ListTooSmall> {
        if line.len() >= count {
            return Err(ListTooSmall);
        }
        self.0 += 1;
        Ok(line)
    }
}
