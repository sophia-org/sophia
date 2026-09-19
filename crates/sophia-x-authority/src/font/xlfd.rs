//! X Logical Font Description names and the patterns that select them.
//!
//! Portions derived from yserver (MIT, Copyright (c) 2026 Jos Dehaes):
//! `crates/yserver/src/kms/core.rs:234-249`, the wildcard matcher.
//!
//! A font name is matched only against names a directory index already
//! published. Nothing here ever becomes a path, so a pattern cannot reach
//! outside the indexed set however it is spelled.

mod tests;

/// The characters a client may put in a font name.
///
/// An XLFD is printable ASCII by construction. Refusing everything else keeps
/// a control character or a stray byte out of the comparison and out of any
/// record that later quotes the pattern.
pub fn name_is_well_formed(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= X_FONT_NAME_MAX_LEN
        && name
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ')
}

/// The longest font name or pattern this authority will consider.
///
/// A full XLFD is fourteen fields and well under this; the bound exists so a
/// pattern cannot make matching quadratic in a client-chosen length.
pub const X_FONT_NAME_MAX_LEN: usize = 255;

/// Whether `name` matches an XLFD `pattern` with `*` and `?` wildcards.
///
/// Case-insensitive, as font names are. `*` matches any run including none,
/// `?` exactly one character.
pub fn pattern_matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().flat_map(char::to_lowercase).collect();
    let name: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    matches_from(&pattern, &name)
}

/// Iterative matcher: on a mismatch it returns to the last `*` and lets it
/// consume one more character. Iterative rather than recursive so a pattern of
/// many stars cannot grow the stack.
fn matches_from(pattern: &[char], name: &[char]) -> bool {
    let (mut pattern_at, mut name_at) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while name_at < name.len() {
        match pattern.get(pattern_at) {
            Some('*') => {
                star = Some((pattern_at, name_at));
                pattern_at += 1;
            }
            Some('?') => {
                pattern_at += 1;
                name_at += 1;
            }
            Some(candidate) if *candidate == name[name_at] => {
                pattern_at += 1;
                name_at += 1;
            }
            _ => match star {
                Some((star_at, resume)) => {
                    pattern_at = star_at + 1;
                    name_at = resume + 1;
                    star = Some((star_at, resume + 1));
                }
                None => return false,
            },
        }
    }
    pattern[pattern_at..].iter().all(|entry| *entry == '*')
}

/// Whether a name carries a wildcard at all.
pub fn is_pattern(name: &str) -> bool {
    name.contains('*') || name.contains('?')
}
