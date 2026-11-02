//! C SDK peer against the same host and lifecycle assertions as the Rust peer.
use super::{Seen, resource};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;

pub struct Binary(PathBuf);

pub fn compile() -> Binary {
    let source = std::env::var_os("SOPHIA_C_SDK_SOURCE").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/c-desktop-sdk/source"),
        PathBuf::from,
    );
    assert!(source.is_absolute() && source.is_dir());
    let root = source.join("src");
    let binary =
        Binary(std::env::temp_dir().join(format!("sophia-w2-c-peer-{}", std::process::id())));
    let mut cc = Command::new(std::env::var_os("CC").unwrap_or_else(|| "cc".into()));
    cc.args([
        "-std=c99",
        "-Wall",
        "-Wextra",
        "-Werror",
        "-pedantic",
        "-UNDEBUG",
    ]);
    for domain in [
        "nine_p",
        "shell_files",
        "shell_session",
        "image_channel",
        "image_producer",
    ] {
        let mut sources: Vec<_> = std::fs::read_dir(root.join(domain))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "c"))
            .collect();
        sources.sort();
        cc.args(sources);
    }
    cc.arg(root.join("desktop_connection.c"))
        .arg(root.join("tests/shell_retire_on_present_peer.c"))
        .arg("-o")
        .arg(&binary.0);
    assert!(cc.status().unwrap().success(), "strict C99 build failed");
    binary
}

impl Binary {
    pub fn spawn(
        self,
        socket: &Path,
        seen: mpsc::Sender<Seen>,
    ) -> (std::thread::JoinHandle<()>, u32) {
        let mut child = Command::new(&self.0)
            .arg(socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id();
        child.stdin.take().unwrap().write_all(b"G").unwrap();
        let handle = std::thread::spawn(move || {
            let _binary = self;
            for line in BufReader::new(child.stdout.take().unwrap()).lines() {
                let line = line.unwrap();
                let (kind, id) = line.split_once(' ').expect("C lifecycle report");
                let id = id.parse().unwrap();
                let event = match kind {
                    "uploaded" => Seen::Uploaded(id),
                    "retiring" => Seen::Retiring(resource(id)),
                    "released" => Seen::Released(resource(id)),
                    "kept" => Seen::Kept(resource(id)),
                    "closed" => {
                        assert_eq!(id, 0);
                        Seen::Closed
                    }
                    _ => panic!("unexpected C report: {line}"),
                };
                seen.send(event).unwrap();
            }
            assert!(child.wait().unwrap().success(), "C W2 peer failed");
        });
        (handle, pid)
    }
}

impl Drop for Binary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
