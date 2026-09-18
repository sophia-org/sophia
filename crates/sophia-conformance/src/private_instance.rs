//! Conformance-host entry validation. A pathname or an `--inside` flag cannot
//! attest isolation: the launcher supplies open kernel namespace descriptors.
//!
//! Call once, before starting threads. Only inherited activation descriptors are
//! consumed; explicitly delegated capabilities remain owned by the caller.
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::{FileTypeExt, MetadataExt};

mod launch;
pub use launch::{Child, Launch, Mount};

pub const NAMESPACES: [&str; 6] = ["mnt", "net", "pid", "user", "ipc", "uts"];
pub const ENVIRONMENT: [(&str, &str); 5] = [
    ("PATH", "/usr/bin:/bin"),
    ("HOME", "/home/test"),
    ("LANG", "C.UTF-8"),
    ("PYTHONDONTWRITEBYTECODE", "1"),
    ("PWD", "/work"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    namespaces: BTreeMap<String, i32>,
    descriptors: BTreeMap<i32, [u64; 3]>,
}

/// Constructible only by validating the inherited kernel objects at entry.
#[derive(Debug)]
pub struct Activation {
    namespaces: BTreeMap<String, String>,
    delegated: BTreeSet<i32>,
}

impl Activation {
    pub fn namespaces(&self) -> &BTreeMap<String, String> {
        &self.namespaces
    }

    pub fn delegated(&self) -> &BTreeSet<i32> {
        &self.delegated
    }

    /// Transfer an explicitly delegated pipe into ordinary Rust ownership.
    pub fn take_pipe(&mut self, descriptor: i32) -> Result<File, String> {
        if !self.delegated.contains(&descriptor) {
            return Err("pipe was not delegated by the launcher".into());
        }
        let path = format!("/proc/self/fd/{descriptor}");
        if !std::fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .file_type()
            .is_fifo()
        {
            return Err("delegated control must be a pipe".into());
        }
        let file = File::open(path).map_err(|e| e.to_string())?;
        nix::unistd::close(descriptor).map_err(|e| e.to_string())?;
        self.delegated.remove(&descriptor);
        Ok(file)
    }
}

fn fd_identity(descriptor: i32) -> Result<[u64; 3], String> {
    let metadata =
        std::fs::metadata(format!("/proc/self/fd/{descriptor}")).map_err(|e| e.to_string())?;
    Ok([
        metadata.dev(),
        metadata.ino(),
        u64::from(metadata.mode() & 0o170000),
    ])
}

pub fn validate_entry(descriptor: i32) -> Result<Activation, String> {
    if descriptor < 3 {
        return Err("activation must be an inherited pipe above stderr".into());
    }
    let path = format!("/proc/self/fd/{descriptor}");
    let metadata = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    if !metadata.file_type().is_fifo() {
        return Err("activation is not a runner pipe".into());
    }
    let opened = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    let read = File::from(opened).take(4097).read_to_end(&mut bytes);
    // This raw descriptor came from exec, not an existing Rust owner. nix
    // provides the safe close API; no borrowed descriptor is made to own it.
    nix::unistd::close(descriptor).map_err(|e| e.to_string())?;
    read.map_err(|e| e.to_string())?;
    if bytes.len() > 4096 {
        return Err("oversized activation".into());
    }
    let record: Record = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    let expected = NAMESPACES.into_iter().collect::<BTreeSet<_>>();
    if record
        .namespaces
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != expected
        || record
            .namespaces
            .values()
            .any(|fd| *fd < 3 || *fd == descriptor)
        || record.namespaces.values().collect::<BTreeSet<_>>().len() != NAMESPACES.len()
        || record.descriptors.keys().any(|fd| {
            *fd < 3
                || *fd == descriptor
                || record.namespaces.values().any(|namespace| namespace == fd)
        })
    {
        return Err("invalid namespace or delegated descriptor inventory".into());
    }
    let checked = validate_namespaces(&record.namespaces);
    for fd in record.namespaces.values() {
        nix::unistd::close(*fd).map_err(|e| e.to_string())?;
    }
    let namespaces = checked?;
    let environment = std::env::vars_os().collect::<BTreeMap<_, _>>();
    let allowed = ENVIRONMENT
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
    if environment != allowed {
        return Err("ambient environment present".into());
    }
    if std::path::Path::new("/dev/dri").exists()
        || std::path::Path::new("/dev/input").exists()
        || File::open("/dev/tty").is_ok()
    {
        return Err("ambient devices or controlling terminal present".into());
    }
    for (&fd, identity) in &record.descriptors {
        if fd_identity(fd)? != *identity {
            return Err("delegated capability changed".into());
        }
    }
    let descriptors = std::fs::read_dir("/proc/self/fd")
        .map_err(|e| e.to_string())?
        .map(|entry| {
            entry
                .map_err(|e| e.to_string())?
                .file_name()
                .to_string_lossy()
                .parse::<i32>()
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    for fd in descriptors {
        let Ok(metadata) = std::fs::metadata(format!("/proc/self/fd/{fd}")) else {
            continue; // The directory iterator has closed its own descriptor.
        };
        if (fd > 2 && !record.descriptors.contains_key(&fd))
            || (fd <= 2 && metadata.file_type().is_socket())
        {
            return Err("unlisted inherited descriptor".into());
        }
    }
    Ok(Activation {
        namespaces,
        delegated: record.descriptors.into_keys().collect(),
    })
}

fn validate_namespaces(fds: &BTreeMap<String, i32>) -> Result<BTreeMap<String, String>, String> {
    let mut actual = BTreeMap::new();
    for (name, fd) in fds {
        let path = format!("/proc/self/fd/{fd}");
        let previous = std::fs::read_link(&path).map_err(|e| e.to_string())?;
        let current =
            std::fs::read_link(format!("/proc/self/ns/{name}")).map_err(|e| e.to_string())?;
        let namespace = File::open(&path).map_err(|e| e.to_string())?;
        // NSFS_MAGIC plus the kernel's descriptor link verifies both object
        // kind and namespace type. A caller-provided string or ordinary file
        // named "mnt:[...]" cannot satisfy the filesystem check.
        let nsfs = rustix::fs::fstatfs(&namespace).map_err(|e| e.to_string())?;
        if nsfs.f_type != 0x6e736673
            || !previous.to_string_lossy().starts_with(&format!("{name}:["))
            || previous == current
        {
            return Err(format!("entry did not cross the kernel {name} namespace"));
        }
        actual.insert(name.clone(), current.to_string_lossy().into_owned());
    }
    Ok(actual)
}
