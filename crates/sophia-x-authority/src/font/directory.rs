//! A font directory's index: `fonts.dir` and `fonts.alias`.
//!
//! Portions derived from yserver (MIT, Copyright (c) 2026 Jos Dehaes):
//! `crates/yserver/src/kms/core.rs:181-232`, the two file formats and the
//! quoted-alias rule.
//!
//! This is the safeguard that makes a host font path safe to expose to
//! clients. A client names a font; the name is looked up in an index this
//! module built; the index maps it to a file name the *directory* chose. A
//! client-supplied string is never joined onto a path, so no spelling of a
//! font name can name a file the directory did not publish.
//!
//! Two further bounds: the index is read once at startup with a cap on entries
//! and line length, and every open refuses to follow a symbolic link, so a
//! link planted in a font directory cannot redirect a read elsewhere.

mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::xlfd;

/// The most entries one directory may publish.
///
/// The host's widest directory holds a few thousand; this leaves room without
/// letting a directory of a million lines be read into memory.
pub const X_FONT_DIRECTORY_MAX_ENTRIES: usize = 8192;

/// The longest line either index file may carry.
pub const X_FONT_DIRECTORY_MAX_LINE: usize = 1024;

/// How many aliases may chain before the chain is called a loop.
///
/// The X server uses twenty (`dix/dixfonts.c`), and so does this.
pub const X_FONT_ALIAS_MAX_HOPS: usize = 20;

/// One indexed directory.
#[derive(Clone, Debug, Default)]
pub struct XFontDirectory {
    root: PathBuf,
    /// Published font name, lowercased, to what the directory published for it.
    names: BTreeMap<String, XFontDirectoryEntry>,
    /// Alias, lowercased, to the name it stands for.
    aliases: BTreeMap<String, String>,
}

/// One published font: the name as the directory spelled it and the file it
/// chose for it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XFontDirectoryEntry {
    pub name: String,
    pub file: String,
}

/// Why a directory could not be indexed. None of these is fatal to the
/// session: the element is skipped and the next one is tried.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XFontDirectoryError {
    Unreadable,
    Malformed,
}

impl XFontDirectory {
    /// Index a directory by reading its `fonts.dir` and optional `fonts.alias`.
    pub fn index(root: &Path) -> Result<Self, XFontDirectoryError> {
        let names = parse_index(&read_index(&root.join("fonts.dir"))?);
        // A directory with no aliases is ordinary; one whose alias file cannot
        // be read still serves its own fonts.
        let aliases = read_index(&root.join("fonts.alias"))
            .map(|text| parse_aliases(&text))
            .unwrap_or_default();
        Ok(Self {
            root: root.to_path_buf(),
            names,
            aliases,
        })
    }

    /// Build an index directly, for tests and for the built-in element.
    pub fn from_entries(
        root: &Path,
        names: BTreeMap<String, XFontDirectoryEntry>,
        aliases: BTreeMap<String, String>,
    ) -> Self {
        Self {
            root: root.to_path_buf(),
            names,
            aliases,
        }
    }

    /// The file this directory publishes for an exact name, if any.
    ///
    /// The returned path is built from the directory's own root and the file
    /// name the index carried, never from the caller's string.
    pub fn file_for(&self, name: &str) -> Option<PathBuf> {
        let entry = self.names.get(&name.to_lowercase())?;
        Some(self.root.join(&entry.file))
    }

    /// What an alias stands for, one hop.
    pub fn alias(&self, name: &str) -> Option<&str> {
        self.aliases.get(&name.to_lowercase()).map(String::as_str)
    }

    /// Every name this directory answers to, aliases included.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names
            .values()
            .map(|entry| entry.name.as_str())
            .chain(self.aliases.keys().map(String::as_str))
    }

    /// Every name matching a pattern.
    pub fn matching<'a>(&'a self, pattern: &'a str) -> impl Iterator<Item = &'a str> {
        self.names()
            .filter(move |candidate| xlfd::pattern_matches(pattern, candidate))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Read one index file with the bounds this module promises.
fn read_index(path: &Path) -> Result<String, XFontDirectoryError> {
    let text = read_regular_file(path).map_err(|()| XFontDirectoryError::Unreadable)?;
    String::from_utf8(text).map_err(|_| XFontDirectoryError::Malformed)
}

/// Open and read a file that must be a regular file and must not be a symlink.
fn read_regular_file(path: &Path) -> Result<Vec<u8>, ()> {
    use rustix::fs::{Mode, OFlags};
    let file = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| ())?;
    let stat = rustix::fs::fstat(&file).map_err(|_| ())?;
    if stat.st_mode & libc_file_type_mask() != libc_regular_file() {
        return Err(());
    }
    let len = usize::try_from(stat.st_size).map_err(|_| ())?;
    if len > X_FONT_DIRECTORY_MAX_ENTRIES * X_FONT_DIRECTORY_MAX_LINE {
        return Err(());
    }
    let mut out = vec![0u8; len];
    let mut filled = 0usize;
    while filled < len {
        match rustix::io::read(&file, &mut out[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return Err(()),
        }
    }
    out.truncate(filled);
    Ok(out)
}

const fn libc_file_type_mask() -> u32 {
    0o170_000
}

const fn libc_regular_file() -> u32 {
    0o100_000
}

/// `fonts.dir`: a count line, then `file name` pairs split at the first space.
fn parse_index(text: &str) -> BTreeMap<String, XFontDirectoryEntry> {
    let mut names = BTreeMap::new();
    for line in text.lines().skip(1).take(X_FONT_DIRECTORY_MAX_ENTRIES) {
        if line.len() > X_FONT_DIRECTORY_MAX_LINE {
            continue;
        }
        let Some((file, name)) = line.split_once(' ') else {
            continue;
        };
        let name = name.trim();
        if file.is_empty() || !xlfd::name_is_well_formed(name) || !file_name_is_safe(file) {
            continue;
        }
        names.insert(
            name.to_lowercase(),
            XFontDirectoryEntry {
                name: name.to_owned(),
                file: file.to_owned(),
            },
        );
    }
    names
}

/// A published file name must be a plain name inside this directory.
///
/// The index is data from the filesystem rather than from a client, but it is
/// still data, and a `fonts.dir` naming `../../etc/shadow` must not be obeyed.
fn file_name_is_safe(file: &str) -> bool {
    !file.is_empty()
        && file.len() <= X_FONT_DIRECTORY_MAX_LINE
        && !file.contains('/')
        && file != "."
        && file != ".."
}

/// `fonts.alias`: `alias name` pairs, `!` comments, either side optionally
/// double quoted so a name may contain spaces.
fn parse_aliases(text: &str) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    for line in text.lines().take(X_FONT_DIRECTORY_MAX_ENTRIES) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('!') || line.len() > X_FONT_DIRECTORY_MAX_LINE {
            continue;
        }
        let Some((alias, rest)) = split_token(line) else {
            continue;
        };
        let Some((target, _)) = split_token(rest.trim_start()) else {
            continue;
        };
        if !xlfd::name_is_well_formed(&alias) || !xlfd::name_is_well_formed(&target) {
            continue;
        }
        aliases.insert(alias.to_lowercase(), target);
    }
    aliases
}

/// Take one token, honouring double quotes, and return it with the remainder.
fn split_token(text: &str) -> Option<(String, &str)> {
    if let Some(rest) = text.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((rest[..end].to_owned(), &rest[end + 1..]));
    }
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    if end == 0 {
        return None;
    }
    Some((text[..end].to_owned(), &text[end..]))
}
