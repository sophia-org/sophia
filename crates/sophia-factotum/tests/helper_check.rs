//! The agent executes the PAM helper with the user's password, so it checks
//! the helper's whole path before the first attempt and then executes only
//! the canonical path it checked.

use sophia_factotum::agent::check_helper;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sophia-factotum-helper-check-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A root-owned executable whose every directory is root's alone. `None`
/// inside a user namespace that does not map root (the isolated test
/// wrapper's), where no file can be seen to belong to root.
fn system_executable() -> Option<PathBuf> {
    use std::os::unix::fs::MetadataExt;
    if std::fs::metadata("/").unwrap().uid() != 0 {
        eprintln!("root is not mapped here; the accepting case is not exercised");
        return None;
    }
    let canonical = std::fs::canonicalize("/bin/sh").unwrap();
    assert_eq!(check_helper(&canonical), Ok(canonical.clone()));
    Some(canonical)
}

#[test]
fn a_relative_or_missing_helper_is_refused() {
    assert!(check_helper(Path::new("bin/sh")).is_err());
    let scratch = Scratch::new("missing");
    assert!(check_helper(&scratch.0.join("absent")).is_err());
}

#[test]
fn a_helper_the_user_owns_is_refused() {
    if rustix::process::geteuid().is_root() {
        return;
    }
    let scratch = Scratch::new("owned");
    let helper = scratch.0.join("helper");
    std::fs::write(&helper, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(check_helper(&helper).is_err());
}

#[test]
fn a_root_file_that_is_not_executable_is_refused() {
    assert!(check_helper(Path::new("/etc/passwd")).is_err());
}

#[test]
fn a_symlink_in_a_user_directory_resolves_to_the_path_that_is_executed() {
    let Some(target) = system_executable() else {
        return;
    };
    let scratch = Scratch::new("link");
    let link = scratch.0.join("helper");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    // The user could repoint the link at any time, so the agent never
    // executes it: it executes the canonical path, all of it root's.
    assert_eq!(check_helper(&link), Ok(target.clone()));
    let directory = scratch.0.join("bin");
    std::os::unix::fs::symlink(target.parent().unwrap(), &directory).unwrap();
    let through = directory.join(target.file_name().unwrap());
    assert_eq!(check_helper(&through), Ok(target));
}

#[test]
fn a_symlink_to_a_user_owned_helper_is_refused() {
    if rustix::process::geteuid().is_root() {
        return;
    }
    let scratch = Scratch::new("user-target");
    let helper = scratch.0.join("helper");
    std::fs::write(&helper, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let link = scratch.0.join("link");
    std::os::unix::fs::symlink(&helper, &link).unwrap();
    assert!(check_helper(&link).is_err());
}
