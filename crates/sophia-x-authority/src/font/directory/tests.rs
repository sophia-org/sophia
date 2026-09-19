#![cfg(test)]

//! The index is the safeguard, so the tests are mostly about what it refuses.

use std::collections::BTreeMap;
use std::path::Path;

use super::{XFontDirectory, XFontDirectoryEntry, parse_aliases, parse_index, split_token};

#[test]
fn a_fonts_dir_publishes_file_and_name_pairs() {
    let index = parse_index(
        "2\n\
         6x13.pcf.gz -misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso10646-1\n\
         10x20.pcf.gz -misc-fixed-medium-r-normal--20-200-75-75-c-100-iso8859-1\n",
    );
    assert_eq!(index.len(), 2);
    let entry = index
        .get("-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso10646-1")
        .expect("the 6x13 face is published");
    assert_eq!(entry.file, "6x13.pcf.gz");
}

#[test]
fn a_name_is_never_joined_onto_a_path() {
    // The whole point of the index: a client names a font, the directory
    // chooses the file. A published entry that tries to escape the directory
    // is dropped rather than obeyed.
    let index = parse_index(
        "3\n\
         ../../../etc/shadow -evil-escape-1\n\
         sub/dir/font.pcf -evil-escape-2\n\
         .. -evil-escape-3\n",
    );
    assert!(
        index.is_empty(),
        "no entry may name a file outside the directory"
    );

    let directory = XFontDirectory::from_entries(
        Path::new("/usr/share/fonts/X11/misc"),
        BTreeMap::from([(
            "fixed".to_owned(),
            XFontDirectoryEntry {
                name: "fixed".to_owned(),
                file: "6x13.pcf.gz".to_owned(),
            },
        )]),
        BTreeMap::new(),
    );
    assert_eq!(
        directory.file_for("fixed"),
        Some(Path::new("/usr/share/fonts/X11/misc/6x13.pcf.gz").to_path_buf())
    );
    assert_eq!(
        directory.file_for("../../../etc/shadow"),
        None,
        "a name the index never published resolves to nothing at all"
    );
}

#[test]
fn a_malformed_line_is_skipped_and_the_rest_of_the_directory_still_serves() {
    let index = parse_index(
        "4\n\
         no-space-here\n\
         6x13.pcf.gz fixed\n\
         \n\
         7x13.pcf.gz -misc-fixed-medium-r-normal--13-120-75-75-c-70-iso8859-1\n",
    );
    assert_eq!(index.len(), 2, "two good lines survive two bad ones");
}

#[test]
fn a_name_with_a_control_character_is_not_published() {
    let index = parse_index("1\n6x13.pcf.gz fix\u{7}ed\n");
    assert!(index.is_empty());
}

#[test]
fn aliases_carry_comments_and_quoted_names() {
    let aliases = parse_aliases(
        "! the standard aliases\n\
         fixed -misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1\n\
         \n\
         \"a spaced alias\" \"-misc-fixed-medium-r-normal--13-120-75-75-c-70-iso8859-1\"\n\
         6x13bold -misc-fixed-bold-r-semicondensed--13-120-75-75-c-60-iso8859-1\n",
    );
    assert_eq!(aliases.len(), 3);
    assert_eq!(
        aliases.get("fixed").map(String::as_str),
        Some("-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1")
    );
    assert!(aliases.contains_key("a spaced alias"));
}

#[test]
fn a_token_stops_at_whitespace_or_the_closing_quote() {
    assert_eq!(
        split_token("fixed rest"),
        Some(("fixed".to_owned(), " rest"))
    );
    assert_eq!(
        split_token("\"two words\" rest"),
        Some(("two words".to_owned(), " rest"))
    );
    assert_eq!(split_token(""), None);
    assert_eq!(split_token("\"unterminated"), None);
}

#[test]
fn the_host_directory_indexes_if_it_is_present() {
    // Not a requirement: this authority must serve text with no font
    // directory at all. When the host does have one, indexing it must
    // succeed, which is the only part of the path that touches a real
    // filesystem.
    let root = Path::new("/usr/share/fonts/X11/misc");
    if !root.join("fonts.dir").exists() {
        return;
    }
    let directory = XFontDirectory::index(root).expect("the host directory indexes");
    assert!(
        directory.names().count() > 100,
        "the misc directory publishes hundreds of faces"
    );
    assert!(
        directory.file_for("fixed").is_some() || directory.alias("fixed").is_some(),
        "the standard fixed alias resolves"
    );
}
