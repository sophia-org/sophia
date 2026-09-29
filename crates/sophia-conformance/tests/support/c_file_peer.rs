//! Builds independent C file peers against the pinned C desktop SDK.
//!
//! The SDK is built by its own vendored makefile with only file clients, so the
//! peer can link only `libsophia-desktop` and `libsophia-9p`: no IPC library
//! and no Rust encoder is available to it.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

/// Parallel tests share one SDK build directory per test binary.
static SDK_BUILD: Mutex<()> = Mutex::new(());

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run(command: &mut Command, what: &str) {
    let status = command
        .status()
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    assert!(status.success(), "{what} failed: {status}");
}

/// Build the SDK into `directory/sdk` and the peer into `directory`. A nonzero
/// `mutation` selects one of the peer's red-control builds.
pub fn build(directory: &Path, source: &str, mutation: u32) -> PathBuf {
    assert!(matches!(
        source,
        "shell_content_file_peer" | "shell_descriptor_file_peer"
    ));
    let sdk = repository().join("vendor/c-desktop-sdk/source");
    let libraries = directory.join("sdk");
    let _serial = SDK_BUILD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !libraries.join("libsophia-desktop.a").is_file() {
        run(
            Command::new("make")
                .arg("-s")
                .arg("-j2")
                .arg("-C")
                .arg(&sdk)
                .arg(format!("BUILD={}", libraries.display()))
                .arg("all"),
            "pinned C desktop SDK build",
        );
    }
    assert!(
        !libraries.join("libsophia-desktop-ipc.a").exists(),
        "the peer must not see the SDK's IPC library"
    );
    let peer = directory.join(format!("{source}-{mutation}"));
    run(
        Command::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
            .args([
                "-std=c99",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-pedantic",
                "-UNDEBUG",
            ])
            .arg(format!("-DPEER_MUTATION={mutation}"))
            .arg("-I")
            .arg(sdk.join("src"))
            .arg(repository().join(format!(
                "crates/sophia-conformance/tests/support/{source}.c"
            )))
            .arg("-L")
            .arg(&libraries)
            .args(["-lsophia-desktop", "-lsophia-9p", "-o"])
            .arg(&peer),
        "independent C file peer build",
    );
    peer
}
