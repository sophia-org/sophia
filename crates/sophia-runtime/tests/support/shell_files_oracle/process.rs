use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct ChildGuard(pub Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
pub struct Scratch(pub PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
pub fn build(repo: &Path, dir: &Path) -> PathBuf {
    let module = repo.join("tools/9p-oracle");
    let manifest = std::fs::read_to_string(module.join("go.mod")).unwrap();
    let mut required = Vec::new();
    let mut block = false;
    for line in manifest.lines().map(str::trim) {
        let entry = match line {
            "require (" => {
                block = true;
                continue;
            }
            ")" => {
                block = false;
                continue;
            }
            _ if block => line,
            _ => match line.strip_prefix("require ") {
                Some(entry) => entry,
                None => continue,
            },
        };
        if !entry.ends_with("// indirect") {
            required.push(entry);
        }
    }
    assert_eq!(
        required,
        ["github.com/hugelgupf/p9 v0.4.1"],
        "oracle dependency pin"
    );
    assert!(
        !manifest
            .lines()
            .any(|line| line.trim_start().starts_with("replace"))
    );
    assert!(std::fs::read_to_string(module.join("go.sum")).unwrap().lines().any(|line|line=="github.com/hugelgupf/p9 v0.4.1 h1:04RUBWSYlvP38QX8At5VXnMTg9qNEnbP/548aMLtfsk="));
    let binary = dir.join("shell-oracle");
    let log = dir.join("go-build.log");
    let mut child = ChildGuard(
        Command::new("nice")
            .args(["-n", "19", "go", "build", "-p", "2", "-o"])
            .arg(&binary)
            .arg("./cmd/shell-oracle")
            .current_dir(module)
            .env("GOFLAGS", "-mod=readonly")
            .env("GOPROXY", "off")
            .env("GOTOOLCHAIN", "local")
            .env("GOWORK", "off")
            .env("GOMAXPROCS", "2")
            .env(
                "GOCACHE",
                std::env::var_os("GOCACHE")
                    .map(PathBuf::from)
                    // The checkout may be read-only. The caller owns this
                    // scratch directory and removes it after the test.
                    .unwrap_or_else(|| dir.join("go-cache")),
            )
            .stdout(Stdio::from(std::fs::File::create(&log).unwrap()))
            .stderr(Stdio::from(
                std::fs::OpenOptions::new().append(true).open(&log).unwrap(),
            ))
            .spawn()
            .unwrap(),
    );
    let start = Instant::now();
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(
                status.success(),
                "Go offline build failed: {}",
                std::fs::read_to_string(log).unwrap()
            );
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "Go build deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    binary
}
