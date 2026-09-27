//! Verify the immutable C SDK snapshot before compiling its source.
use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    repository: String,
    revision: String,
    files: BTreeMap<String, String>,
}

pub fn run(repo: &Path) -> Result<Vec<String>, String> {
    let revision = verify(&repo.join("vendor/c-desktop-sdk"), repo)?;
    Ok(vec![format!(
        "c_desktop_sdk status=pass revision={revision}"
    )])
}

pub fn verify(snapshot: &Path, repo: &Path) -> Result<String, String> {
    let bytes = std::fs::read(snapshot.join("manifest.json")).map_err(|e| e.to_string())?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if manifest.schema != 1
        || manifest.repository != "https://github.com/sophia-org/sophia-desktop-sdk-c"
        || !hex(&manifest.revision, 40)
        || manifest.files.is_empty()
    {
        return Err("invalid C SDK snapshot identity".into());
    }
    for (name, digest) in &manifest.files {
        let path = Path::new(name);
        if name.is_empty()
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || !hex(digest, 64)
        {
            return Err(format!("invalid C SDK manifest entry {name:?}"));
        }
    }
    let source = snapshot.join("source");
    let mut actual = BTreeMap::new();
    collect(&source, &source, &mut actual)?;
    if actual != manifest.files {
        let changed = actual
            .iter()
            .find(|(name, digest)| manifest.files.get(*name) != Some(*digest))
            .map(|(name, _)| name.as_str())
            .or_else(|| {
                manifest
                    .files
                    .keys()
                    .find(|name| !actual.contains_key(*name))
                    .map(String::as_str)
            })
            .unwrap_or("unknown");
        return Err(format!("C SDK snapshot differs from its pin: {changed}"));
    }
    for (local, authoritative) in [
        (
            "spec/sophia-shell-files-v1.kdl",
            "protocol/sophia-shell-files-v1.kdl",
        ),
        ("spec/sophia-shell-v1.kdl", "protocol/sophia-shell-v1.kdl"),
        ("spec/sophia-9p-profile.md", "docs/sophia-9p-profile.md"),
        ("spec/sophia-shell-files.md", "docs/sophia-shell-files.md"),
        (
            "spec/references/diod-9p2000L-protocol.md",
            "docs/references/diod-9p2000L-protocol.md",
        ),
    ] {
        if std::fs::read(source.join(local)).map_err(|e| e.to_string())?
            != std::fs::read(repo.join(authoritative)).map_err(|e| e.to_string())?
        {
            return Err(format!("C SDK contract drift: {authoritative}"));
        }
    }
    Ok(manifest.revision)
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn collect(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        let path = entry.path();
        if kind.is_dir() {
            collect(root, &path, files)?;
        } else if kind.is_file() {
            let name = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_str()
                .ok_or("non-UTF-8 SDK path")?
                .to_owned();
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            files.insert(name, format!("{:x}", Sha256::digest(bytes)));
        } else {
            return Err(format!(
                "SDK source must contain only regular files: {}",
                path.display()
            ));
        }
    }
    Ok(())
}
