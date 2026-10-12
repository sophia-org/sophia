use super::*;
use rustix::fs::{AtFlags, CWD, Mode, OFlags, linkat, open};
use std::os::unix::fs::FileTypeExt;

/// A Session-selected existing group socket. O_PATH pins the inode; publishing
/// another name never opens a new listener or re-resolves the original path.
pub struct CapturedX11Socket {
    service: ServiceReference,
    file: fs::File,
}
impl CapturedX11Socket {
    pub fn capture(service: ServiceReference, path: &Path) -> io::Result<Self> {
        if service.protocol != EndpointProtocol::X11
            || !service.is_valid()
            || !path.is_absolute()
            || fs::canonicalize(path)? != path
        {
            return Err(refused("invalid X11 service or socket path"));
        }
        let parent = fs::symlink_metadata(
            path.parent()
                .ok_or_else(|| refused("missing X11 socket parent"))?,
        )?;
        if !parent.is_dir()
            || parent.uid() != rustix::process::geteuid().as_raw()
            || parent.mode() & 0o077 != 0
        {
            return Err(refused(
                "X11 socket requires a private owned group directory",
            ));
        }
        let file = fs::File::from(open(
            path,
            OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let metadata = file.metadata()?;
        if !metadata.file_type().is_socket()
            || metadata.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(refused("X11 source is not an owned socket"));
        }
        Ok(Self { service, file })
    }
    pub fn service(&self) -> &ServiceReference {
        &self.service
    }

    /// Share the captured inode across launches without resolving its old name.
    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            service: self.service.clone(),
            file: self.file.try_clone()?,
        })
    }

    /// Check a conventional X path before launch without replacing this
    /// capture. Publication still uses the original inode, never the new one.
    pub fn matches_source(&self, path: &Path) -> io::Result<bool> {
        let current = Self::capture(self.service.clone(), path)?;
        let old = self.file.metadata()?;
        let new = current.file.metadata()?;
        Ok((old.dev(), old.ino()) == (new.dev(), new.ino()))
    }

    pub(super) fn materialize(&self, destination: &Path) -> io::Result<()> {
        // Linux permits linkat through an O_PATH fd's proc link without
        // AT_EMPTY_PATH's capability requirement. Cross-filesystem linking or
        // an already unlinked source refuses; neither gets a path fallback.
        linkat(
            CWD,
            format!("/proc/self/fd/{}", self.file.as_raw_fd()),
            CWD,
            destination,
            AtFlags::SYMLINK_FOLLOW,
        )?;
        let source = self.file.metadata()?;
        let target = fs::symlink_metadata(destination)?;
        if !target.file_type().is_socket()
            || (target.dev(), target.ino()) != (source.dev(), source.ino())
        {
            return Err(refused("published X11 alias changed socket identity"));
        }
        // Do not chmod: both paths name the frontend's same socket inode.
        Ok(())
    }
}
