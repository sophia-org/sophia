//! The key ring: what `ctl` adds and removes and what protocols look up.
//!
//! Ported from 9front `sys/src/cmd/auth/factotum/util.c` (`findkey`,
//! `replacekey`, `canusekey`) and `rpc.c` (`ctlwrite`'s delkey); MIT,
//! Copyright (c) 2021 Plan 9 Foundation and 9front authors.
//!
//! Sophia's agent serves only its owner (admission checks the peer's UID),
//! so 9front's `owner=` and speak-for checks reduce to the owner's case and
//! are not carried.

use crate::attr::{AttrKind, AttrList};
use std::sync::Arc;

/// One key. The public attributes begin with `proto=`; private (`!`)
/// attributes hold the secrets and zero themselves when dropped.
#[derive(Debug)]
pub struct Key {
    pub attrs: AttrList,
    pub private: AttrList,
    /// The protocol this key belongs to, by its `prototab` name.
    pub proto: &'static str,
}

impl Key {
    /// `%A %N` as the `ctl` file lists it: public attributes, then the
    /// private names as queries. A key without private attributes keeps
    /// 9front's trailing space before the newline.
    pub fn ctl_line(&self) -> String {
        format!("key {} {}\n", self.attrs, self.private.names())
    }
}

/// What a protocol asks of [`Keyring::find`] (9front's `Keyinfo` plus the
/// `findkey` format string).
pub struct KeyQuery<'a> {
    /// `attr0`: normally the conversation's `start` attributes.
    pub attrs: &'a AttrList,
    /// `attr1`: the protocol's own requirements, such as `user? !password?`.
    pub extra: AttrList,
    /// Matching keys to pass over before taking one.
    pub skip: usize,
    /// List keys that want confirmation without asking for it.
    pub no_confirm: bool,
    pub use_disabled: bool,
}

#[derive(Debug)]
pub enum KeyLookup {
    Found(Arc<Key>),
    /// The key wants confirmation (a `confirm` attribute).
    Confirm(Arc<Key>),
    /// No key matched and the request had queries: ask for one described by
    /// this template (empty when keys matched but were all skipped).
    Needkey(String),
    /// The error text for `failure`.
    Failure(String),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteRefusal {
    /// A pattern named a private attribute's value.
    PrivateValue,
    NothingMatched,
}

#[derive(Debug, Default)]
pub struct Keyring {
    keys: Vec<Arc<Key>>,
    /// 9front's `askforkeys`, on unless the agent was told never to prompt.
    pub ask_for_keys: bool,
}

impl Keyring {
    pub fn new() -> Self {
        Self {
            keys: Vec::new(),
            ask_for_keys: true,
        }
    }

    pub fn keys(&self) -> impl Iterator<Item = &Arc<Key>> {
        self.keys.iter()
    }

    /// `replacekey`: a key whose public attributes match an existing key's in
    /// both directions replaces it in place; any other is appended, or put
    /// first when `before` is set.
    pub fn replace(&mut self, key: Key, before: bool) {
        let empty = AttrList::new();
        let key = Arc::new(key);
        if let Some(slot) = self.keys.iter_mut().find(|existing| {
            key.attrs.matches(&existing.attrs, &empty) && existing.attrs.matches(&key.attrs, &empty)
        }) {
            *slot = key;
        } else if before {
            self.keys.insert(0, key);
        } else {
            self.keys.push(key);
        }
    }

    /// `delkey`: removes every key the pattern matches against its public and
    /// private attributes. A private name may appear only as a query, so a
    /// secret can never be probed by guessing its value.
    pub fn delete(&mut self, pattern: &AttrList) -> Result<(), DeleteRefusal> {
        if pattern
            .iter()
            .any(|attr| attr.kind != AttrKind::Query && attr.is_private())
        {
            return Err(DeleteRefusal::PrivateValue);
        }
        let before = self.keys.len();
        self.keys
            .retain(|key| !pattern.matches(&key.attrs, &key.private));
        if self.keys.len() == before {
            return Err(DeleteRefusal::NothingMatched);
        }
        Ok(())
    }

    /// `findkey` (`util.c:366-476`), for the owner. `known` says whether a
    /// protocol name is in the agent's protocol table.
    pub fn find(&self, query: &KeyQuery<'_>, known: impl Fn(&str) -> bool) -> KeyLookup {
        let proto = query
            .attrs
            .value("proto")
            .or_else(|| query.extra.value("proto"));
        if let Some(proto) = proto
            && !known(proto)
        {
            return KeyLookup::Failure(format!("unknown protocol {proto}"));
        }
        let mut matched = 0;
        for key in &self.keys {
            if key.attrs.value("disabled").is_some() && !query.use_disabled {
                continue;
            }
            if !query.attrs.matches(&key.attrs, &key.private)
                || !query.extra.matches(&key.attrs, &key.private)
            {
                continue;
            }
            matched += 1;
            if matched <= query.skip {
                continue;
            }
            if !query.no_confirm && key.attrs.value("confirm").is_some() {
                return KeyLookup::Confirm(Arc::clone(key));
            }
            return KeyLookup::Found(Arc::clone(key));
        }
        if self.ask_for_keys && (query.attrs.has_query() || query.extra.has_query()) {
            if matched != 0 {
                return KeyLookup::Needkey(String::new());
            }
            let mut template = query.attrs.clone();
            template.extend(&query.extra);
            template.delete("role");
            template.delete("disabled");
            return KeyLookup::Needkey(template.sorted().to_string());
        }
        KeyLookup::Failure(format!("no key matches {} {}", query.attrs, query.extra))
    }
}
