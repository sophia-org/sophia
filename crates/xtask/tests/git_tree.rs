#[path = "../src/git_tree.rs"]
mod git_tree;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn git(directory: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(directory)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn tree_identity_matches_git_including_directory_order_and_executable_mode() {
    let root =
        Scratch(std::env::temp_dir().join(format!("sophia-git-tree-{}", std::process::id())));
    std::fs::create_dir(&root.0).unwrap();
    let source = root.0.join("source");
    std::fs::create_dir_all(source.join("a")).unwrap();
    std::fs::write(source.join("a/z"), b"nested").unwrap();
    std::fs::write(source.join("a.c"), b"sort before the a/ tree").unwrap();
    std::fs::write(source.join("a0"), b"sort after the a/ tree").unwrap();
    std::fs::write(source.join("run"), b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(source.join("run"), std::fs::Permissions::from_mode(0o755)).unwrap();
    git(
        &root.0,
        &["init", "--bare", "--object-format=sha1", "objects"],
    );
    let args = ["--git-dir=objects", "--work-tree=source"];
    git(&root.0, &[args[0], args[1], "add", "--all"]);
    let expected = git(&root.0, &[args[0], args[1], "write-tree"]);
    let inventory = git_tree::inventory(&source).unwrap();
    assert_eq!(inventory.tree, expected);
    assert_eq!(inventory.files.len(), 4);
    let raw = format!(
        "tree {expected}\nauthor Fixture <fixture@example.invalid> 0 +0000\ncommitter Fixture <fixture@example.invalid> 0 +0000\n\nfixture\n"
    );
    std::fs::write(root.0.join("commit"), &raw).unwrap();
    let revision = git(&root.0, &["hash-object", "-t", "commit", "commit"]);
    assert!(git_tree::verify_commit(raw.as_bytes(), &revision, &expected).is_ok());
    assert!(git_tree::verify_commit(raw.as_bytes(), &"0".repeat(40), &expected).is_err());
    assert!(git_tree::verify_commit(raw.as_bytes(), &revision, &"0".repeat(40)).is_err());
}
