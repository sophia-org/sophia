use super::*;

#[test]
fn cpu_guest_uses_the_normal_startup_lifecycle_without_proof_polling() {
    // Isolate the native opt-in in a child: no process-global environment
    // mutation and no device opens. This test only parses configuration.
    if std::env::var_os("SOPHIA_CPU_CONFIG_CHILD").is_none() {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "live_session::tests::cpu_harness_config::cpu_guest_uses_the_normal_startup_lifecycle_without_proof_polling",
                "--nocapture",
            ])
            .env("SOPHIA_CPU_CONFIG_CHILD", "1")
            .env("SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE", "1")
            .output()
            .unwrap();
        assert!(child.status.success(), "{child:?}");
        return;
    }
    // Execute the actual recipe's argument construction, not a second copy.
    let init = include_str!("../../../../tools/qemu_guest_init.sh");
    let construction = init
        .split_once("    runtime_ms=$(((cpu_seconds + cpu_grace + 30) * 1000))")
        .unwrap()
        .1
        .split_once("    echo \"sophia_qemu_cpu schema=1 status=running")
        .unwrap()
        .0;
    let script = format!(
        "runtime_ms=100000; cpu_seconds=60; cpu_grace=10; cpu_rate=5; \
         cpu_target=next; cpu_mode=open; cpu_clients=1; cpu_size=head; cpu_damage=patch; \
         {construction}\nprintf '%s\\0' \"$@\""
    );
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", &script])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output.stderr);
    let args: Vec<_> = std::str::from_utf8(&output.stdout)
        .unwrap()
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect();
    assert_eq!(&args[..2], &["session", "run"]);
    let config = PersistentXtermSessionConfig::from_args(&args[2..]).unwrap();
    assert!(config.normal_session);
    assert!(config.exit_when_startup_exits);
    assert!(!config.startup_proof_requested());
    assert!(config.client.is_none());
    assert_eq!(config.applications.startup, ["cpu"]);
    let app = &config.applications.applications["cpu"];
    assert_eq!(
        app.executable,
        std::path::Path::new("/usr/bin/present_cpu_workload")
    );
    assert!(app.arguments.iter().any(|arg| arg == "--sample-pid=parent"));
    assert!(app.arguments.iter().any(|arg| arg == "--seconds=60"));
    for argument in ["--clients=1", "--size=head", "--damage=patch"] {
        assert!(app.arguments.iter().any(|arg| arg == argument));
    }
}
