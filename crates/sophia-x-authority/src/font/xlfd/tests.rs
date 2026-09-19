#![cfg(test)]

use super::{X_FONT_NAME_MAX_LEN, is_pattern, name_is_well_formed, pattern_matches};

#[test]
fn a_pattern_selects_a_family_the_way_a_font_menu_expects() {
    let face = "-misc-fixed-medium-r-semicondensed--13-120-75-75-c-60-iso8859-1";
    assert!(pattern_matches("*", face));
    assert!(pattern_matches("-misc-fixed-*", face));
    assert!(pattern_matches("*-iso8859-1", face));
    assert!(pattern_matches("-misc-fixed-*-13-*-iso8859-1", face));
    assert!(!pattern_matches("-misc-fixed-bold-*", face));
    assert!(!pattern_matches("*-iso10646-1", face));
}

#[test]
fn matching_ignores_case_because_font_names_do() {
    assert!(pattern_matches(
        "-MISC-FIXED-*",
        "-misc-fixed-medium-r-normal--13-0-0-0-c-0-iso8859-1"
    ));
    assert!(pattern_matches("fixed", "FIXED"));
}

#[test]
fn a_single_character_wildcard_spans_exactly_one() {
    assert!(pattern_matches("6x1?", "6x13"));
    assert!(!pattern_matches("6x1?", "6x1"));
    assert!(!pattern_matches("6x1?", "6x134"));
}

#[test]
fn consecutive_stars_do_not_blow_up() {
    // The shape that makes a naive backtracking matcher exponential. The
    // matcher is iterative, so this is linear and must simply answer.
    let pattern = "*".repeat(40) + "z";
    assert!(!pattern_matches(&pattern, &"a".repeat(120)));
    assert!(pattern_matches(&pattern, &("a".repeat(120) + "z")));
}

#[test]
fn a_trailing_star_matches_the_empty_remainder() {
    assert!(pattern_matches("fixed*", "fixed"));
    assert!(pattern_matches("*fixed*", "fixed"));
    assert!(pattern_matches("", ""));
    assert!(!pattern_matches("", "fixed"));
}

#[test]
fn a_name_with_a_control_character_or_no_length_is_refused() {
    assert!(name_is_well_formed("fixed"));
    assert!(name_is_well_formed(
        "-misc-fixed-medium-r-normal--13-120-75-75-c-60-iso8859-1"
    ));
    assert!(!name_is_well_formed(""));
    assert!(!name_is_well_formed("fix\ted"));
    assert!(!name_is_well_formed("fix\0ed"));
    assert!(!name_is_well_formed(&"a".repeat(X_FONT_NAME_MAX_LEN + 1)));
}

#[test]
fn a_plain_name_is_not_a_pattern() {
    assert!(!is_pattern("fixed"));
    assert!(is_pattern("fixed*"));
    assert!(is_pattern("6x1?"));
}
