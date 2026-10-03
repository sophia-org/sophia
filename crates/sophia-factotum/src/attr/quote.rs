//! Plan 9's rc-style tokenizing and quoting, as factotum's text files use it.
//!
//! Ported from 9front (MIT, Copyright (c) 2021 Plan 9 Foundation and 9front
//! authors): `sys/src/libc/port/tokenize.c` (`tokenize`, `qtoken`),
//! `sys/src/libc/port/needsrcquote.c` and the quoting rule of
//! `sys/src/libc/fmt/fmtquote.c` (`_quotesetup`).

/// `tokenize`'s separators. All are ASCII, so splitting bytes never splits a
/// UTF-8 sequence.
const SEPARATORS: &[u8] = b" \t\r\n";

/// Splits `input` into at most `max` tokens with rc quoting: `'...'` quotes
/// separators, and `''` inside a quote is one `'`. Tokens past `max` are
/// dropped, as `tokenize` drops them.
pub fn tokenize(input: &str, max: usize) -> Vec<String> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut at = 0;
    while tokens.len() < max {
        while at < bytes.len() && SEPARATORS.contains(&bytes[at]) {
            at += 1;
        }
        if at == bytes.len() {
            break;
        }
        let mut token = Vec::new();
        let mut quoting = false;
        while at < bytes.len() && (quoting || !SEPARATORS.contains(&bytes[at])) {
            if bytes[at] != b'\'' {
                token.push(bytes[at]);
                at += 1;
            } else if !quoting {
                quoting = true;
                at += 1;
            } else if bytes.get(at + 1) == Some(&b'\'') {
                // A doubled quote inside a quote is one quote.
                token.push(b'\'');
                at += 2;
            } else {
                // The closing quote.
                quoting = false;
                at += 1;
            }
        }
        // Only ASCII quotes were removed, so the token is still UTF-8.
        tokens.push(String::from_utf8_lossy(&token).into_owned());
    }
    tokens
}

/// `needsrcquote`, plus the space, control and quote runes `_quotesetup`
/// always quotes.
fn needs_quote(c: char) -> bool {
    c <= ' ' || c == '\'' || "`^#*[]=|\\?${}()'<>&;".contains(c)
}

/// `%q`: the string as rc would need it written. An empty string is `''`.
pub fn quote(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.is_empty() && !text.chars().any(needs_quote) {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('\'');
    for c in text.chars() {
        if c == '\'' {
            quoted.push('\'');
        }
        quoted.push(c);
    }
    quoted.push('\'');
    std::borrow::Cow::Owned(quoted)
}
