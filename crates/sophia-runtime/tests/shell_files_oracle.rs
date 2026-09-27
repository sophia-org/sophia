//! Independent Go judgments of the production shell file export. Admission and
//! scheduling are supplied fixtures; no protected launcher or native display.
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[path = "support/shell_files_oracle/fixture.rs"]
mod fixture;
#[path = "support/shell_files_oracle/process.rs"]
mod process;
#[path = "support/shell_files_oracle/verdict.rs"]
mod verdict;

struct Control {
    stream: UnixStream,
    bytes: Vec<u8>,
    command: Option<(String, String)>,
}
impl Control {
    fn poll(&mut self, fixtures: &mut [fixture::Fixture]) -> bool {
        if self.command.is_none() {
            let mut b = [0; 128];
            match self.stream.read(&mut b) {
                Ok(0) => panic!("control disconnected"),
                Ok(n) => self.bytes.extend_from_slice(&b[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return false,
                Err(e) => panic!("control read {e}"),
            }
            assert!(self.bytes.len() <= 128, "control bound");
            if self.bytes.last() != Some(&b'\n') {
                return false;
            }
            let line = std::str::from_utf8(&self.bytes).unwrap();
            let words = line.split_whitespace().collect::<Vec<_>>();
            assert_eq!(words.len(), 2);
            self.command = Some((words[0].into(), words[1].into()));
        }
        let (name, phase) = self.command.as_ref().unwrap();
        let f = fixtures
            .iter_mut()
            .find(|f| f.name == name)
            .expect("known fixture");
        match f.phase(phase) {
            Ok(false) => false,
            result => {
                let line = match result {
                    Ok(true) => "ok\n".to_owned(),
                    Err(e) => format!("error {e}\n"),
                    Ok(false) => unreachable!(),
                };
                self.stream.set_nonblocking(false).unwrap();
                self.stream
                    .set_write_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                self.stream.write_all(line.as_bytes()).unwrap();
                true
            }
        }
    }
}

#[test]
fn independent_go_judges_the_production_shell_file_export() {
    let root = std::env::temp_dir().join(format!("sophia-shell-oracle-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let scratch = process::Scratch(root);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let binary = process::build(&repo, &scratch.0);
    let listener = UnixListener::bind(scratch.0.join("control.sock")).unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut fixtures = fixture::NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| fixture::Fixture::new(&scratch.0, name, i))
        .collect::<Vec<_>>();
    let stdout = scratch.0.join("stdout");
    let stderr = scratch.0.join("stderr");
    let mut child = process::ChildGuard(
        Command::new("nice")
            .args(["-n", "19"])
            .arg(binary)
            .arg("-root")
            .arg(&scratch.0)
            .env("GOMAXPROCS", "2")
            .stdin(Stdio::piped())
            .stdout(Stdio::from(std::fs::File::create(&stdout).unwrap()))
            .stderr(Stdio::from(std::fs::File::create(&stderr).unwrap()))
            .spawn()
            .unwrap(),
    );
    for f in &mut fixtures {
        f.authorize(child.0.id());
    }
    child.0.stdin.take().unwrap().write_all(b"G").unwrap();
    let start = Instant::now();
    let mut control: Option<Control> = None;
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(
            start.elapsed() < Duration::from_secs(90),
            "oracle deadline\n{}\n{}",
            std::fs::read_to_string(&stdout).unwrap(),
            std::fs::read_to_string(&stderr).unwrap()
        );
        for path in [&stdout, &stderr] {
            assert!(
                std::fs::metadata(path).unwrap().len() < 65536,
                "oracle output bound"
            );
        }
        for f in &mut fixtures {
            f.tick();
        }
        if control.is_none() {
            match listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(true).unwrap();
                    control = Some(Control {
                        stream,
                        bytes: Vec::new(),
                        command: None,
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => panic!("control accept {e}"),
            }
        }
        if control.as_mut().is_some_and(|c| c.poll(&mut fixtures)) {
            control = None;
        }
        std::thread::sleep(Duration::from_micros(100));
    };
    let output = std::fs::read_to_string(&stdout).unwrap();
    let errors = std::fs::read_to_string(&stderr).unwrap();
    for f in &mut fixtures {
        f.cleanup();
    }
    verdict::validate(&output, status.success())
        .unwrap_or_else(|e| panic!("{e}\n{output}\n{errors}"));
    assert!(fixtures[0].presented && fixtures[0].action_acked);
    assert!(
        fixtures
            .iter()
            .filter(|f| matches!(f.name, "missing" | "cancelled" | "refused" | "unservable"))
            .all(|f| f.revoked)
    );
    println!("{output}");
}

#[test]
fn a_pass_requires_every_named_check_and_a_successful_exit() {
    let text = verdict::valid_transcript();
    assert!(verdict::validate(&text, true).is_ok());
    assert!(verdict::validate(&text, false).is_err());
    assert!(verdict::validate(&text.replacen(" ok\n", " FAIL\n", 1), true).is_err());
    assert!(
        verdict::validate(
            text.lines().skip(1).collect::<Vec<_>>().join("\n").as_str(),
            true
        )
        .is_err()
    );
    assert!(verdict::validate(&format!("check action/echo ok\n{text}"), true).is_err());
    assert!(verdict::validate(&text[..text.len() - 20], true).is_err());
}
