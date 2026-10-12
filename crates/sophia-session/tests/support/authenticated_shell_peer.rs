//! Shared generic shell SDK compiler; binaries remain inside the leased target.
use std::path::PathBuf;

pub(crate) struct Peer(pub(crate) PathBuf);
impl Peer {
    pub(crate) fn build(label: &str) -> Self {
        let target = PathBuf::from(std::env::var_os("CARGO_TARGET_DIR").expect("cargo-slot"));
        let temporary = target.join("tmp");
        std::fs::create_dir_all(&temporary).unwrap();
        let binary = temporary.join(format!("t317-shell-sdk-{label}-{}", std::process::id()));
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vendor/c-desktop-sdk/source/src");
        let mut units: Vec<_> = ["nine_p", "launch_auth", "shell_files", "shell_session"]
            .into_iter()
            .flat_map(|dir| std::fs::read_dir(source.join(dir)).unwrap())
            .map(|entry| entry.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "c"))
            .collect();
        units.sort();
        assert!(
            std::process::Command::new("/usr/bin/cc")
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .args([
                    "-std=c99",
                    "-Wall",
                    "-Wextra",
                    "-Werror",
                    "-pedantic",
                    "-O2"
                ])
                .arg("-I")
                .arg(source)
                .arg(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/tests/support/shell_authentication_peer.c"
                ))
                .args(units)
                .arg("-o")
                .arg(&binary)
                .status()
                .unwrap()
                .success()
        );
        Self(binary)
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
