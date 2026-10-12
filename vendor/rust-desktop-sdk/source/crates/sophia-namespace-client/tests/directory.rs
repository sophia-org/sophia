#![cfg(target_os = "linux")]
use sophia_namespace_client::Namespace;
use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::{fs::symlink, net::UnixListener};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "ns-sdk-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn view(&self, name: &str, service: u64, endpoint: &str) -> (PathBuf, UnixListener) {
        let root = self.0.join(name);
        fs::create_dir(&root).unwrap();
        let target = root.join(endpoint);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        let pinned = fs::File::open(&root).unwrap();
        let listener =
            UnixListener::bind(format!("/proc/self/fd/{}/temporary", pinned.as_raw_fd())).unwrap();
        fs::rename(root.join("temporary"), target).unwrap();
        listener.set_nonblocking(true).unwrap();
        fs::write(root.join("discovery"), format!("sophia-namespace\t1\t1\t1\nendpoint\t{endpoint}\t9p2000.L\tinspection\t1\t{service}\t1\tavailable\n")).unwrap();
        (root, listener)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn old_directory_handle_cannot_be_retargeted_by_a_new_path() {
    let f = Fixture::new();
    let (old, a) = f.view("view", 31, "roles/status");
    let pinned = Namespace::open(&old).unwrap();
    fs::rename(&old, f.0.join("retained")).unwrap();
    let (_, b) = f.view("view", 32, "roles/status");
    let next = Namespace::open(&old).unwrap();
    assert_eq!(pinned.discovery().endpoints[0].service, 31);
    assert_eq!(next.discovery().endpoints[0].service, 32);
    let _stream = pinned.connect("roles/status").unwrap();
    assert!(a.accept().is_ok());
    assert_eq!(
        b.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    assert!(pinned.connect("undeclared").is_err());
}

#[test]
fn long_names_connect_without_changing_working_directory() {
    let f = Fixture::new();
    let name = format!("{}/{}", "x".repeat(200), "y".repeat(200));
    let (root, listener) = f.view("long", 31, &name);
    let before = std::env::current_dir().unwrap();
    let pinned = Namespace::open(&root).unwrap();
    let stream = pinned.connect(&name).unwrap();
    assert!(listener.accept().is_ok());
    assert_eq!(std::env::current_dir().unwrap(), before);
    assert!(
        rustix::io::fcntl_getfd(&stream)
            .unwrap()
            .contains(rustix::io::FdFlags::CLOEXEC)
    );
    assert!(
        rustix::fs::fcntl_getfl(&stream)
            .unwrap()
            .contains(rustix::fs::OFlags::NONBLOCK)
    );
}

#[test]
fn discovery_and_endpoint_symlinks_never_escape_the_pinned_root() {
    let f = Fixture::new();
    let (root, _listener) = f.view("real", 31, "roles/status");
    symlink(&root, f.0.join("alias")).unwrap();
    assert!(Namespace::open(&f.0.join("alias")).is_err());
    assert!(Namespace::open(Path::new("relative")).is_err());
    let pinned = Namespace::open(&root).unwrap();
    fs::rename(root.join("roles"), f.0.join("foreign")).unwrap();
    symlink(f.0.join("foreign"), root.join("roles")).unwrap();
    assert!(pinned.connect("roles/status").is_err());
    fs::remove_file(root.join("roles")).unwrap();
    fs::create_dir(root.join("roles")).unwrap();
    symlink(f.0.join("foreign/status"), root.join("roles/status")).unwrap();
    assert!(pinned.connect("roles/status").is_err());
    fs::rename(root.join("discovery"), f.0.join("foreign-discovery")).unwrap();
    symlink(f.0.join("foreign-discovery"), root.join("discovery")).unwrap();
    assert!(Namespace::open(&root).is_err());
}
