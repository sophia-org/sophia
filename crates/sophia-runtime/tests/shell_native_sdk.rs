//! The C desktop SDK's native launcher session (sophia_ns over sophia_ss and
//! 9P) against the production file export and content owners. Session steps
//! are scripted by the fixture; no launch-policy, physical-renderer or
//! pointer-activation claim is made here. The fixture's content clock is
//! frozen, so no deadline or production timing behaviour is covered.
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
#[path = "support/shell_native_sdk/fixture.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "support/shell_files_oracle/process.rs"]
mod process;

const VERDICT: &str = "sophia_c_native_sdk status=pass\n";

fn compile(repo: &Path, root: &Path) -> PathBuf {
    let sdk = repo.join("vendor/c-desktop-sdk/source/src");
    let output = root.join("c-native-peer");
    let mut command = Command::new("nice");
    command
        .args(["-n", "19"])
        .arg(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .args([
            "-std=c99",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pedantic",
            "-UNDEBUG",
        ]);
    for dir in ["nine_p", "shell_files", "shell_session", "native_session"] {
        let mut files = std::fs::read_dir(sdk.join(dir))
            .unwrap()
            .map(|f| f.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "c"))
            .collect::<Vec<_>>();
        files.sort();
        command.args(files);
    }
    command
        .arg(sdk.join("desktop_connection.c"))
        .arg(sdk.join("tests/desktop_native_peer.c"))
        .arg("-o")
        .arg(&output);
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    output
}
struct Control {
    stream: UnixStream,
    bytes: Vec<u8>,
    phase: Option<String>,
}
impl Control {
    /// True once this connection's phase is answered.
    fn poll(&mut self, fixture: &mut fixture::Fixture) -> bool {
        if self.phase.is_none() {
            let mut b = [0; 128];
            match self.stream.read(&mut b) {
                Ok(0) => panic!("C control disconnected"),
                Ok(n) => self.bytes.extend_from_slice(&b[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return false,
                Err(e) => panic!("{e}"),
            }
            assert!(self.bytes.len() <= 128);
            if self.bytes.last() != Some(&b'\n') {
                return false;
            }
            let words = std::str::from_utf8(&self.bytes)
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>();
            assert_eq!(words.len(), 2);
            assert_eq!(words[0], "native");
            self.phase = Some(words[1].into());
        }
        let reply = match fixture.phase(self.phase.as_ref().unwrap()) {
            Ok(false) => return false,
            Ok(true) => "ok\n".to_owned(),
            Err(e) => format!("error {e}\n"),
        };
        self.stream.set_nonblocking(false).unwrap();
        self.stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        self.stream.write_all(reply.as_bytes()).unwrap();
        true
    }
}
#[test]
fn c_native_session_drives_the_launcher_lifecycle_against_production_owners() {
    let root = std::env::temp_dir().join(format!("sophia-c-native-sdk-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let scratch = process::Scratch(root);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = compile(&repo, &scratch.0);
    let control_path = scratch.0.join("control.sock");
    let listener = UnixListener::bind(&control_path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut fixture = fixture::Fixture::new(&scratch.0);
    let stdout = scratch.0.join("peer.stdout");
    let stderr = scratch.0.join("peer.stderr");
    let mut child = process::ChildGuard(
        Command::new("nice")
            .args(["-n", "19"])
            .arg(&binary)
            .arg(&control_path)
            .arg(fixture.socket_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::from(std::fs::File::create(&stdout).unwrap()))
            .stderr(Stdio::from(std::fs::File::create(&stderr).unwrap()))
            .spawn()
            .unwrap(),
    );
    fixture.authorize(child.0.id());
    child.0.stdin.take().unwrap().write_all(b"G").unwrap();
    let start = Instant::now();
    let mut control: Option<Control> = None;
    let mut owner_errors = Vec::new();
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            start.elapsed() < Duration::from_secs(40),
            "C native peer deadline: {}",
            std::fs::read_to_string(&stderr).unwrap()
        );
        if let Err(e) = fixture.tick() {
            owner_errors.push(e);
            assert!(owner_errors.len() < 16);
        }
        if control.is_none() {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true).unwrap();
                    control = Some(Control {
                        stream,
                        bytes: Vec::new(),
                        phase: None,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => panic!("{e}"),
            }
        }
        if control.as_mut().is_some_and(|c| c.poll(&mut fixture)) {
            control = None;
        }
        for p in [&stdout, &stderr] {
            assert!(std::fs::metadata(p).unwrap().len() < 65536);
        }
        std::thread::sleep(Duration::from_micros(100));
    };
    fixture.cleanup();
    let out = std::fs::read_to_string(&stdout).unwrap();
    let err = std::fs::read_to_string(&stderr).unwrap();
    assert!(
        status.success() && owner_errors.is_empty(),
        "{status}\n{out}\n{err}\n{owner_errors:?}"
    );
    assert_eq!(out, VERDICT);
    fixture.assert_finished();
}
