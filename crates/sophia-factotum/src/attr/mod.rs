//! Attribute lists: the `name=value` and `name?` tuples that describe keys,
//! conversations and needkey templates.
//!
//! Ported from 9front (MIT, Copyright (c) 2021 Plan 9 Foundation and 9front
//! authors): `sys/src/libauth/attr.c` (`_parseattr`, `cleanattr`,
//! `_findattr`, `_delattr`, `_attrfmt`) and `sys/src/cmd/auth/factotum/util.c`
//! (`setattrs`, `sortattr`, `attrnamefmt`). The order of entries and the
//! quirks of each operation are kept, because they reach the wire.

mod matching;
mod quote;

pub use quote::{quote, tokenize};

/// `_parseattr` reads at most this many tokens and drops the rest.
const MAX_TOKENS: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttrKind {
    /// `name=value`; a bare word is a name-value with an empty value.
    Nameval,
    /// `name?`: the name must be present, with any value.
    Query,
    /// Internal only, never parsed: `name=value` that must hold only if the
    /// name is present.
    Default,
}

#[derive(Clone, Eq, PartialEq)]
pub struct Attr {
    pub kind: AttrKind,
    pub name: String,
    pub value: String,
}

impl Attr {
    pub fn new(kind: AttrKind, name: &str, value: &str) -> Self {
        Self {
            kind,
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }

    /// Whether this is a private (`!`) attribute, whose value is a secret.
    pub fn is_private(&self) -> bool {
        self.name.starts_with('!')
    }
}

impl core::fmt::Debug for Attr {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // A private value never appears in debug output.
        let value: &str = if self.is_private() { ".." } else { &self.value };
        formatter
            .debug_struct("Attr")
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("value", &value)
            .finish()
    }
}

impl Drop for Attr {
    fn drop(&mut self) {
        // Private values are secrets; the others cost nothing to clear.
        if self.is_private() {
            zeroize::Zeroize::zeroize(&mut self.value);
        }
    }
}

/// An ordered attribute list. Order is significant: lookups take the first
/// match, and formatting keeps it.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AttrList(Vec<Attr>);

impl AttrList {
    pub const fn new() -> Self {
        Self(Vec::new())
    }

    /// `_parseattr`: tokenizes with rc quoting; a token with `=` splits at
    /// the first one, a token ending in `?` is a query, any other word is a
    /// name-value with an empty value. Queries for names that also appear as
    /// non-queries are then removed (`cleanattr`).
    pub fn parse(text: &str) -> Self {
        let mut list = Self(
            tokenize(text, MAX_TOKENS)
                .into_iter()
                .map(|token| {
                    if let Some((name, value)) = token.split_once('=') {
                        Attr::new(AttrKind::Nameval, name, value)
                    } else if let Some(name) = token.strip_suffix('?') {
                        Attr::new(AttrKind::Query, name, "")
                    } else {
                        Attr::new(AttrKind::Nameval, &token, "")
                    }
                })
                .collect(),
        );
        list.clean();
        list
    }

    fn clean(&mut self) {
        let answered = self
            .0
            .iter()
            .filter(|attr| attr.kind != AttrKind::Query)
            .map(|attr| attr.name.clone())
            .collect::<Vec<_>>();
        self.0
            .retain(|attr| attr.kind != AttrKind::Query || !answered.contains(&attr.name));
    }

    pub fn iter(&self) -> impl Iterator<Item = &Attr> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn push(&mut self, attr: Attr) {
        self.0.push(attr);
    }

    pub fn insert_front(&mut self, attr: Attr) {
        self.0.insert(0, attr);
    }

    /// Appends every entry of `other`, keeping both orders.
    pub fn extend(&mut self, other: &AttrList) {
        self.0.extend(other.0.iter().cloned());
    }

    /// `_findattr`: the first entry of that name that is not a query.
    pub fn find(&self, name: &str) -> Option<&Attr> {
        self.0
            .iter()
            .find(|attr| attr.name == name && attr.kind != AttrKind::Query)
    }

    /// `_strfindattr`.
    pub fn value(&self, name: &str) -> Option<&str> {
        self.find(name).map(|attr| attr.value.as_str())
    }

    /// Whether any entry is a query.
    pub fn has_query(&self) -> bool {
        self.0.iter().any(|attr| attr.kind == AttrKind::Query)
    }

    /// `_delattr`: removes every entry of that name.
    pub fn delete(&mut self, name: &str) {
        self.0.retain(|attr| attr.name != name);
    }

    /// Removes and returns every entry the predicate selects, in order.
    pub fn take_where(&mut self, mut select: impl FnMut(&Attr) -> bool) -> AttrList {
        let (taken, kept) = std::mem::take(&mut self.0)
            .into_iter()
            .partition(|attr| select(attr));
        self.0 = kept;
        AttrList(taken)
    }

    /// `setattrs`: for each entry of `other`, a name-value replaces the
    /// first same-named entry here (making it a name-value) and deletes the
    /// later ones; a query is skipped when the name is already here;
    /// anything not found is appended.
    pub fn set_attrs(&mut self, other: &AttrList) {
        for attr in other.iter() {
            let mut found = false;
            let mut skip = false;
            let mut index = 0;
            while index < self.0.len() {
                if self.0[index].name != attr.name {
                    index += 1;
                    continue;
                }
                match attr.kind {
                    AttrKind::Nameval if !found => {
                        found = true;
                        self.0[index].value = attr.value.clone();
                        self.0[index].kind = AttrKind::Nameval;
                        index += 1;
                    }
                    AttrKind::Nameval => {
                        self.0.remove(index);
                    }
                    AttrKind::Query => {
                        skip = true;
                        break;
                    }
                    // Never produced by parsing; 9front's switch has no arm
                    // for it either, and keeping the entry is the only
                    // terminating reading of that.
                    AttrKind::Default => index += 1,
                }
            }
            if !found && !skip {
                self.0.push(attr.clone());
            }
        }
    }

    /// `setattr`: parses `text` and applies it with [`AttrList::set_attrs`].
    pub fn set(&mut self, text: &str) {
        self.set_attrs(&AttrList::parse(text));
    }

    /// `sortattr`: 9front's recursive merge sort, ported exactly. It is not
    /// stable, and the order it gives equal names reaches needkey templates.
    pub fn sorted(self) -> Self {
        Self(sort(self.0))
    }

    /// `%N` (`attrnamefmt`): each name as `name?`, so a private list reads
    /// back without its values. An empty list is the empty string.
    pub fn names(&self) -> String {
        self.0
            .iter()
            .map(|attr| format!("{}?", quote(&attr.name)))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// `%A` with every private value written as a query, for logs.
    pub fn masked(&self) -> String {
        self.0
            .iter()
            .map(|attr| match attr.kind {
                _ if attr.is_private() => format!("{}?", quote(&attr.name)),
                AttrKind::Query => format!("{}?", quote(&attr.name)),
                AttrKind::Nameval | AttrKind::Default => {
                    format!("{}={}", quote(&attr.name), quote(&attr.value))
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// `%A` (`_attrfmt`): entries joined by single spaces, a query as `name?`
/// and anything else as `name=value`, each part rc-quoted.
impl core::fmt::Display for AttrList {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (index, attr) in self.0.iter().enumerate() {
            if index != 0 {
                formatter.write_str(" ")?;
            }
            match attr.kind {
                AttrKind::Query => write!(formatter, "{}?", quote(&attr.name))?,
                AttrKind::Nameval | AttrKind::Default => {
                    write!(formatter, "{}={}", quote(&attr.name), quote(&attr.value))?;
                }
            }
        }
        Ok(())
    }
}

impl FromIterator<Attr> for AttrList {
    fn from_iter<I: IntoIterator<Item = Attr>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// `sortattr` over a vector. 9front deals the entries alternately onto the
/// front of two lists (the even-indexed to one, the odd-indexed to the other,
/// each reversed by prepending), sorts each, and merges taking the first
/// list's head only when its name is strictly smaller.
fn sort(list: Vec<Attr>) -> Vec<Attr> {
    if list.len() < 2 {
        return list;
    }
    let mut odd = Vec::new();
    let mut even = Vec::new();
    for (index, attr) in list.into_iter().enumerate() {
        if index % 2 == 1 {
            odd.push(attr);
        } else {
            even.push(attr);
        }
    }
    odd.reverse();
    even.reverse();
    let mut odd = sort(odd).into_iter().peekable();
    let mut even = sort(even).into_iter().peekable();
    let mut merged = Vec::with_capacity(odd.len() + even.len());
    loop {
        let take_odd = match (odd.peek(), even.peek()) {
            (None, None) => break,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(a0), Some(a1)) => a0.name.as_bytes() < a1.name.as_bytes(),
        };
        let next = if take_odd { odd.next() } else { even.next() };
        merged.extend(next);
    }
    merged
}
