use std::collections::BTreeSet;
#[cfg(any(
    feature = "drm-hotplug",
    all(feature = "seat-control", feature = "libdrm-events")
))]
use std::ffi::OsStr;
use std::io;

/// A session's immutable restriction on its seat inventory. This never grants
/// seat membership or matches renumberable card/render node names.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LiveGpuAdmission {
    excluded: BTreeSet<String>,
}

impl LiveGpuAdmission {
    pub fn new(excluded: impl IntoIterator<Item = String>) -> io::Result<Self> {
        let mut policy = Self::default();
        for identity in excluded {
            if policy.excluded.len() == 16
                || !valid_identity(&identity)
                || !policy.excluded.insert(identity)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "GPU exclusions require at most sixteen unique stable ID_PATH values",
                ));
            }
        }
        Ok(policy)
    }

    #[cfg(any(
        feature = "drm-hotplug",
        all(feature = "seat-control", feature = "libdrm-events")
    ))]
    pub(crate) fn admits(&self, identity: Option<&OsStr>) -> io::Result<bool> {
        let identity = identity
            .and_then(OsStr::to_str)
            .filter(|identity| valid_identity(identity));
        match identity {
            Some(identity) => Ok(!self.excluded.contains(identity)),
            None if self.excluded.is_empty() => Ok(true),
            None => Err(io::Error::other(
                "GPU admission cannot establish a stable ID_PATH",
            )),
        }
    }
}

pub(crate) fn valid_identity(identity: &str) -> bool {
    !identity.is_empty()
        && identity.len() <= 256
        && identity
            .split_once('-')
            .is_some_and(|(kind, path)| !kind.is_empty() && !path.is_empty())
        && identity.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'+')
        })
}
