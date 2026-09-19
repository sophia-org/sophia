//! Which font a name resolves to, and what it costs to keep it.
//!
//! The catalog is an ordered list of path elements. Every element but the last
//! is a host directory that published an index; the last is always the
//! built-in element, so a resolution can always end somewhere and a session
//! with no font directories still serves text.
//!
//! Resolution follows the X server's order within each element: an exact
//! published name, then an alias, then a wildcard match. A name that resolves
//! nowhere is `BadName`, exactly as before this file existed.
//!
//! Four safeguards make a host path safe to expose:
//!
//! 1. The path is session configuration. `SetFontPath` is a client request and
//!    is refused, so no client can point this anywhere.
//! 2. A client's string is matched against an index and never joined onto a
//!    path, so no spelling reaches a file the directory did not publish.
//! 3. Reads follow no symbolic links and accept only regular files, bounded in
//!    size, parsed without a single unchecked index.
//! 4. Loaded faces live in a cache bounded by both count and bytes, so a client
//!    opening many fonts costs a fixed ceiling rather than growing memory.

mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::builtin;
use super::directory::XFontDirectory;
use super::pcf::{self, XLoadedFont};
use super::xlfd;
use std::sync::Arc;

/// A face, shared by every resource that opened it.
pub type XFontHandle = Arc<XLoadedFont>;

/// The most faces the catalog keeps loaded at once.
pub const X_FONT_CACHE_MAX_FONTS: usize = 64;

/// The most glyph and metric bytes the catalog keeps loaded at once.
pub const X_FONT_CACHE_MAX_BYTES: usize = 16 * 1024 * 1024;

/// The largest matrix a face may declare before it is refused.
///
/// `QueryFont` must send one entry per matrix cell whenever a face's ink
/// bounds differ, so a face's matrix is also the size of a reply this
/// authority will be asked to build. A full two-byte matrix is 65,536 cells
/// and 786 KB of reply, which is what a real server sends for the Unicode
/// 6x13; anything larger is malformed rather than large.
pub const X_FONT_MAX_MATRIX_CELLS: usize = 65_536;

/// The directories a session searches when it configures none.
///
/// The same set XLibre compiles in, filtered at startup to those that exist.
pub const X_DEFAULT_FONT_PATH: &[&str] = &[
    "/usr/share/fonts/X11/misc",
    "/usr/share/fonts/misc",
    "/usr/share/fonts/X11/75dpi",
    "/usr/share/fonts/X11/100dpi",
];

/// Why a name did not become a face.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XFontOpenError {
    /// No element publishes this name.
    NotFound,
    /// A file was published but could not be read as a font.
    Unreadable,
}

/// What happened when a face was opened, for the evidence record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum XFontOpenSource {
    Builtin,
    Host,
    Cached,
}

#[derive(Clone, Debug, Default)]
struct CacheEntry {
    font: XFontHandle,
    bytes: usize,
    /// Monotonic use counter; the least recently used entry is evicted first.
    used: u64,
}

/// The ordered font path and the faces loaded from it.
#[derive(Clone, Debug, Default)]
pub struct XFontCatalog {
    directories: Vec<XFontDirectory>,
    cache: BTreeMap<String, CacheEntry>,
    cached_bytes: usize,
    clock: u64,
    opened: u64,
    refused: u64,
    evicted: u64,
}

impl XFontCatalog {
    /// Index the configured path, skipping every element that cannot be read.
    ///
    /// A malformed or missing directory is not fatal: the element is skipped
    /// and the built-in element still answers, so a session never fails to
    /// start over a font directory.
    pub fn index(path: &[PathBuf]) -> Self {
        let directories = path
            .iter()
            .filter_map(|root| match XFontDirectory::index(root) {
                Ok(directory) => Some(directory),
                Err(error) => {
                    tracing::info!(
                        directory = %root.display(),
                        ?error,
                        "sophia_x11_font schema=1 status=skipped source=host"
                    );
                    None
                }
            })
            .collect();
        Self {
            directories,
            ..Self::default()
        }
    }

    /// The default path, filtered to the directories that exist.
    pub fn default_path() -> Vec<PathBuf> {
        X_DEFAULT_FONT_PATH
            .iter()
            .map(PathBuf::from)
            .filter(|root| root.join("fonts.dir").is_file())
            .collect()
    }

    /// A catalog with no host directories: the built-in element alone.
    pub fn builtin_only() -> Self {
        Self::default()
    }

    pub fn path(&self) -> impl Iterator<Item = &Path> {
        self.directories.iter().map(XFontDirectory::root)
    }

    pub const fn opened(&self) -> u64 {
        self.opened
    }

    pub const fn refused(&self) -> u64 {
        self.refused
    }

    /// Resolve and load a face by name.
    pub fn open(&mut self, name: &str) -> Result<(XFontHandle, XFontOpenSource), XFontOpenError> {
        if !xlfd::name_is_well_formed(name) {
            self.refused = self.refused.saturating_add(1);
            return Err(XFontOpenError::NotFound);
        }
        let key = name.to_lowercase();
        if let Some(entry) = self.cache.get_mut(&key) {
            self.clock = self.clock.saturating_add(1);
            entry.used = self.clock;
            return Ok((Arc::clone(&entry.font), XFontOpenSource::Cached));
        }
        let (font, source) = self.load(name).inspect_err(|_| {
            self.refused = self.refused.saturating_add(1);
        })?;
        let bytes = font.retained_bytes();
        let handle = Arc::new(font);
        self.clock = self.clock.saturating_add(1);
        self.cache.insert(
            key,
            CacheEntry {
                font: Arc::clone(&handle),
                bytes,
                used: self.clock,
            },
        );
        self.cached_bytes = self.cached_bytes.saturating_add(bytes);
        self.opened = self.opened.saturating_add(1);
        self.evict_to_bounds();
        tracing::debug!(
            glyphs = handle.metrics.char_infos.len(),
            bytes,
            cache_fonts = self.cache.len(),
            cache_bytes = self.cached_bytes,
            "sophia_x11_font schema=1 status=opened"
        );
        Ok((handle, source))
    }

    /// Resolve a name to a face without consulting the cache.
    fn load(&self, name: &str) -> Result<(XLoadedFont, XFontOpenSource), XFontOpenError> {
        for directory in &self.directories {
            let Some(resolved) = resolve_in(directory, name) else {
                continue;
            };
            let Some(path) = directory.file_for(&resolved) else {
                continue;
            };
            return match read_font_file(&path) {
                Some(font) if font.metrics.matrix_len() <= X_FONT_MAX_MATRIX_CELLS => {
                    Ok((font, XFontOpenSource::Host))
                }
                Some(_) => Err(XFontOpenError::Unreadable),
                None => Err(XFontOpenError::Unreadable),
            };
        }
        if builtin::provides(name) {
            return Ok((builtin::fixed_6x13(), XFontOpenSource::Builtin));
        }
        // A pattern that no host directory matched may still name the built-in
        // face, which is how `-misc-fixed-*` resolves on a bare session.
        if xlfd::is_pattern(name)
            && builtin::X_BUILTIN_FONT_NAMES
                .iter()
                .any(|candidate| xlfd::pattern_matches(name, candidate))
        {
            return Ok((builtin::fixed_6x13(), XFontOpenSource::Builtin));
        }
        Err(XFontOpenError::NotFound)
    }

    /// Names matching a pattern, across the whole path, bounded by `max_names`.
    pub fn list(&self, pattern: &str, max_names: usize) -> Vec<String> {
        if max_names == 0 || !xlfd::name_is_well_formed(pattern) {
            return Vec::new();
        }
        let mut names = Vec::new();
        for directory in &self.directories {
            for candidate in directory.matching(pattern) {
                if names.len() >= max_names {
                    return names;
                }
                let candidate = candidate.to_owned();
                if !names.contains(&candidate) {
                    names.push(candidate);
                }
            }
        }
        for candidate in builtin::X_BUILTIN_FONT_NAMES {
            if names.len() >= max_names {
                break;
            }
            if xlfd::pattern_matches(pattern, candidate)
                && !names
                    .iter()
                    .any(|name| name.eq_ignore_ascii_case(candidate))
            {
                names.push((*candidate).to_owned());
            }
        }
        names
    }

    /// Drop least recently used faces until both bounds hold.
    fn evict_to_bounds(&mut self) {
        while self.cache.len() > X_FONT_CACHE_MAX_FONTS
            || self.cached_bytes > X_FONT_CACHE_MAX_BYTES
        {
            let Some(oldest) = self
                .cache
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(entry) = self.cache.remove(&oldest) {
                self.cached_bytes = self.cached_bytes.saturating_sub(entry.bytes);
                self.evicted = self.evicted.saturating_add(1);
            }
        }
    }

    pub const fn cached_bytes(&self) -> usize {
        self.cached_bytes
    }

    pub fn cached_fonts(&self) -> usize {
        self.cache.len()
    }

    pub const fn evicted(&self) -> u64 {
        self.evicted
    }
}

/// Exact name, then alias chain, then wildcard, within one element.
fn resolve_in(directory: &XFontDirectory, name: &str) -> Option<String> {
    if directory.file_for(name).is_some() {
        return Some(name.to_owned());
    }
    let mut current = name.to_owned();
    for _ in 0..super::directory::X_FONT_ALIAS_MAX_HOPS {
        let Some(next) = directory.alias(&current) else {
            break;
        };
        let next = next.to_owned();
        if directory.file_for(&next).is_some() {
            return Some(next);
        }
        if next.eq_ignore_ascii_case(&current) {
            break;
        }
        current = next;
    }
    if xlfd::is_pattern(name) {
        // A pattern picks the first published name it matches, which is the
        // directory's own order.
        return directory
            .matching(name)
            .find(|candidate| directory.file_for(candidate).is_some())
            .map(str::to_owned);
    }
    None
}

/// Read a font file, decompressing it when it is gzipped.
fn read_font_file(path: &Path) -> Option<XLoadedFont> {
    let bytes = read_bounded(path)?;
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        inflate_gzip(&bytes)?
    } else {
        bytes
    };
    pcf::load(&bytes).ok()
}

/// Read a regular file, following no links, refusing anything oversized.
fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    use rustix::fs::{Mode, OFlags};
    let file = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .ok()?;
    let stat = rustix::fs::fstat(&file).ok()?;
    if stat.st_mode & 0o170_000 != 0o100_000 {
        return None;
    }
    let len = usize::try_from(stat.st_size).ok()?;
    if len > pcf::X_PCF_MAX_BYTES {
        return None;
    }
    let mut out = vec![0u8; len];
    let mut filled = 0usize;
    while filled < len {
        match rustix::io::read(&file, &mut out[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(rustix::io::Errno::INTR) => {}
            Err(_) => return None,
        }
    }
    out.truncate(filled);
    Some(out)
}

/// Decompress a gzip member, bounded by the reader's own ceiling.
///
/// The host ships every core font gzipped, so this is the ordinary path. The
/// bound is what stops a small file claiming an enormous expansion.
fn inflate_gzip(bytes: &[u8]) -> Option<Vec<u8>> {
    // Header: magic, method, flags, four-byte time, extra flags, OS.
    let flags = *bytes.get(3)?;
    let mut at = 10usize;
    if flags & 0b0000_0100 != 0 {
        let extra = usize::from(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]));
        at = at.checked_add(2)?.checked_add(extra)?;
    }
    for flag in [0b0000_1000u8, 0b0001_0000] {
        if flags & flag != 0 {
            at = bytes.get(at..)?.iter().position(|byte| *byte == 0)? + at + 1;
        }
    }
    if flags & 0b0000_0010 != 0 {
        at = at.checked_add(2)?;
    }
    let deflate = bytes.get(at..bytes.len().checked_sub(8)?)?;
    miniz_oxide::inflate::decompress_to_vec(deflate)
        .ok()
        .filter(|out| out.len() <= pcf::X_PCF_MAX_BYTES)
}
