//! Key matching, ported from 9front `sys/src/cmd/auth/factotum/util.c`
//! (`matchattr`, `hasname`, `hasnameval`, `ignored`; MIT, Copyright (c)
//! 2021 Plan 9 Foundation and 9front authors).

use super::{AttrKind, AttrList};

/// Pattern names matched as defaults whatever their parsed kind, so
/// `role=client` also matches a key without a `role` (`util.c:337-351`).
const IGNORED: [&str; 2] = ["role", "disabled"];

impl AttrList {
    /// Whether every entry of `self` holds against the union of `public`
    /// and `private` (`matchattr`, `util.c:590-615`).
    ///
    /// - A query needs the name present as a non-query.
    /// - A name-value needs some entry of that name with exactly that value.
    /// - A default, and any pattern named `role` or `disabled`, needs that
    ///   value only if the name is present at all.
    pub fn matches(&self, public: &AttrList, private: &AttrList) -> bool {
        self.iter().all(|pattern| {
            let kind = if IGNORED.contains(&pattern.name.as_str()) {
                AttrKind::Default
            } else {
                pattern.kind
            };
            let present =
                || public.find(&pattern.name).is_some() || private.find(&pattern.name).is_some();
            let valued = || {
                public.has_value(&pattern.name, &pattern.value)
                    || private.has_value(&pattern.name, &pattern.value)
            };
            match kind {
                AttrKind::Query => present(),
                AttrKind::Nameval => valued(),
                AttrKind::Default => !present() || valued(),
            }
        })
    }

    /// `hasnameval` over one list: some non-query entry has this exact
    /// value.
    fn has_value(&self, name: &str, value: &str) -> bool {
        self.iter()
            .any(|attr| attr.kind != AttrKind::Query && attr.name == name && attr.value == value)
    }
}
