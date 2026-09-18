use super::{process, types::SourceIdentity};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::os::unix::{ffi::OsStrExt, fs::PermissionsExt};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub(super) fn digest(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub(super) fn contents(root: &Path) -> Result<String, String> {
    fn visit(root: &Path, path: &Path, hash: &mut Sha256) -> Result<(), String> {
        let mut entries = std::fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort();
        for entry in entries {
            let metadata = std::fs::symlink_metadata(&entry).map_err(|e| e.to_string())?;
            hash.update(
                entry
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .as_os_str()
                    .as_bytes(),
            );
            hash.update([0]);
            hash.update(metadata.permissions().mode().to_le_bytes());
            if metadata.is_symlink() {
                hash.update(
                    std::fs::read_link(&entry)
                        .map_err(|e| e.to_string())?
                        .as_os_str()
                        .as_bytes(),
                );
            } else if metadata.is_dir() {
                visit(root, &entry, hash)?;
            } else if metadata.is_file() {
                hash.update(digest(&entry)?.as_bytes());
            } else {
                return Err("snapshot contains a non-data artifact".into());
            }
            hash.update([0]);
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    visit(root, root, &mut hash)?;
    Ok(format!("{:x}", hash.finalize()))
}

pub(super) fn git(repo: &Path, arguments: &[&str], log: &Path) -> Result<String, String> {
    let mut command = process::private_command("/usr/bin/git");
    command
        .arg("-C")
        .arg(repo)
        .args(arguments)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    process::capture(&mut command, log)
}

pub(super) fn snapshot(repo: &Path, output: &Path) -> Result<SourceIdentity, String> {
    let log = output.join("git.log");
    let clean = git(
        repo,
        &["status", "--porcelain=v1", "--untracked-files=all"],
        &log,
    )?
    .is_empty();
    if !clean {
        return Err("source is dirty; commit the exact candidate before acceptance".into());
    }
    let commit = git(repo, &["rev-parse", "HEAD"], &log)?;
    let tree = git(repo, &["rev-parse", "HEAD^{tree}"], &log)?;
    let archive = output.join("source.tar");
    let result = process::run(
        process::private_command("/usr/bin/git")
            .arg("-C")
            .arg(repo)
            .args(["archive", "--format=tar", &commit]),
        &archive,
        Duration::from_secs(120),
    )?;
    if !result.clean() {
        return Err("committed source archive failed".into());
    }
    let source = output.join("source");
    std::fs::create_dir(&source).map_err(|e| e.to_string())?;
    let extracted = process::run(
        process::private_command("/usr/bin/tar")
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(&source),
        &output.join("extract.log"),
        Duration::from_secs(120),
    )?;
    if !extracted.clean() {
        return Err("committed source extraction failed".into());
    }
    if git(repo, &["rev-parse", "HEAD"], &log)? != commit
        || !git(
            repo,
            &["status", "--porcelain=v1", "--untracked-files=all"],
            &log,
        )?
        .is_empty()
    {
        return Err("source changed while snapshot was prepared".into());
    }
    Ok(SourceIdentity {
        commit,
        tree,
        clean,
        archive_sha256: digest(&archive)?,
        content_sha256: contents(&source)?,
    })
}

pub(super) fn namespaces() -> Result<BTreeMap<String, String>, String> {
    ["mnt", "net", "pid", "user", "ipc", "uts"]
        .into_iter()
        .map(|name| {
            let value =
                std::fs::read_link(format!("/proc/self/ns/{name}")).map_err(|e| e.to_string())?;
            Ok((name.to_owned(), value.display().to_string()))
        })
        .collect()
}

pub(super) fn json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let text = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let temporary = path.with_extension("tmp");
    std::fs::write(&temporary, text).map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}

pub(super) fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

pub(super) fn toolchain(repo: &Path, output: &Path) -> Result<std::path::PathBuf, String> {
    // Only resolve the selected compiler here. Builds run inside containment.
    let sysroot = process::capture(
        Command::new("rustc")
            .current_dir(repo)
            .args(["--print", "sysroot"]),
        &output.join("toolchain.log"),
    )?;
    std::fs::canonicalize(sysroot).map_err(|e| e.to_string())
}
