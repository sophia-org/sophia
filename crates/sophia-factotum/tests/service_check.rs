//! A PAM service without its own stack falls back to `other`, which commonly
//! refuses everyone. The agent therefore starts only when the service's stack
//! exists where PAM looks first, belongs to root and is writable by no one
//! else; otherwise Session refuses to lock rather than take a lock nobody
//! could open.

use sophia_factotum::agent::check_service;
use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "sophia-factotum-service-check-{}-{name}",
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

#[test]
fn a_name_that_is_not_plain_is_refused() {
    let etc = [Path::new("/etc")];
    for name in ["", "../etc/passwd", "a/b", "sophia lock", &"x".repeat(65)] {
        assert!(check_service(name, &etc).is_err(), "{name:?}");
    }
}

#[test]
fn a_missing_stack_is_refused() {
    let first = Scratch::new("missing-first");
    let second = Scratch::new("missing-second");
    assert_eq!(
        check_service("sophia-lock", &[&first.0, &second.0]),
        Err("pam service file is missing; a lock could not be opened")
    );
}

#[test]
fn the_first_stack_pam_would_read_decides() {
    if rustix::process::geteuid().is_root() {
        return;
    }
    // A user-owned stack where PAM looks first is refused, even though a
    // protected one exists later in the search order.
    let first = Scratch::new("first");
    std::fs::write(first.0.join("passwd"), "auth required pam_permit.so\n").unwrap();
    assert!(check_service("passwd", &[&first.0, Path::new("/etc")]).is_err());
    // So is a symlink, whatever it names.
    let linked = Scratch::new("linked");
    std::os::unix::fs::symlink("/etc/passwd", linked.0.join("passwd")).unwrap();
    assert!(check_service("passwd", &[&linked.0]).is_err());
}

#[test]
fn a_root_owned_protected_stack_is_accepted() {
    use std::os::unix::fs::MetadataExt;
    if std::fs::metadata("/").unwrap().uid() != 0 {
        eprintln!("root is not mapped here; the accepting case is not exercised");
        return;
    }
    // Any root-owned, protected regular file stands in for a stack.
    let absent = Scratch::new("absent");
    assert_eq!(
        check_service("passwd", &[&absent.0, Path::new("/etc")]),
        Ok(PathBuf::from("/etc/passwd"))
    );
}
