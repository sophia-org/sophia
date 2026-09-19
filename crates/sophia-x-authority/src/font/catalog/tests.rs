#![cfg(test)]

//! Resolution order, the cache bounds, and the safeguards.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{
    X_FONT_CACHE_MAX_FONTS, XFontCatalog, XFontOpenError, XFontOpenSource, inflate_gzip, resolve_in,
};
use crate::font::directory::{XFontDirectory, XFontDirectoryEntry};

fn entry(name: &str, file: &str) -> (String, XFontDirectoryEntry) {
    (
        name.to_lowercase(),
        XFontDirectoryEntry {
            name: name.to_owned(),
            file: file.to_owned(),
        },
    )
}

fn directory() -> XFontDirectory {
    XFontDirectory::from_entries(
        Path::new("/fonts"),
        BTreeMap::from([
            entry(
                "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1",
                "6x13.pcf.gz",
            ),
            entry(
                "-misc-fixed-bold-r-normal--20-200-75-75-c-100-iso8859-1",
                "10x20B.pcf.gz",
            ),
        ]),
        BTreeMap::from([
            (
                "fixed".to_owned(),
                "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1".to_owned(),
            ),
            ("terminal".to_owned(), "fixed".to_owned()),
            ("loop-a".to_owned(), "loop-b".to_owned()),
            ("loop-b".to_owned(), "loop-a".to_owned()),
        ]),
    )
}

#[test]
fn resolution_tries_the_exact_name_then_the_alias_then_a_pattern() {
    let directory = directory();
    let exact = "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1";
    assert_eq!(resolve_in(&directory, exact).as_deref(), Some(exact));
    assert_eq!(
        resolve_in(&directory, "fixed").as_deref(),
        Some(exact),
        "an alias resolves to the name it stands for"
    );
    assert_eq!(
        resolve_in(&directory, "terminal").as_deref(),
        Some(exact),
        "and a chain of aliases follows through"
    );
    assert_eq!(
        resolve_in(&directory, "-misc-fixed-medium-*").as_deref(),
        Some(exact),
        "a pattern picks a published name"
    );
    assert_eq!(resolve_in(&directory, "no-such-font").as_deref(), None);
}

#[test]
fn a_circular_alias_stops_rather_than_spinning() {
    // Two aliases naming each other. The hop bound is what ends it.
    assert_eq!(resolve_in(&directory(), "loop-a").as_deref(), None);
}

#[test]
fn a_bare_session_still_serves_the_built_in_face() {
    // No host directories at all: the element that is always present answers,
    // which is why a session never fails to render text.
    let mut catalog = XFontCatalog::builtin_only();
    let (font, source) = catalog.open("fixed").expect("the built-in face opens");
    assert_eq!(source, XFontOpenSource::Builtin);
    assert_eq!(font.metrics.font_ascent, 11);
    assert_eq!(
        font.metrics.max_byte1, 0,
        "the built-in face is single byte"
    );
    assert!(font.metrics.all_chars_exist);
    for name in ["6x13", "cursor", "nil2", "FIXED"] {
        assert!(catalog.open(name).is_ok(), "{name} opens");
    }
    assert_eq!(
        catalog.open("helvetica").unwrap_err(),
        XFontOpenError::NotFound
    );
}

#[test]
fn a_name_that_is_not_well_formed_never_reaches_the_filesystem() {
    let mut catalog = XFontCatalog::builtin_only();
    for name in ["", "fix\u{7}ed", "../../etc/shadow"] {
        assert_eq!(catalog.open(name).unwrap_err(), XFontOpenError::NotFound);
    }
    assert_eq!(catalog.refused(), 3);
}

#[test]
fn a_reopened_face_comes_from_the_cache() {
    let mut catalog = XFontCatalog::builtin_only();
    let (first, source) = catalog.open("fixed").expect("first open");
    assert_eq!(source, XFontOpenSource::Builtin);
    let (second, source) = catalog.open("fixed").expect("second open");
    assert_eq!(source, XFontOpenSource::Cached);
    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "the same face is shared rather than parsed twice"
    );
    assert_eq!(catalog.opened(), 1);
    assert_eq!(catalog.cached_fonts(), 1);
}

#[test]
fn the_cache_is_bounded_by_count() {
    // Every built-in name is the same face, so fill the cache with distinct
    // keys by asking for patterns that all resolve to it.
    let mut catalog = XFontCatalog::builtin_only();
    for index in 0..=X_FONT_CACHE_MAX_FONTS {
        let pattern = format!("*fixed{}", "*".repeat(index));
        let _ = catalog.open(&pattern);
    }
    assert!(
        catalog.cached_fonts() <= X_FONT_CACHE_MAX_FONTS,
        "held {} faces, over the bound",
        catalog.cached_fonts()
    );
    assert!(
        catalog.evicted() > 0,
        "the bound actually evicted something"
    );
}

#[test]
fn listing_answers_a_pattern_and_honours_its_limit() {
    let catalog = XFontCatalog::builtin_only();
    assert!(
        catalog.list("*", 0).is_empty(),
        "a zero limit lists nothing"
    );
    let all = catalog.list("*", 100);
    assert!(all.iter().any(|name| name == "fixed"));
    assert!(all.iter().any(|name| name == "6x13"));
    assert_eq!(catalog.list("*", 2).len(), 2, "the limit is obeyed");
    assert!(catalog.list("helvetica*", 10).is_empty());
}

#[test]
fn a_gzip_member_inflates_and_a_corrupt_one_does_not() {
    // The host ships every core font gzipped, so this is the ordinary path.
    let raw = std::fs::read("../../tools/fixtures/fonts/6x13-iso8859-1.pcf")
        .expect("the checked-in fixture");
    let mut gzipped = vec![0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0, 0x03];
    gzipped.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(&raw, 6));
    gzipped.extend_from_slice(&[0; 8]);
    assert_eq!(inflate_gzip(&gzipped).as_deref(), Some(raw.as_slice()));
    assert_eq!(inflate_gzip(&[0x1f, 0x8b, 0x08, 0x00]), None);
    assert_eq!(inflate_gzip(&[]), None);
}

#[test]
fn the_host_path_resolves_a_real_face_when_one_is_configured() {
    // The end-to-end shape of the host element: index a real directory,
    // resolve a name through it, decompress and parse the file it published.
    // Skipped where the host ships no X11 core fonts; the built-in element
    // covers that case and has its own test above.
    let path: Vec<PathBuf> = XFontCatalog::default_path();
    if path.is_empty() {
        return;
    }
    let mut catalog = XFontCatalog::index(&path);
    let (font, source) = catalog
        .open("-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso10646-1")
        .expect("the host's Unicode 6x13 opens");
    assert_eq!(source, XFontOpenSource::Host);
    assert_eq!(font.metrics.font_ascent, 11);
    assert_eq!(font.metrics.font_descent, 2);
    assert_eq!(
        font.metrics.max_byte1, 255,
        "the Unicode face is a two-byte matrix, which is the whole point"
    );
    assert!(
        font.glyph(0x2500).is_some(),
        "box drawings light horizontal is present in the real face"
    );
    assert!(font.glyph(0x03a9).is_some(), "as is capital omega");
    assert!(font.glyph(0x00e9).is_some(), "and e-acute");
}
