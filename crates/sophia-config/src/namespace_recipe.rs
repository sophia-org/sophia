//! Pinned recipe loading. Walk beneath one directory descriptor, rejecting
//! symlinks at every component; no source path comes from an application.
use rustix::fs::{Mode, OFlags, openat};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const MAX_BYTES: usize = 65536;
const MAX_FILES: usize = 33; // Top level plus 32 distinct include files.
const MAX_DEPTH: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
struct FileIdentity {
    device: u64,
    inode: u64,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
fn identity(file: &File) -> io::Result<FileIdentity> {
    let m = file.metadata()?;
    if !m.is_file() {
        return Err(refused("recipe source is not a regular file"));
    }
    Ok(FileIdentity {
        device: m.dev(),
        inode: m.ino(),
        size: m.len(),
        modified: (m.mtime(), m.mtime_nsec()),
        changed: (m.ctime(), m.ctime_nsec()),
    })
}
fn refused(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub struct PinnedNamespaceRecipe {
    root: File,
    root_path: PathBuf,
    entry: String,
    files: BTreeMap<String, String>,
    identities: BTreeMap<String, FileIdentity>,
    digest: [u8; 32],
    expanded_bytes: usize,
}
impl PinnedNamespaceRecipe {
    pub fn load(root: &Path, entry: &str) -> io::Result<Self> {
        if !root.is_absolute() {
            return Err(refused("recipe root must be absolute"));
        }
        let root_file = open_directory(root)?;
        let entry = relative(entry)?;
        let mut snapshot = Self {
            root: root_file,
            root_path: root.to_owned(),
            entry: entry.clone(),
            files: BTreeMap::new(),
            identities: BTreeMap::new(),
            digest: [0; 32],
            expanded_bytes: 0,
        };
        snapshot.load_file(&entry, 0, &mut BTreeSet::new())?;
        snapshot.verify()?;
        let mut digest = Sha256::new();
        digest.update(entry.as_bytes());
        digest.update([0]);
        for (name, bytes) in &snapshot.files {
            digest.update(name.as_bytes());
            digest.update([0]);
            digest.update((bytes.len() as u64).to_le_bytes());
            digest.update(bytes.as_bytes());
        }
        snapshot.digest = digest.finalize().into();
        Ok(snapshot)
    }
    pub fn source(&self) -> &str {
        &self.files[&self.entry]
    }
    /// Includes the entry itself so a recursive include is detected, not
    /// silently interpreted as a missing file. The compiler refuses headers
    /// inside fragments as well.
    pub fn files(&self) -> &BTreeMap<String, String> {
        &self.files
    }
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    /// Recheck immediately before candidate publication. Compilation uses
    /// these retained bytes throughout, never a later path read.
    pub fn verify(&self) -> io::Result<()> {
        let current = open_directory(&self.root_path)?;
        let a = current.metadata()?;
        let b = self.root.metadata()?;
        if (a.dev(), a.ino()) != (b.dev(), b.ino()) {
            return Err(refused("recipe root changed"));
        }
        for (name, bytes) in &self.files {
            let (observed, id) = read_source(&self.root, name)?;
            if observed != *bytes || self.identities.get(name) != Some(&id) {
                return Err(refused("pinned recipe source changed"));
            }
        }
        Ok(())
    }
    fn load_file(
        &mut self,
        name: &str,
        depth: usize,
        active: &mut BTreeSet<String>,
    ) -> io::Result<()> {
        if depth > MAX_DEPTH {
            return Err(refused("recipe include depth exceeds limit"));
        }
        if !active.insert(name.to_owned()) {
            return Err(refused("recipe include cycle"));
        }
        if !self.files.contains_key(name) {
            if self.files.len() == MAX_FILES {
                return Err(refused("recipe file count exceeds limit"));
            }
            let (source, id) = read_source(&self.root, name)?;
            if source.len() + self.files.values().map(String::len).sum::<usize>() > MAX_BYTES {
                return Err(refused("recipe source bytes exceed limit"));
            }
            self.files.insert(name.to_owned(), source);
            self.identities.insert(name.to_owned(), id);
        }
        self.expanded_bytes = self
            .expanded_bytes
            .checked_add(self.files[name].len())
            .ok_or_else(|| refused("recipe expansion exceeds limit"))?;
        if self.expanded_bytes > MAX_BYTES {
            return Err(refused("recipe expansion exceeds limit"));
        }
        // Only discover includes here. Full grammar and expanded byte/operation
        // accounting belong to the pure compiler, not a second parser.
        let includes = self.files[name]
            .lines()
            .filter_map(|line| {
                let words: Vec<_> = line
                    .trim_matches([' ', '\t'])
                    .split([' ', '\t'])
                    .filter(|s| !s.is_empty())
                    .collect();
                match words.as_slice() {
                    ["include", name] => Some(relative(name)),
                    _ => None,
                }
            })
            .collect::<io::Result<Vec<_>>>()?;
        for include in includes {
            self.load_file(&include, depth + 1, active)?;
        }
        active.remove(name);
        Ok(())
    }
}

fn relative(text: &str) -> io::Result<String> {
    if text.starts_with('/') || text.chars().any(char::is_control) {
        return Err(refused("recipe include escapes config root"));
    }
    let mut parts = Vec::new();
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts
                    .pop()
                    .ok_or_else(|| refused("recipe include escapes config root"))?;
            }
            _ if part.len() <= 255 => parts.push(part),
            _ => return Err(refused("recipe component exceeds limit")),
        }
    }
    let path = parts.join("/");
    if path.is_empty() || path.len() >= 1024 {
        return Err(refused("invalid recipe path"));
    }
    Ok(path)
}

fn open_directory(path: &Path) -> io::Result<File> {
    // Pin from / instead of canonicalizing: even an intermediate symlink is
    // refused and cannot swap between a check and the corresponding open.
    let mut current = File::open("/")?;
    for part in path.components() {
        match part {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(part) => {
                current = File::from(openat(
                    &current,
                    part,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                )?)
            }
            _ => return Err(refused("recipe root is not normalized")),
        }
    }
    Ok(current)
}

fn read_source(root: &File, path: &str) -> io::Result<(String, FileIdentity)> {
    let mut directory = root.try_clone()?;
    let mut parts = path.split('/').peekable();
    while let Some(part) = parts.next() {
        let mut flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
        if parts.peek().is_some() {
            flags |= OFlags::DIRECTORY;
        }
        let mut file = File::from(openat(&directory, part, flags, Mode::empty())?);
        if parts.peek().is_none() {
            let before = identity(&file)?;
            if before.size > MAX_BYTES as u64 {
                return Err(refused("recipe file exceeds limit"));
            }
            let mut bytes = Vec::new();
            (&mut file)
                .take(MAX_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > MAX_BYTES || identity(&file)? != before {
                return Err(refused("recipe changed while reading"));
            }
            let text = String::from_utf8(bytes).map_err(|_| refused("recipe is not UTF-8"))?;
            return Ok((text, before));
        }
        directory = file;
    }
    Err(refused("empty recipe source path"))
}
