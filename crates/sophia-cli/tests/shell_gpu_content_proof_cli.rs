#![cfg(feature = "native-session")]

//! The shell GPU content proof refuses before any device access when it is
//! unarmed or its parameters are missing or invalid. No case here reaches the
//! render inventory: every refusal comes from argument parsing or validation.

use std::io::Read as _;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const VALID: [&str; 9] = [
    "--transport=9p2000.L",
    "--client=/absent/shell-client",
    "--output=800x600",
    "--surface=800x24",
    "--edge=top",
    "--outcomes=presented,renderer-failed",
    "--end=client-exits",
    "--pixels=full-surface-raster",
    "--discrete-input=denied",
];

/// Run the proof command with a bounded wait and return its stderr.
fn refuse(armed: bool, args: &[String]) -> String {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sophia"));
    command
        .arg("shell-gpu-content-proof")
        .args(args)
        .env_remove("SOPHIA_SHELL_GPU_EXPECTED_DEVICE")
        .env_remove("SOPHIA_SHELL_GPU_PROOF_ARM")
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if armed {
        command.env("SOPHIA_SHELL_GPU_PROOF_ARM", "1");
    }
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("proof did not refuse promptly: {args:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();
    assert!(!status.success(), "proof accepted {args:?}");
    stderr
}

fn with(replace: &str, value: Option<&str>) -> Vec<String> {
    VALID
        .iter()
        .filter(|arg| !arg.starts_with(replace))
        .map(|arg| (*arg).to_owned())
        .chain(value.map(|value| format!("{replace}{value}")))
        .collect()
}

#[test]
fn an_unarmed_proof_refuses() {
    let stderr = refuse(false, &with("--client=", Some("/absent/shell-client")));
    assert!(stderr.contains("SOPHIA_SHELL_GPU_PROOF_ARM=1"), "{stderr}");
}

#[test]
fn every_required_flag_is_named_when_missing() {
    for flag in [
        "--transport=",
        "--client=",
        "--output=",
        "--surface=",
        "--edge=",
        "--outcomes=",
        "--end=",
        "--pixels=",
        "--discrete-input=",
    ] {
        let stderr = refuse(true, &with(flag, None));
        let key = flag.trim_end_matches('=');
        assert!(
            stderr.contains(&format!("shell-gpu-content-proof requires {key}=")),
            "{flag}: {stderr}"
        );
    }
}

#[test]
fn malformed_values_are_refused() {
    for (flag, value, expected) in [
        ("--transport=", "auto", "shell transport must be"),
        ("--output=", "800", "--output must be WxH"),
        (
            "--output=",
            "4294967296x600",
            "--output dimension overflows",
        ),
        ("--surface=", "-1x24", "--surface must be WxH"),
        ("--edge=", "center", "unknown --edge"),
        ("--outcomes=", "", "unknown --outcomes entry"),
        (
            "--outcomes=",
            "presented,,presented",
            "unknown --outcomes entry",
        ),
        ("--end=", "later", "unknown --end"),
        ("--pixels=", "any-shape", "unknown --pixels"),
        ("--discrete-input=", "yes", "unknown --discrete-input"),
    ] {
        let stderr = refuse(true, &with(flag, Some(value)));
        assert!(stderr.contains(expected), "{flag}{value}: {stderr}");
    }
    let mut args = with("--client=", Some("/absent/shell-client"));
    args.push("--timeout-ms=soon".into());
    assert!(refuse(true, &args).contains("--timeout-ms is not a number"));
}

#[test]
fn invalid_parameters_are_refused_by_validation() {
    let seventeen = vec!["presented"; 17].join(",");
    for (flag, value) in [
        ("--output=", "0x600"),
        ("--output=", "2147483648x600"),
        ("--surface=", "800x0"),
        ("--surface=", "801x24"),
        ("--surface=", "8193x24"),
        ("--surface=", "800x513"),
        ("--surface=", "800x301"),
        ("--outcomes=", seventeen.as_str()),
        ("--client=", "relative/client"),
    ] {
        let stderr = refuse(true, &with(flag, Some(value)));
        assert!(
            stderr.contains("invalid shell-gpu-content-proof parameters"),
            "{flag}{value}: {stderr}"
        );
    }
    for timeout in ["0", "120001"] {
        let mut args = with("--client=", Some("/absent/shell-client"));
        args.push(format!("--timeout-ms={timeout}"));
        assert!(refuse(true, &args).contains("invalid shell-gpu-content-proof parameters"));
    }
}

/// The render node is a FIFO: opening it would block the proof past the
/// bounded wait, so a prompt parameter refusal shows the node was not touched.
#[test]
fn invalid_parameters_never_open_the_render_node() {
    let root = std::env::temp_dir().join(format!("shell-gpu-proof-cli-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let node = root.join("renderD128");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        node.as_path(),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    let mut args = with("--surface=", Some("0x24"));
    args.push(format!("--render-node={}", node.display()));
    let stderr = refuse(true, &args);
    assert!(
        stderr.contains("invalid shell-gpu-content-proof parameters"),
        "{stderr}"
    );
    std::fs::remove_dir_all(root).unwrap();
}
