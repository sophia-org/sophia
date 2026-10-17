//! The schema reader's class rule. Image-channel kinds carry one explicit
//! class each; every other kind keeps the numeric ranges, including kinds
//! next to the image namespace.
#[path = "support/file_schema.rs"]
#[allow(dead_code)] // Only the class rule and the parser are exercised here.
mod file_schema;

use std::panic::{AssertUnwindSafe, catch_unwind};

use file_schema::*;

/// A minimal contract declaring one kind with its body.
fn contract(class: &str, kind: u16) -> String {
    format!(
        "protocol \"probe\" api-version=1 {{\n\
         \x20   {class} \"Probe\" kind={kind}\n\
         \x20   body \"Probe\" size=8 {{\n\
         \x20       field \"value\" type=\"u64\" offset=0\n\
         \x20   }}\n\
         }}\n"
    )
}

fn parses(class: &str, kind: u16) -> bool {
    let text = contract(class, kind);
    // A refused declaration panics inside the parser; its message is noise.
    match catch_unwind(AssertUnwindSafe(|| Schema::parse(&text))) {
        Ok(schema) => {
            assert_eq!(schema.kinds["Probe"], (class.to_owned(), kind));
            true
        }
        Err(_) => false,
    }
}

const CLASSES: [&str; 3] = ["object", "event", "candidate"];

#[test]
fn image_kinds_take_exactly_their_contract_class() {
    let expected = [
        (0x7000, "candidate"),
        (0x7001, "event"),
        (0x7002, "event"),
        (0x7003, "event"),
        (0x7004, "event"),
        (0x7005, "object"),
        (0x7006, "object"),
    ];
    for (kind, class) in expected {
        assert_eq!(image_class(kind), Some(class), "{kind:#x}");
        for other in CLASSES {
            assert_eq!(parses(other, kind), other == class, "{other} {kind:#x}");
        }
    }
}

#[test]
fn numeric_ranges_still_bind_every_other_kind() {
    for (kind, class) in [
        (1, "object"),
        (15, "object"),
        (16, "event"),
        (255, "event"),
        (256, "candidate"),
        (0x6fff, "candidate"),
        (0x7007, "candidate"),
        (u16::MAX, "candidate"),
    ] {
        assert_eq!(image_class(kind), None, "{kind:#x}");
        for other in CLASSES {
            assert_eq!(parses(other, kind), other == class, "{other} {kind:#x}");
        }
    }
    // Kind 0 belongs to no class.
    for class in CLASSES {
        assert!(!parses(class, 0));
    }
}
