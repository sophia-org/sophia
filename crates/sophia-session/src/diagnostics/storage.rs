//! Private host storage. No application or policy-client capability is exposed.
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};

use rustix::fs::{Mode, OFlags, open, openat};

pub(super) fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub(super) fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        && value != "."
        && value != ".."
}

pub(super) struct Directory {
    pub path: PathBuf,
    fd: File,
}

impl Directory {
    pub fn open(path: &Path, create: bool) -> io::Result<Self> {
        if create {
            match fs::DirBuilder::new().mode(0o700).create(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        let fd = File::from(open(
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let metadata = fd.metadata()?;
        if metadata.uid() != rustix::process::getuid().as_raw() || metadata.mode() & 0o077 != 0 {
            return Err(invalid(
                "diagnostic directory must be owned by this user with mode 0700",
            ));
        }
        Ok(Self {
            path: path.to_owned(),
            fd,
        })
    }

    pub fn child(&self, name: &str, create: bool) -> io::Result<Self> {
        if !token(name) {
            return Err(invalid("invalid diagnostic identifier"));
        }
        Self::open(&self.path.join(name), create)
    }

    pub fn file(&self, name: &str, flags: OFlags) -> io::Result<File> {
        if !token(name) {
            return Err(invalid("invalid diagnostic filename"));
        }
        let file = File::from(openat(
            &self.fd,
            name,
            flags | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::RUSR | Mode::WUSR,
        )?);
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::getuid().as_raw()
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            return Err(invalid(
                "unsafe diagnostic file ownership, permissions, or links",
            ));
        }
        Ok(file)
    }

    pub fn lock(&self) -> io::Result<File> {
        let file = self.file("lock", OFlags::RDWR | OFlags::CREATE)?;
        file.lock()?;
        Ok(file)
    }

    pub fn read(&self, name: &str, limit: u64) -> io::Result<String> {
        String::from_utf8(self.read_bytes(name, limit)?)
            .map_err(|_| invalid("diagnostic record is not UTF-8"))
    }

    pub fn read_bytes(&self, name: &str, limit: u64) -> io::Result<Vec<u8>> {
        let file = self.file(name, OFlags::RDONLY)?;
        if file.metadata()?.len() > limit {
            return Err(invalid("diagnostic record exceeds its bound"));
        }
        let mut content = Vec::new();
        file.take(limit + 1).read_to_end(&mut content)?;
        if content.len() as u64 > limit {
            return Err(invalid("diagnostic record exceeds its bound"));
        }
        Ok(content)
    }

    pub fn replace(&self, name: &str, value: &str) -> io::Result<()> {
        self.replace_bytes(name, value.as_bytes())
    }

    pub fn replace_bytes(&self, name: &str, value: &[u8]) -> io::Result<()> {
        // All callers hold the directory's lock. Never truncate an unchecked file.
        let temporary = format!("{name}.new");
        let mut file = self.file(&temporary, OFlags::WRONLY | OFlags::CREATE)?;
        file.set_len(0)?;
        file.write_all(value)?;
        file.sync_all()?;
        rustix::fs::renameat(&self.fd, temporary.as_str(), &self.fd, name)?;
        self.fd.sync_all()
    }

    pub fn append(&self, name: &str, value: &str, limit: u64) -> io::Result<()> {
        let mut file = self.file(name, OFlags::WRONLY | OFlags::CREATE | OFlags::APPEND)?;
        if file.metadata()?.len().saturating_add(value.len() as u64) > limit {
            return Err(io::Error::other("diagnostic record capacity exhausted"));
        }
        file.write_all(value.as_bytes())?;
        Ok(())
    }

    pub fn sync(&self, name: &str) -> io::Result<()> {
        self.file(name, OFlags::RDONLY)?.sync_all()
    }
}

pub(super) fn field<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
}

pub(super) fn names(directory: &Directory) -> io::Result<Vec<String>> {
    let mut result = Vec::new();
    for entry in fs::read_dir(&directory.path)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if token(&name) && entry.file_type()?.is_dir() {
            result.push(name);
        }
    }
    result.sort();
    Ok(result)
}

pub(super) fn bytes(directory: &Directory) -> io::Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(&directory.path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != rustix::process::getuid().as_raw()
        {
            return Err(invalid("unexpected entry in diagnostic session"));
        }
        total = total.saturating_add(metadata.len());
    }
    Ok(total)
}
