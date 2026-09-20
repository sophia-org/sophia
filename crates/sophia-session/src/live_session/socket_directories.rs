//! The socket-directory layout a client group reaches its listener through.
//!
//! One runtime root per display, `$XDG_RUNTIME_DIR/sophia/display-<n>/`, and
//! under it one directory per client group -- `shared/` for the trusted
//! listener and `confined-<k>/` for each confined group -- each owner-only
//! and each holding exactly one entry, the socket `X<n>`. The contract a
//! sandbox keeps with this layout is written in `docs/namespaces-and-portals.md`
//! under *Socket Directories*; this module is the session's half of it.
//!
//! THE SESSION OWNS THE LAYOUT AND NOTHING ELSE. What mounts a group directory
//! into a sandbox is the sandbox's, and this module refuses to know its name.
//! What it enforces is what makes the contract worth mounting: a group
//! directory that holds only its own socket, with no symbolic link in it that
//! could lead back out to the trusted path.

use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

/// Why a layout could not be prepared or a group could not be admitted.
///
/// Each names the boundary that failed, as the style guide asks; none is a
/// wrapped string.
#[derive(Debug)]
pub enum SocketDirectoryError {
    /// `XDG_RUNTIME_DIR` was not given. The layout has no home without it and
    /// invents none: a root under `/tmp` would be world-listable and would
    /// turn path exclusion into a suggestion.
    RuntimeDirUnset,
    /// The runtime directory was given as a relative path. A relative root
    /// would be resolved against whatever the current directory happened to be
    /// at each call, and the sandbox and the session would not agree on it.
    RuntimeDirRelative(PathBuf),
    /// A directory in the layout could not be made.
    Unmakeable {
        path: PathBuf,
        cause: std::io::Error,
    },
    /// A path in the layout existed already and was not a directory this
    /// session may use: a symbolic link, a file, or a directory somebody other
    /// than the owner can enter.
    ///
    /// REFUSED RATHER THAN REPAIRED. Loosening a mode or replacing a link
    /// found in place would be acting on a path an attacker may have prepared,
    /// and the session cannot tell the two apart. A layout it did not make is
    /// not a layout it may vouch for.
    Unusable {
        path: PathBuf,
        because: SocketDirectoryFault,
    },
    /// A group directory could not be read to confirm what it holds.
    Unreadable {
        path: PathBuf,
        cause: std::io::Error,
    },
    /// A group directory held something other than its own socket. The
    /// contract is that a sandbox may mount it whole; an extra entry is an
    /// extra thing mounted, and a link is a way back out.
    Occupied {
        path: PathBuf,
        entry: std::ffi::OsString,
    },
}

/// What was wrong with a path found already in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketDirectoryFault {
    /// Not a directory at all.
    NotADirectory,
    /// A symbolic link. Never followed: a link here is the escape the layout
    /// exists to prevent, and following it to ask what it points at would be
    /// the first half of taking it.
    SymbolicLink,
    /// A directory somebody other than its owner may enter or list. Group and
    /// other bits are both refused: the socket inside is reachable by anyone
    /// who can enter, and the layout's whole claim is that nobody else can.
    Reachable(u32),
    /// A directory this user does not own. Owner-only is worth nothing when
    /// the owner is somebody else: the mode says only its owner may enter,
    /// and that owner is not us. `live_xauthority_directory` asks the same of
    /// the runtime directory it puts a cookie in, for the same reason.
    Foreign(u32),
}

impl core::fmt::Display for SocketDirectoryError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RuntimeDirUnset => {
                formatter.write_str("XDG_RUNTIME_DIR is unset; the socket layout has no root")
            }
            Self::RuntimeDirRelative(path) => write!(
                formatter,
                "XDG_RUNTIME_DIR must be absolute; found {}",
                path.display()
            ),
            Self::Unmakeable { path, cause } => {
                write!(formatter, "could not make {}: {cause}", path.display())
            }
            Self::Unusable { path, because } => match because {
                SocketDirectoryFault::NotADirectory => {
                    write!(formatter, "{} is not a directory", path.display())
                }
                SocketDirectoryFault::SymbolicLink => {
                    write!(formatter, "{} is a symbolic link", path.display())
                }
                SocketDirectoryFault::Reachable(mode) => write!(
                    formatter,
                    "{} is reachable beyond its owner (mode {mode:04o})",
                    path.display()
                ),
                SocketDirectoryFault::Foreign(uid) => write!(
                    formatter,
                    "{} is owned by uid {uid}, not by this session",
                    path.display()
                ),
            },
            Self::Unreadable { path, cause } => {
                write!(formatter, "could not read {}: {cause}", path.display())
            }
            Self::Occupied { path, entry } => write!(
                formatter,
                "{} holds {:?}, which is not its socket",
                path.display(),
                entry
            ),
        }
    }
}

impl std::error::Error for SocketDirectoryError {}

/// Which client group a directory belongs to.
///
/// NAMED, NOT NUMBERED FROM A COUNTER. The name is what a launcher is given
/// and what appears in the layout, so it has to be derivable from the group
/// rather than from the order groups happened to be created in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientGroup {
    /// The trusted listener, for classic shared-X clients.
    Shared,
    /// One confined group. The index is the group's, not a connection's.
    ///
    /// Constructed by the controls today, and by the per-group listeners when
    /// the multiplexer half of the plan lands; this session runs one group,
    /// so nothing in it names a second yet.
    #[cfg_attr(not(test), allow(dead_code))]
    Confined(u32),
}

impl ClientGroup {
    fn directory_name(self) -> String {
        match self {
            Self::Shared => "shared".to_owned(),
            Self::Confined(index) => format!("confined-{index}"),
        }
    }
}

/// One display's socket-directory layout.
///
/// Holds the root and the display number, which is all that is needed to name
/// any group's directory and socket. It does not hold the groups: which groups
/// exist is the session's policy, and this is the layout that policy is
/// written into.
#[derive(Debug, Clone)]
pub struct SocketDirectoryLayout {
    root: PathBuf,
    display_number: u32,
}

/// Owner-only, and the mode every directory in the layout is both made with
/// and checked against.
const OWNER_ONLY: u32 = 0o700;

impl SocketDirectoryLayout {
    /// Prepare the runtime root for one display.
    ///
    /// BEFORE ANY LISTENER BINDS, which is the only time it can be done
    /// honestly: a directory made after a socket is already accepting
    /// connections has not excluded anything from the clients already inside.
    pub fn prepare(display_number: u32) -> Result<Self, SocketDirectoryError> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .ok_or(SocketDirectoryError::RuntimeDirUnset)?;
        if !runtime.is_absolute() {
            return Err(SocketDirectoryError::RuntimeDirRelative(runtime));
        }
        let root = runtime
            .join("sophia")
            .join(format!("display-{display_number}"));
        // The intermediate `sophia/` is made with the same mode as the rest:
        // an owner-only leaf under a world-enterable parent is still
        // owner-only, but the parent's listing would name every display this
        // user is running, and the layout's claim is about paths rather than
        // about what is interesting.
        make_owner_only(&runtime.join("sophia"))?;
        make_owner_only(&root)?;
        Ok(Self {
            root,
            display_number,
        })
    }

    /// Adopt a root without making it, for a caller that has one already.
    ///
    /// Read by the controls, which put a layout under a directory of their own
    /// rather than under the user's real runtime directory. The session uses
    /// `prepare`, which finds the root itself.
    ///
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn rooted_at(root: PathBuf, display_number: u32) -> Result<Self, SocketDirectoryError> {
        if !root.is_absolute() {
            return Err(SocketDirectoryError::RuntimeDirRelative(root));
        }
        make_owner_only(&root)?;
        Ok(Self {
            root,
            display_number,
        })
    }

    /// Where this group's directory is.
    pub fn group_directory(&self, group: ClientGroup) -> PathBuf {
        self.root.join(group.directory_name())
    }

    /// Where this group's socket is.
    ///
    /// THE SAME NAME IN EVERY GROUP. A client whose group directory is mounted
    /// at the standard path sees `X<n>` and asks for `:<n>`, so nothing about
    /// a launch has to change when it is confined. The directory is what
    /// differs, and the directory is the part a client cannot name.
    pub fn socket_path(&self, group: ClientGroup) -> PathBuf {
        self.group_directory(group)
            .join(format!("X{}", self.display_number))
    }

    /// Make this group's directory, owner-only, and answer where its socket
    /// goes.
    pub fn prepare_group(&self, group: ClientGroup) -> Result<PathBuf, SocketDirectoryError> {
        make_owner_only(&self.group_directory(group))?;
        Ok(self.socket_path(group))
    }

    /// Confirm this group's directory is one a sandbox may mount whole.
    ///
    /// THE CHECK THE CONTRACT RESTS ON. A launcher mounts the directory, not
    /// the socket, so whatever else is in there is mounted too: one stray
    /// entry is one more thing inside the sandbox, and one symbolic link is a
    /// way back out to the trusted path the mount exists to exclude. Asked
    /// after the listener has bound, when the socket is there to be counted.
    ///
    /// `symlink_metadata` throughout: a link is refused, never followed, so
    /// nothing here is decided by what it points at.
    pub fn verify_group(&self, group: ClientGroup) -> Result<(), SocketDirectoryError> {
        let directory = self.group_directory(group);
        check_owner_only(&directory)?;
        let socket = self.socket_path(group);
        let expected = socket.file_name().unwrap_or_default();
        let entries =
            std::fs::read_dir(&directory).map_err(|cause| SocketDirectoryError::Unreadable {
                path: directory.clone(),
                cause,
            })?;
        for entry in entries {
            let entry = entry.map_err(|cause| SocketDirectoryError::Unreadable {
                path: directory.clone(),
                cause,
            })?;
            if entry.file_name() != expected {
                return Err(SocketDirectoryError::Occupied {
                    path: directory,
                    entry: entry.file_name(),
                });
            }
            // The socket's own name is right; its kind still has to be. A
            // symbolic link named `X<n>` is the escape wearing the one name
            // this directory is allowed to hold.
            let metadata = std::fs::symlink_metadata(entry.path()).map_err(|cause| {
                SocketDirectoryError::Unreadable {
                    path: entry.path(),
                    cause,
                }
            })?;
            if metadata.file_type().is_symlink() {
                return Err(SocketDirectoryError::Unusable {
                    path: entry.path(),
                    because: SocketDirectoryFault::SymbolicLink,
                });
            }
        }
        Ok(())
    }
}

/// Make one directory owner-only, or confirm the one already there is.
fn make_owner_only(path: &Path) -> Result<(), SocketDirectoryError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => return check_owner_only(path),
        Err(cause) if cause.kind() != std::io::ErrorKind::NotFound => {
            return Err(SocketDirectoryError::Unmakeable {
                path: path.to_path_buf(),
                cause,
            });
        }
        Err(_) => {}
    }
    // MADE WITH THE MODE, not made and then chmodded. Between a permissive
    // create and a later tightening there is an interval in which the
    // directory is enterable, and a socket bound in that interval is reachable
    // by whoever was watching for it.
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(OWNER_ONLY)
        .create(path)
        .map_err(|cause| SocketDirectoryError::Unmakeable {
            path: path.to_path_buf(),
            cause,
        })?;
    check_owner_only(path)
}

/// Confirm a path is a directory nobody but its owner may enter.
fn check_owner_only(path: &Path) -> Result<(), SocketDirectoryError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|cause| SocketDirectoryError::Unmakeable {
            path: path.to_path_buf(),
            cause,
        })?;
    let kind = metadata.file_type();
    if kind.is_symlink() {
        return Err(SocketDirectoryError::Unusable {
            path: path.to_path_buf(),
            because: SocketDirectoryFault::SymbolicLink,
        });
    }
    if !kind.is_dir() {
        return Err(SocketDirectoryError::Unusable {
            path: path.to_path_buf(),
            because: SocketDirectoryFault::NotADirectory,
        });
    }
    // `recursive(true)` applies the mode only to what it creates, and a
    // directory already in place keeps whatever mode it had -- which is the
    // case this exists for.
    let mode = metadata.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        return Err(SocketDirectoryError::Unusable {
            path: path.to_path_buf(),
            because: SocketDirectoryFault::Reachable(mode),
        });
    }
    // AND OURS. Owner-only is worth nothing when the owner is somebody else:
    // the mode then says only *they* may enter. A directory prepared in
    // advance at a path we were going to use is exactly the shape this
    // catches.
    let owner = metadata.uid();
    if owner != rustix::process::geteuid().as_raw() {
        return Err(SocketDirectoryError::Unusable {
            path: path.to_path_buf(),
            because: SocketDirectoryFault::Foreign(owner),
        });
    }
    Ok(())
}
