use sophia_conformance::record::{
    RecordError, RecordFields, after_marker, each_after_marker, last_after_marker,
};

const MARKER: &str = "sophia_example_record schema=1 ";

#[test]
fn marker_is_found_in_bare_and_decorated_lines() {
    let bare = "sophia_example_record schema=1 status=ok count=2";
    let decorated = "2026-09-27T10:00:00Z \u{1b}[32m INFO\u{1b}[0m sophia: sophia_example_record schema=1 status=ok count=2";
    assert_eq!(after_marker(bare, MARKER), Some("status=ok count=2"));
    assert_eq!(after_marker(decorated, MARKER), Some("status=ok count=2"));
    assert_eq!(
        after_marker("sophia_other schema=1 status=ok", MARKER),
        None
    );
}

#[test]
fn last_and_each_follow_line_order() {
    let text =
        "sophia_example_record schema=1 n=1\nnoise\nprefix sophia_example_record schema=1 n=2\n";
    assert_eq!(last_after_marker(text, MARKER), Some("n=2"));
    assert_eq!(
        each_after_marker(text, MARKER).collect::<Vec<_>>(),
        ["n=1", "n=2"]
    );
    assert_eq!(last_after_marker("noise\n", MARKER), None);
}

#[test]
fn strict_parse_reads_every_field() {
    let fields = RecordFields::parse("status=ok count=2 empty=").unwrap();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields.get("status"), Some("ok"));
    assert_eq!(fields.get("empty"), Some(""));
    assert_eq!(fields.require("count"), Ok("2"));
    assert_eq!(
        fields.iter().collect::<Vec<_>>(),
        [("count", "2"), ("empty", ""), ("status", "ok")]
    );
    assert!(RecordFields::parse("").unwrap().is_empty());
}

#[test]
fn strict_parse_refuses_bare_tokens_empty_keys_and_duplicates() {
    assert_eq!(
        RecordFields::parse("sophia_example_record status=ok"),
        Err(RecordError::MalformedField {
            token: "sophia_example_record".into()
        })
    );
    assert_eq!(
        RecordFields::parse("status=ok =value"),
        Err(RecordError::MalformedField {
            token: "=value".into()
        })
    );
    assert_eq!(
        RecordFields::parse("status=ok status=bad"),
        Err(RecordError::DuplicateField {
            name: "status".into()
        })
    );
}

#[test]
fn named_parse_skips_exactly_one_leading_name() {
    let fields = RecordFields::parse_named("sophia_example_record schema=1 status=ok").unwrap();
    assert_eq!(fields.get("schema"), Some("1"));
    assert_eq!(fields.len(), 2);
    assert!(
        RecordFields::parse_named("sophia_example_record")
            .unwrap()
            .is_empty()
    );
    // Without a leading name the grammar is simply strict.
    assert_eq!(
        RecordFields::parse_named("a=1").unwrap().get("a"),
        Some("1")
    );
}

#[test]
fn named_parse_refuses_a_later_or_second_bare_token() {
    assert_eq!(
        RecordFields::parse_named("sophia_example_record status=ok stray"),
        Err(RecordError::MalformedField {
            token: "stray".into()
        })
    );
    assert_eq!(
        RecordFields::parse_named("sophia_example_record second status=ok"),
        Err(RecordError::MalformedField {
            token: "second".into()
        })
    );
    assert_eq!(
        RecordFields::parse_named("name =value"),
        Err(RecordError::MalformedField {
            token: "=value".into()
        })
    );
    assert_eq!(
        RecordFields::parse_named("name a=1 a=2"),
        Err(RecordError::DuplicateField { name: "a".into() })
    );
}

#[test]
fn unsigned_accepts_only_canonical_decimal() {
    let fields = RecordFields::parse(
        "zero=0 small=42 max=18446744073709551615 plus=+1 minus=-1 lead=01 over=18446744073709551616 empty= hex=0x10 space=1_000",
    )
    .unwrap();
    assert_eq!(fields.unsigned("zero"), Ok(0));
    assert_eq!(fields.unsigned("small"), Ok(42));
    assert_eq!(fields.unsigned("max"), Ok(u64::MAX));
    for name in ["plus", "minus", "lead", "over", "empty", "hex", "space"] {
        assert!(
            matches!(
                fields.unsigned(name),
                Err(RecordError::InvalidUnsigned { name: ref field, .. }) if field == name
            ),
            "{name} was accepted"
        );
    }
    assert_eq!(
        fields.unsigned("absent"),
        Err(RecordError::MissingField {
            name: "absent".into()
        })
    );
}

#[test]
fn require_names_the_missing_field() {
    let error = RecordFields::parse("a=1")
        .unwrap()
        .require("b")
        .unwrap_err();
    assert_eq!(error, RecordError::MissingField { name: "b".into() });
    assert_eq!(error.to_string(), "record is missing field b");
}
