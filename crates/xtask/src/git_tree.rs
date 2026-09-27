//! Offline Git identities for immutable SDK source snapshots.
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct Inventory {
    pub tree: String,
    pub files: BTreeMap<String, String>,
}

pub fn inventory(root: &Path) -> Result<Inventory, String> {
    let mut files = BTreeMap::new();
    let tree = collect(root, root, &mut files)?;
    Ok(Inventory { tree, files })
}

/// This binds content to the recorded commit identity. Signature authorization
/// remains a review/release step; hashing a commit does not verify its signer.
pub fn verify_commit(raw: &[u8], revision: &str, tree: &str) -> Result<(), String> {
    if git_hash("commit", raw)? != revision {
        return Err("SDK revision does not identify upstream.commit".into());
    }
    let text = std::str::from_utf8(raw).map_err(|e| e.to_string())?;
    if text.lines().next() != Some(format!("tree {tree}").as_str()) {
        return Err("SDK source tree does not match its pinned commit".into());
    }
    Ok(())
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

// Git's object identity is computed without writing objects or consulting an
// external checkout. The recorded raw commit binds its revision to this tree.
fn git_hash(kind: &str, bytes: &[u8]) -> Result<String, String> {
    let mut child = Command::new("git")
        .args(["hash-object", "--stdin", "-t", kind])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("git hash-object {kind}: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("git stdin unavailable")?
        .write_all(bytes)
        .map_err(|e| format!("git hash-object {kind} input: {e}"))?;
    let result = child.wait_with_output().map_err(|e| e.to_string())?;
    let hash = String::from_utf8(result.stdout).map_err(|e| e.to_string())?;
    let hash = hash.trim();
    if !result.status.success() || !hex(hash, 40) {
        return Err(format!(
            "git hash-object {kind}: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok(hash.to_owned())
}

fn collect(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<String, String> {
    let metadata = std::fs::symlink_metadata(directory)
        .map_err(|e| format!("{}: {e}", directory.display()))?;
    if !metadata.is_dir() {
        return Err(format!(
            "SDK source root/directory must not be symlinked: {}",
            directory.display()
        ));
    }
    let mut entries = BTreeMap::new();
    for entry in
        std::fs::read_dir(directory).map_err(|e| format!("{}: {e}", directory.display()))?
    {
        let entry = entry.map_err(|e| format!("{}: {e}", directory.display()))?;
        let path = entry.path();
        let metadata =
            std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "non-UTF-8 SDK path")?;
        let (mode, object, key) = if metadata.is_dir() {
            ("40000", collect(root, &path, files)?, format!("{name}/"))
        } else if metadata.is_file() {
            let name = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_str()
                .ok_or("non-UTF-8 SDK path")?
                .to_owned();
            let bytes = read(&path)?;
            files.insert(name, format!("{:x}", Sha256::digest(&bytes)));
            let mode = if metadata.permissions().mode() & 0o100 != 0 {
                "100755"
            } else {
                "100644"
            };
            (
                mode,
                git_hash("blob", &bytes)?,
                entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "non-UTF-8 SDK path")?,
            )
        } else {
            return Err(format!(
                "SDK source must contain only regular files: {}",
                path.display()
            ));
        };
        let mut encoded = format!("{mode} {name}\0").into_bytes();
        for index in (0..40).step_by(2) {
            encoded.push(
                u8::from_str_radix(&object[index..index + 2], 16).map_err(|e| e.to_string())?,
            );
        }
        entries.insert(key, encoded);
    }
    git_hash("tree", &entries.into_values().flatten().collect::<Vec<_>>())
}
