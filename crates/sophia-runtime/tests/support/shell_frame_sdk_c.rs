//! Compile the independent pinned C client, sharing the Rust SDK test's owner loop.
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

pub(super) struct Peer {
    child: Child,
    directory: PathBuf,
}

impl Peer {
    pub(super) fn spawn(directory: &Path, socket: &Path) -> Self {
        let sdk =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/c-desktop-sdk/source/src");
        let binary = directory.join("frame-client-c");
        let mut sources = Vec::new();
        for module in [
            "nine_p",
            "shell_files",
            "shell_session",
            "image_channel",
            "image_producer",
        ] {
            sources.extend(
                std::fs::read_dir(sdk.join(module))
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .filter(|path| path.extension().is_some_and(|extension| extension == "c")),
            );
        }
        sources.sort();
        let result = Command::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
            .args([
                "-std=c99",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-pedantic",
                "-UNDEBUG",
            ])
            .arg("-I")
            .arg(sdk)
            .args(sources)
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/shell_frame_sdk_c.c"))
            .arg("-o")
            .arg(&binary)
            .status()
            .unwrap();
        assert!(result.success(), "strict C99 SDK peer build failed");
        let child = Command::new(binary)
            .arg(directory)
            .arg(socket)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        Self {
            child,
            directory: directory.to_owned(),
        }
    }

    pub(super) fn pid(&self) -> u32 {
        self.child.id()
    }

    pub(super) fn is_finished(&mut self) -> bool {
        self.directory.join("c-complete").exists() || self.child.try_wait().unwrap().is_some()
    }

    pub(super) fn finish(&mut self) {
        // Keep the admitted process and socket alive until the owner finishes its pass.
        std::fs::write(self.directory.join("c-release"), []).unwrap();
        assert!(self.child.wait().unwrap().success(), "C SDK peer failed");
        assert!(self.directory.join("c-complete").exists());
    }
}

impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
