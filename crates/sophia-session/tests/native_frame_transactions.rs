//! Startup opt-in refuses unsupported builds and malformed flags before launch.
#[path = "../src/live_session/config/frame_transactions.rs"]
mod frame_transactions;

fn parse(args: &[&str], native: bool) -> Result<bool, Box<dyn std::error::Error>> {
    frame_transactions::parse(
        &args.iter().map(|arg| (*arg).into()).collect::<Vec<_>>(),
        native,
    )
}

#[test]
fn sft_is_off_without_one_explicit_supported_native_flag() {
    const FLAG: &str = "--experimental-native-frame-transaction";
    assert!(!parse(&[], true).unwrap());
    assert!(!parse(&[], false).unwrap());
    assert!(parse(&[FLAG], false).is_err());
    assert!(parse(&[FLAG, FLAG], true).is_err());
    assert!(parse(&["--experimental-native-frame-transaction=true"], true).is_err());
    let requested = parse(&[FLAG], true);
    if cfg!(all(
        feature = "native-session",
        feature = "experimental-native-frame-transaction"
    )) {
        assert!(requested.unwrap());
    } else {
        assert!(requested.is_err());
    }
}

#[test]
fn sft_requires_an_independent_cpu_bar() {
    use sophia_config::{ShellComponentConfig, ShellComponentRole, ShellGpuMode};
    let mut bar = ShellComponentConfig {
        id: "bar".into(),
        role: ShellComponentRole::Bar,
        executable: "/unused".into(),
        config: None,
        gpu: ShellGpuMode::Denied,
        reservation: None,
        transport: Default::default(),
    };
    assert!(frame_transactions::validate_selection(&[]).is_err());
    assert!(frame_transactions::validate_selection(std::slice::from_ref(&bar)).is_ok());
    bar.gpu = ShellGpuMode::Direct;
    assert!(frame_transactions::validate_selection(std::slice::from_ref(&bar)).is_err());
    bar.gpu = ShellGpuMode::Denied;
    bar.role = ShellComponentRole::ApplicationLauncher;
    assert!(frame_transactions::validate_selection(std::slice::from_ref(&bar)).is_err());
    let mut descriptor = bar.clone();
    descriptor.role = ShellComponentRole::Descriptor;
    bar.role = ShellComponentRole::Bar;
    assert!(frame_transactions::validate_selection(&[bar, descriptor]).is_err());
}

#[cfg(feature = "experimental-native-frame-transaction")]
#[test]
fn owner_permission_requires_a_bar_and_precedes_reservation() {
    use sophia_config::ShellComponentRole;
    use sophia_session::shell_component_connections::{
        ComponentConnectionError, ShellComponentConnections,
    };
    let mut owner = ShellComponentConnections::new().unwrap();
    assert_eq!(
        owner.enable_frame_transactions(),
        Err(ComponentConnectionError::InvalidSelection)
    );
    let directory = std::env::temp_dir().join(format!("sft-opt-in-{}", std::process::id()));
    let slot = owner
        .add(
            "bar",
            ShellComponentRole::Bar,
            &directory,
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
    let key = owner.reserve_attempt(slot).unwrap();
    assert_eq!(
        owner.enable_frame_transactions(),
        Err(ComponentConnectionError::Busy)
    );
    owner.close(key).unwrap();
    assert!(owner.collect().quiescent());
    // Retirement is still an attempt; configuration is startup-only.
    assert_eq!(
        owner.enable_frame_transactions(),
        Err(ComponentConnectionError::Busy)
    );
    owner.finish_after_backend_drop(()).unwrap();
}

#[test]
fn terminal_only_requires_explicit_sft_and_a_single_valueless_flag() {
    const FLAG: &str = "--experimental-native-frame-terminal-only";
    let parse = |args: &[&str], sft| {
        frame_transactions::parse_terminal_only(
            &args.iter().map(|arg| (*arg).into()).collect::<Vec<_>>(),
            sft,
        )
    };
    assert!(!parse(&[], false).unwrap());
    assert!(!parse(&[], true).unwrap());
    assert!(parse(&[FLAG], false).is_err());
    assert!(parse(&[FLAG], true).unwrap());
    assert!(parse(&[FLAG, FLAG], true).is_err());
    assert!(parse(&["--experimental-native-frame-terminal-only=true"], true).is_err());
}

#[test]
fn lean_requires_terminal_only_and_a_single_valueless_flag() {
    const FLAG: &str = "--experimental-native-frame-lean";
    let args = |args: &[&str]| {
        args.iter()
            .map(|arg| (*arg).into())
            .collect::<Vec<String>>()
    };
    let parse =
        |flags: &[&str], terminal_only| frame_transactions::parse_lean(&args(flags), terminal_only);
    assert!(!parse(&[], false).unwrap());
    assert!(parse(&[FLAG], false).is_err());
    assert!(parse(&[FLAG], true).unwrap());
    assert!(parse(&[FLAG, FLAG], true).is_err());
    assert!(parse(&["--experimental-native-frame-lean=true"], true).is_err());
    let both = args(&["--experimental-native-frame-terminal-only", FLAG]);
    assert_eq!(
        frame_transactions::options(&both, true).unwrap(),
        (true, true)
    );
    assert!(frame_transactions::options(&both[1..], true).is_err());
    assert!(frame_transactions::options(&both, false).is_err());
}
