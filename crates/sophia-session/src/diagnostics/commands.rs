//! Session identity, retention, and incident operations; independent of Engine liveness.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::storage::{Directory, bytes, field, invalid, names, token};
use super::{METADATA_LIMIT, SEGMENT_LIMIT, SEGMENTS, Stamp, boot_id, start_ticks};

#[derive(Clone, Copy, Debug)]
pub struct Retention {
    pub finished_sessions: usize,
    pub bytes: u64,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            finished_sessions: 20,
            bytes: 1024 * 1024 * 1024,
        }
    }
}

pub struct Store {
    root: Directory,
    preserved: PathBuf,
    retention: Retention,
}

#[derive(Clone, Debug)]
pub struct SessionRecord {
    pub id: String,
    pub profile: String,
    pub status: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub recording: String,
}

#[derive(Clone, Debug)]
pub struct Marker {
    pub id: String,
    pub session: String,
    pub path: PathBuf,
}

pub struct Inspection {
    pub record: SessionRecord,
    pub manifest: String,
    pub identities: String,
    pub health: String,
    pub markers: String,
    pub lifecycle: String,
    pub events: Vec<String>,
    pub window_start_msec: Option<u64>,
    pub window_end_msec: Option<u64>,
    pub retained_first_msec: Option<u64>,
    pub retained_last_msec: Option<u64>,
}

impl Store {
    pub fn from_environment() -> io::Result<Self> {
        let state = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
            })
            .ok_or_else(|| invalid("XDG_STATE_HOME or HOME is required"))?;
        if !state.is_absolute() {
            return Err(invalid("state home must be absolute"));
        }
        Self::open(&state.join("sophia"), Retention::default())
    }

    pub fn open(parent: &Path, retention: Retention) -> io::Result<Self> {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt};
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)?;
        let metadata = fs::symlink_metadata(parent)?;
        if !metadata.is_dir()
            || metadata.uid() != rustix::process::getuid().as_raw()
            || metadata.mode() & 0o022 != 0
        {
            return Err(invalid("unsafe diagnostic parent ownership or permissions"));
        }
        Ok(Self {
            root: Directory::open(&parent.join("sessions"), true)?,
            preserved: parent.join("session-investigations"),
            retention,
        })
    }

    pub fn begin(&self, profile: &str, owner: u32, identity: &str) -> io::Result<SessionRecord> {
        if !token(profile) || identity.len() as u64 > METADATA_LIMIT / 2 {
            return Err(invalid("invalid session identity"));
        }
        let _lock = self.root.lock()?;
        if self
            .list_unlocked()?
            .iter()
            .any(|r| r.status == "running" && r.profile == profile)
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "a session with this profile is already running",
            ));
        }
        self.prune_unlocked()?;
        let now = Stamp::now();
        let nonce = fs::read_to_string("/proc/sys/kernel/random/uuid")?;
        let id = format!("{:020}-{}", now.utc_msec, nonce.trim());
        let run = self.root.child(&id, true)?;
        let _run_lock = run.lock()?;
        run.replace("manifest", &format!("schema=1\nsession_id={id}\nprofile={profile}\nstarted_utc_msec={}\nstarted_boot_msec={}\nowner_boot={}\nowner_pid={owner}\nowner_start_ticks={}\n{identity}", now.utc_msec, now.boot_msec, boot_id()?, start_ticks(owner)?))?;
        run.replace("identity.log", "")?;
        run.replace("markers.log", "")?;
        run.replace(
            "health",
            "discarded=0\nrotated_bytes=0\nrecording=starting\n",
        )?;
        drop(_run_lock);
        self.prune_unlocked()?;
        // Keep legacy directories, with an explicit view of the new record.
        // Failure to publish this convenience link does not invalidate evidence.
        let _ = self.publish_current(profile, &run.path);
        self.record(&id)
    }

    fn publish_current(&self, profile: &str, target: &Path) -> io::Result<()> {
        let parent = self
            .root
            .path
            .parent()
            .ok_or_else(|| invalid("missing diagnostic parent"))?;
        let directory = Directory::open(&parent.join(format!("{profile}-session")), true)?;
        let _lock = directory.lock()?;
        let temporary = directory.path.join("current.new");
        match fs::remove_file(&temporary) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        std::os::unix::fs::symlink(target, &temporary)?;
        fs::rename(temporary, directory.path.join("current"))
    }

    fn record(&self, id: &str) -> io::Result<SessionRecord> {
        let run = self.root.child(id, false)?;
        let manifest = match run.read("manifest", METADATA_LIMIT) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        if !manifest.is_empty()
            && (field(&manifest, "schema") != Some("1")
                || field(&manifest, "session_id") != Some(id))
        {
            return Err(invalid(
                "diagnostic manifest has an invalid schema or session identity",
            ));
        }
        let outcome = match run.read("outcome", METADATA_LIMIT) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        let status = if let Some(status) = field(&outcome, "status") {
            status.to_owned()
        } else if owner_alive(&manifest) {
            "running".into()
        } else {
            "interrupted".into()
        };
        Ok(SessionRecord {
            id: id.into(),
            profile: field(&manifest, "profile").unwrap_or("unknown").into(),
            status,
            path: run.path.clone(),
            bytes: bytes(&run)?,
            recording: run
                .read("health", METADATA_LIMIT)
                .ok()
                .and_then(|health| field(&health, "recording").map(str::to_owned))
                .unwrap_or_else(|| "unavailable".into()),
        })
    }

    fn list_unlocked(&self) -> io::Result<Vec<SessionRecord>> {
        names(&self.root)?
            .into_iter()
            .map(|name| self.record(&name))
            .collect()
    }

    pub fn list(&self) -> io::Result<Vec<SessionRecord>> {
        let _lock = self.root.lock()?;
        self.list_unlocked()
    }

    pub fn select(&self, selector: Option<&str>) -> io::Result<SessionRecord> {
        let _lock = self.root.lock()?;
        self.select_unlocked(selector)
    }

    fn select_unlocked(&self, selector: Option<&str>) -> io::Result<SessionRecord> {
        let records = self.list_unlocked()?;
        match selector {
            Some("latest") => records
                .into_iter()
                .next_back()
                .ok_or_else(|| invalid("no retained sessions")),
            Some(id) => records
                .into_iter()
                .find(|r| r.id == id)
                .ok_or_else(|| invalid("session is not retained")),
            None => {
                let mut live = records.into_iter().filter(|r| r.status == "running");
                let record = live.next().ok_or_else(|| {
                    invalid("no live session; select --session=latest or an explicit ID")
                })?;
                if live.next().is_some() {
                    return Err(invalid(
                        "multiple live sessions; select an explicit session ID",
                    ));
                }
                Ok(record)
            }
        }
    }

    pub fn transfer_owner(&self, id: &str, owner: u32) -> io::Result<()> {
        let _lock = self.root.lock()?;
        let run = self.root.child(id, false)?;
        let _run_lock = run.lock()?;
        let manifest = run.read("manifest", METADATA_LIMIT)?;
        let mut updated = manifest
            .lines()
            .filter(|line| !line.starts_with("owner_"))
            .collect::<Vec<_>>()
            .join("\n");
        updated.push_str(&format!(
            "\nowner_pid={owner}\nowner_boot={}\nowner_start_ticks={}\n",
            boot_id()?,
            start_ticks(owner)?
        ));
        run.replace("manifest", &updated)
    }

    pub fn finish(&self, id: &str, exit_status: Option<i32>) -> io::Result<()> {
        let _lock = self.root.lock()?;
        let run = self.root.child(id, false)?;
        let _run_lock = run.lock()?;
        let now = Stamp::now();
        let outcome = match exit_status {
            Some(0) => "exited",
            Some(_) => "failed",
            None => "interrupted",
        };
        run.replace(
            "outcome",
            &format!(
                "status={outcome}\nexit_status={}\nended_utc_msec={}\nended_boot_msec={}\n",
                exit_status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "unknown".into()),
                now.utc_msec,
                now.boot_msec
            ),
        )?;
        drop(_run_lock);
        self.prune_unlocked()
    }

    pub fn prune(&self) -> io::Result<()> {
        let _lock = self.root.lock()?;
        self.prune_unlocked()
    }

    fn prune_unlocked(&self) -> io::Result<()> {
        let records = self.list_unlocked()?;
        let mut total = records
            .iter()
            .map(|r| {
                if r.status == "running" {
                    r.bytes.max(80 * 1024 * 1024)
                } else {
                    r.bytes
                }
            })
            .sum::<u64>();
        let mut finished = records.iter().filter(|r| r.status != "running").count();
        for record in records {
            if record.status == "running" {
                continue;
            }
            if finished <= self.retention.finished_sessions && total <= self.retention.bytes {
                break;
            }
            let run = self.root.child(&record.id, false)?;
            let _run_lock = run.lock()?;
            // Only directories created in this rolling store are eligible. Legacy
            // attempts, preserved snapshots, and proof archives never enter here.
            bytes(&run)?;
            fs::remove_dir_all(&run.path)?;
            total = total.saturating_sub(record.bytes);
            finished -= 1;
        }
        Ok(())
    }

    pub fn mark(&self, selector: Option<&str>, label: &str) -> io::Result<Marker> {
        if label.len() > 256 || label.chars().any(char::is_control) {
            return Err(invalid(
                "incident label must be at most 256 UTF-8 bytes without control characters",
            ));
        }
        let _lock = self.root.lock()?;
        let record = self.select_unlocked(selector)?;
        let run = self.root.child(&record.id, false)?;
        let _run_lock = run.lock()?;
        let now = Stamp::now();
        let id = fs::read_to_string("/proc/sys/kernel/random/uuid")?
            .trim()
            .to_owned();
        run.append(
            "markers.log",
            &format!(
                "{id}\t{}\t{}\t{}\t{label}\n",
                now.utc_msec,
                now.boot_msec,
                boot_id()?
            ),
            METADATA_LIMIT,
        )?;
        run.sync("markers.log")?;
        Ok(Marker {
            id,
            session: record.id,
            path: record.path,
        })
    }

    pub fn inspect(&self, selector: &str, marker: Option<&str>) -> io::Result<Inspection> {
        let _lock = self.root.lock()?;
        let record = self.select_unlocked(Some(selector))?;
        let run = self.root.child(&record.id, false)?;
        let _run_lock = run.lock()?;
        let manifest = run.read("manifest", METADATA_LIMIT)?;
        let markers = run.read("markers.log", METADATA_LIMIT)?;
        let window = if let Some(marker) = marker {
            let entry = markers
                .lines()
                .find(|line| line.split('\t').next() == Some(marker))
                .ok_or_else(|| invalid("marker is not in this session"))?;
            let fields = entry.splitn(5, '\t').collect::<Vec<_>>();
            let same_boot = fields.get(3).copied() == field(&manifest, "owner_boot");
            // Post-reboot markers describe a report time, not the incident time.
            // Use the retained session tail rather than compare unrelated clocks.
            if same_boot {
                Some(
                    fields[2]
                        .parse::<u64>()
                        .map_err(|_| invalid("invalid marker time"))?,
                )
            } else {
                None
            }
        } else {
            None
        };
        let mut events = Vec::new();
        let mut first = None;
        let mut last = None;
        for index in (0..SEGMENTS).rev() {
            let content = match run.read(&format!("events.{index}.log"), SEGMENT_LIMIT) {
                Ok(value) => value,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            for line in content.lines() {
                let time = line
                    .split('\t')
                    .nth(2)
                    .and_then(|time| time.parse::<u64>().ok());
                if let Some(time) = time {
                    first = Some(first.map_or(time, |old: u64| old.min(time)));
                    last = Some(last.map_or(time, |old: u64| old.max(time)));
                    events.push(line.to_owned());
                }
            }
        }
        // An incident reported after an exit (including a reboot) has a report
        // timestamp, not a known incident timestamp. Show its final retained minute.
        let window = window.filter(|center| {
            record.status == "running" || last.is_some_and(|last| *center <= last)
        });
        let lower = window.unwrap_or(last.unwrap_or(0)).saturating_sub(60_000);
        let upper = window.map(|center| center.saturating_add(60_000));
        events.retain(|line| {
            line.split('\t')
                .nth(2)
                .and_then(|value| value.parse::<u64>().ok())
                .is_some_and(|time| time >= lower && upper.is_none_or(|upper| time <= upper))
        });
        events.sort_by_key(|line| {
            line.split('\t')
                .nth(2)
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0)
        });
        let mut lifecycle = String::new();
        for name in [
            "lifecycle.log",
            "input-guard.log",
            "recovery.log",
            "outcome",
        ] {
            match run.read(name, METADATA_LIMIT) {
                Ok(value) => {
                    for line in value.lines() {
                        if let Some(line) = super::reduced_record(line) {
                            lifecycle.push_str(&line);
                            lifecycle.push('\n');
                        }
                    }
                    if name == "outcome" {
                        lifecycle.push_str(&value);
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Inspection {
            record,
            manifest,
            markers,
            events,
            lifecycle,
            identities: run.read("identity.log", METADATA_LIMIT)?,
            health: run.read("health", METADATA_LIMIT)?,
            window_start_msec: window.map(|t| t.saturating_sub(60_000)),
            window_end_msec: window.map(|t| t.saturating_add(60_000)),
            retained_first_msec: first,
            retained_last_msec: last,
        })
    }

    pub fn keep(&self, selector: &str) -> io::Result<PathBuf> {
        self.keep_with_application_stderr(selector, false)
    }

    pub fn application_records(
        &self,
        selector: &str,
    ) -> io::Result<super::application::ApplicationRecords> {
        let _lock = self.root.lock()?;
        let record = self.select_unlocked(Some(selector))?;
        let run = self.root.child(&record.id, false)?;
        let _run_lock = run.lock()?;
        super::application::read_records(&run)
    }

    pub fn keep_with_application_stderr(
        &self,
        selector: &str,
        include: bool,
    ) -> io::Result<PathBuf> {
        let _lock = self.root.lock()?;
        let record = self.select_unlocked(Some(selector))?;
        let run = self.root.child(&record.id, false)?;
        let _run_lock = run.lock()?;
        let preserved = Directory::open(&self.preserved, true)?;
        let nonce = fs::read_to_string("/proc/sys/kernel/random/uuid")?;
        let snapshot = preserved.child(&format!("{}-{}", record.id, nonce.trim()), true)?;
        let _snapshot_lock = snapshot.lock()?;
        let mut checksums = String::new();
        for entry in fs::read_dir(&run.path)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if name == "lock"
                || name.ends_with(".new")
                || (!include && name.starts_with("application-"))
            {
                continue;
            }
            let value = run.read_bytes(&name, SEGMENT_LIMIT)?;
            snapshot.replace_bytes(&name, &value)?;
            use sha2::Digest;
            checksums.push_str(&format!("{:x}  {name}\n", sha2::Sha256::digest(&value)));
        }
        snapshot.replace("snapshot", &format!("schema=1\nsource_session={}\nsource_status={}\ncutoff_utc_msec={}\ncutoff_boot_msec={}\ncomplete={}\n", record.id, record.status, Stamp::now().utc_msec, Stamp::now().boot_msec, record.status == "exited" || record.status == "failed"))?;
        snapshot.replace("SHA256SUMS", &checksums)?;
        Ok(snapshot.path.clone())
    }

    pub fn preserved_bytes(&self) -> io::Result<u64> {
        if !self.preserved.exists() {
            return Ok(0);
        }
        let preserved = Directory::open(&self.preserved, false)?;
        names(&preserved)?.iter().try_fold(0u64, |total, name| {
            Ok(total.saturating_add(bytes(&preserved.child(name, false)?)?))
        })
    }
}

fn owner_alive(manifest: &str) -> bool {
    let Some(pid) = field(manifest, "owner_pid").and_then(|pid| pid.parse::<u32>().ok()) else {
        return false;
    };
    boot_id().ok().as_deref() == field(manifest, "owner_boot")
        && start_ticks(pid).ok().as_deref() == field(manifest, "owner_start_ticks")
}
