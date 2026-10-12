use super::*;
use rustix::fs::{Mode, OFlags};
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::net::UnixStream;
use std::path::{Component, Path};

/// Pins the selected generation directory. No method changes working directory
/// or follows a replacement `current` name after construction.
pub struct Namespace {
    directory: File,
    discovery: Discovery,
}
const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

impl Namespace {
    pub fn from_env() -> io::Result<Self> {
        let path = std::env::var_os(DIRECTORY_ENV).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Session supplied no namespace directory",
            )
        })?;
        Self::open(Path::new(&path))
    }
    pub fn open(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(invalid("namespace directory must be absolute"));
        }
        let mut directory = File::from(rustix::fs::open("/", DIRECTORY, Mode::empty())?);
        for part in path.components() {
            match part {
                Component::RootDir => (),
                Component::Normal(name) => {
                    directory = File::from(rustix::fs::openat(
                        &directory,
                        name,
                        DIRECTORY,
                        Mode::empty(),
                    )?)
                }
                _ => return Err(invalid("namespace directory traversal")),
            }
        }
        let file = File::from(rustix::fs::openat(
            &directory,
            DISCOVERY_FILE,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )?);
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > MAX_BYTES as u64 {
            return Err(invalid("discovery must be a bounded regular file"));
        }
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        let discovery =
            Discovery::parse(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Self {
            directory,
            discovery,
        })
    }
    pub fn discovery(&self) -> &Discovery {
        &self.discovery
    }

    /// Nonblocking local connect. Availability is only descriptive; the caller
    /// decides retry policy and must authenticate the returned stream.
    pub fn connect(&self, name: &str) -> io::Result<UnixStream> {
        let endpoint = self.discovery.endpoint(name).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "endpoint absent from this view")
        })?;
        let mut directory = self.directory.try_clone()?;
        let mut parts = endpoint.path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                directory = File::from(rustix::fs::openat(
                    &directory,
                    part,
                    DIRECTORY,
                    Mode::empty(),
                )?);
                continue;
            }
            let node = File::from(rustix::fs::openat(
                &directory,
                part,
                OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )?);
            if !node.metadata()?.file_type().is_socket() {
                return Err(invalid("endpoint is not a socket"));
            }
            let address =
                rustix::net::SocketAddrUnix::new(format!("/proc/self/fd/{}", node.as_raw_fd()))?;
            let socket = rustix::net::socket_with(
                rustix::net::AddressFamily::UNIX,
                rustix::net::SocketType::STREAM,
                rustix::net::SocketFlags::CLOEXEC | rustix::net::SocketFlags::NONBLOCK,
                None,
            )?;
            rustix::net::connect(&socket, &address)?;
            return Ok(UnixStream::from(socket));
        }
        Err(invalid("empty endpoint"))
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
