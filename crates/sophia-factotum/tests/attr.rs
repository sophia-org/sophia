//! Attribute lists against 9front's `libauth/attr.c`, libc quoting and
//! factotum's `util.c`. Expected values are worked from the C source.

use sophia_factotum::attr::{AttrKind, AttrList, quote, tokenize};

#[test]
fn tokens_follow_rc_quoting() {
    assert_eq!(tokenize("  a\tb\r\nc ", 256), ["a", "b", "c"]);
    assert_eq!(tokenize("'a b' c", 256), ["a b", "c"]);
    assert_eq!(tokenize("'it''s' x", 256), ["it's", "x"]);
    assert_eq!(tokenize("a'b c'd", 256), ["ab cd"]);
    assert_eq!(tokenize("'unterminated quote", 256), ["unterminated quote"]);
    assert_eq!(
        tokenize("a b c", 2),
        ["a", "b"],
        "tokens past the limit drop"
    );
}

#[test]
fn quoting_matches_needsrcquote() {
    assert_eq!(quote(""), "''");
    assert_eq!(quote("plain"), "plain");
    assert_eq!(quote("!password"), "!password", "! needs no quote");
    assert_eq!(quote("a b"), "'a b'");
    assert_eq!(quote("it's"), "'it''s'");
    assert_eq!(quote("a=b"), "'a=b'");
    assert_eq!(quote("x?"), "'x?'");
    assert_eq!(quote("tab\there"), "'tab\there'");
}

#[test]
fn parsing_splits_at_the_first_equals_and_marks_queries() {
    let list = AttrList::parse("proto=pass a=b=c bare user? x?");
    let entries = list
        .iter()
        .map(|attr| (attr.kind, attr.name.as_str(), attr.value.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        entries,
        [
            (AttrKind::Nameval, "proto", "pass"),
            (AttrKind::Nameval, "a", "b=c"),
            (AttrKind::Nameval, "bare", ""),
            (AttrKind::Query, "user", ""),
            (AttrKind::Query, "x", ""),
        ]
    );
}

#[test]
fn a_query_answered_anywhere_in_the_list_is_dropped() {
    assert_eq!(
        AttrList::parse("user? user=glenda").to_string(),
        "user=glenda"
    );
    assert_eq!(
        AttrList::parse("user=glenda user?").to_string(),
        "user=glenda"
    );
}

#[test]
fn formatting_quotes_names_and_values() {
    assert_eq!(
        AttrList::parse("user='a b' dom=x bare").to_string(),
        "user='a b' dom=x bare=''"
    );
}

#[test]
fn private_names_read_back_as_queries_without_values() {
    assert_eq!(AttrList::parse("!a=1 !b=2").names(), "!a? !b?");
    assert_eq!(AttrList::new().names(), "");
    assert_eq!(
        AttrList::parse("user=tb !password=secret").masked(),
        "user=tb !password?"
    );
    assert!(!format!("{:?}", AttrList::parse("!password=secret")).contains("secret"));
}

#[test]
fn find_skips_queries_and_takes_the_first() {
    let list = AttrList::parse("dom=a dom=b");
    assert_eq!(list.value("dom"), Some("a"));
    assert_eq!(AttrList::parse("user?").value("user"), None);
}

#[test]
fn set_attrs_replaces_the_first_and_drops_the_rest() {
    let mut list = AttrList::parse("proto=pass user=a user=b");
    list.set_attrs(&AttrList::parse("user=c dom=d"));
    assert_eq!(list.to_string(), "proto=pass user=c dom=d");

    let mut list = AttrList::parse("user=a");
    list.set_attrs(&AttrList::parse("user? x?"));
    assert_eq!(
        list.to_string(),
        "user=a x?",
        "a query only adds a missing name"
    );
}

#[test]
fn sorting_is_9fronts_unstable_merge_sort() {
    // A stable sort would give a=1 a=2; 9front's dealing reverses them.
    assert_eq!(
        AttrList::parse("a=1 b a=2").sorted().to_string(),
        "a=2 a=1 b=''"
    );
    assert_eq!(
        AttrList::parse("proto=pass user? !password?")
            .sorted()
            .to_string(),
        "!password? proto=pass user?"
    );
}

#[test]
fn matching_treats_role_and_disabled_as_defaults() {
    let key = AttrList::parse("proto=pass user=x");
    let none = AttrList::new();
    assert!(AttrList::parse("proto=pass role=client").matches(&key, &none));
    let server = AttrList::parse("proto=pass role=server");
    assert!(!AttrList::parse("role=client").matches(&server, &none));
    assert!(AttrList::parse("user?").matches(&key, &none));
    assert!(!AttrList::parse("dom?").matches(&key, &none));
    assert!(!AttrList::parse("user=y").matches(&key, &none));
    let private = AttrList::parse("!password=secret");
    assert!(AttrList::parse("!password?").matches(&key, &private));
}
