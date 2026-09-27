//! Verify the immutable C SDK snapshot before compiling its source.
use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::Deserialize;

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
    let bytes = read(&snapshot.join("manifest.json"))?;
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
            || name.split('/').any(|part| matches!(part, "" | "." | ".."))
            || path
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            || !hex(digest, 64)
        {
            return Err(format!("invalid C SDK manifest entry {name:?}"));
        }
    }
    let source = snapshot.join("source");
    let inventory = crate::git_tree::inventory(&source)?;
    let actual = inventory.files;
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
    let commit = read(&snapshot.join("upstream.commit"))?;
    crate::git_tree::verify_commit(&commit, &manifest.revision, &inventory.tree)?;
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
        ("src/sophia_wm_v1.c", "bindings/c/sophia_wm_v1.c"),
        ("src/sophia_wm_v1.h", "bindings/c/sophia_wm_v1.h"),
    ] {
        same_contract(&source.join(local), &repo.join(authoritative))?;
    }
    for name in [
        "catalog-actions",
        "content-malformed",
        "content",
        "indicators",
        "launcher",
        "native-launcher",
        "reference",
        "tabs",
        "v1-malformed",
        "v1",
    ] {
        let file = format!("sophia-shell-{name}.frames");
        same_contract(
            &source.join("spec/golden").join(&file),
            &repo.join("protocol/golden").join(&file),
        )?;
    }
    Ok(manifest.revision)
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn same_contract(copy: &Path, authoritative: &Path) -> Result<(), String> {
    if read(copy)? != read(authoritative)? {
        return Err(format!("C SDK contract drift: {}", authoritative.display()));
    }
    Ok(())
}
