//! C r7/r8 sessions against the production file export and content owners.
//! Catalog admission is scripted; no Session launch-policy claim.
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
#[allow(dead_code)]
#[path = "support/shell_files_oracle/process.rs"]
mod process;
#[allow(dead_code)]
#[path = "support/shell_files_oracle/roles.rs"]
mod roles;

fn compile(repo: &Path, root: &Path) -> PathBuf {
    let bindings = repo.join("bindings/c");
    let output = root.join("c-role-peer");
    let mut command = Command::new("nice");
    command
        .args(["-n", "19"])
        .arg(std::env::var_os("CC").unwrap_or_else(|| "cc".into()))
        .args(["-std=c99", "-Wall", "-Wextra", "-Werror", "-pedantic"]);
    for dir in ["nine_p", "shell_files"] {
        let mut files = std::fs::read_dir(bindings.join(dir))
            .unwrap()
            .map(|f| f.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "c"))
            .collect::<Vec<_>>();
        files.sort();
        command.args(files);
    }
    command
        .arg(bindings.join("tests/sophia_shell_files_role_peer.c"))
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
    fn poll(&mut self, fixture: &mut roles::Fixture) -> bool {
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
            assert_eq!(words[0], fixture.name);
            self.phase = Some(words[1].into());
        }
        let result = fixture.phase(self.phase.as_ref().unwrap());
        if matches!(result, Ok(false)) {
            return false;
        }
        let reply = match result {
            Ok(true) => "ok\n".to_owned(),
            Err(e) => format!("error {e}\n"),
            Ok(false) => unreachable!(),
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
fn native_launcher_and_persistent_dock_use_the_c_file_session() {
    let root = std::env::temp_dir().join(format!("sophia-c-role-files-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let scratch = process::Scratch(root);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = compile(&repo, &scratch.0);
    let listener = UnixListener::bind(scratch.0.join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    for (i, name) in ["launcher", "dock-small", "dock"].into_iter().enumerate() {
        let mut fixture = roles::Fixture::new(&scratch.0, name, i);
        let stdout = scratch.0.join(format!("{name}.stdout"));
        let stderr = scratch.0.join(format!("{name}.stderr"));
        let mut child = process::ChildGuard(
            Command::new("nice")
                .args(["-n", "19"])
                .arg(&binary)
                .arg(&scratch.0)
                .arg(name)
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
                "C role deadline: {}",
                std::fs::read_to_string(&stderr).unwrap()
            );
            if let Err(e) = fixture.tick() {
                owner_errors.push(e)
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
                assert!(std::fs::metadata(p).unwrap().len() < 65536)
            }
            std::thread::sleep(Duration::from_micros(100));
        };
        fixture.cleanup();
        let out = std::fs::read_to_string(&stdout).unwrap();
        let err = std::fs::read_to_string(&stderr).unwrap();
        assert!(
            status.success() && owner_errors.is_empty(),
            "{name}: {status}\n{out}\n{err}\n{owner_errors:?}"
        );
        assert_eq!(
            out,
            format!("sophia_c_role_files role={name} status=pass\n")
        );
        fixture.assert_finished();
    }
}
