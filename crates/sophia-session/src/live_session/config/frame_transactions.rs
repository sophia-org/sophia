//! Explicit startup permission; no environment or profile default enables SFT.

pub(super) fn parse(args: &[String], native: bool) -> Result<bool, Box<dyn std::error::Error>> {
    const FLAG: &str = "--experimental-native-frame-transaction";
    let count = args.iter().filter(|arg| arg.as_str() == FLAG).count();
    if count > 1
        || args.iter().any(|arg| {
            arg.strip_prefix(FLAG)
                .is_some_and(|suffix| suffix.starts_with('='))
        })
    {
        return Err(format!("{FLAG} is a single, valueless flag").into());
    }
    if count == 0 {
        return Ok(false);
    }
    if !native
        || !cfg!(all(
            feature = "native-session",
            feature = "experimental-native-frame-transaction"
        ))
    {
        return Err(format!("{FLAG} requires --native-scanout and a build with native-session and experimental-native-frame-transaction").into());
    }
    Ok(true)
}

pub(super) fn validate_selection(
    selected: &[sophia_config::ShellComponentConfig],
) -> Result<(), Box<dyn std::error::Error>> {
    use sophia_config::{ShellComponentRole, ShellGpuMode};
    if selected
        .iter()
        .any(|entry| entry.role == ShellComponentRole::Descriptor)
        || !selected
            .iter()
            .any(|entry| entry.role == ShellComponentRole::Bar && entry.gpu == ShellGpuMode::Denied)
    {
        return Err("SFT requires an independent bar with gpu denied".into());
    }
    Ok(())
}

pub(super) fn parse_terminal_only(
    args: &[String],
    frame_transactions: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    parse_option(
        args,
        "--experimental-native-frame-terminal-only",
        "--experimental-native-frame-transaction",
        frame_transactions,
    )
}

/// Lean commits (bit 20) need terminal-only (bit 19), as on the wire.
pub(super) fn parse_lean(
    args: &[String],
    terminal_only: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    parse_option(
        args,
        "--experimental-native-frame-lean",
        "--experimental-native-frame-terminal-only",
        terminal_only,
    )
}

/// `(terminal_only, lean)` for the owner's SFT permission.
pub(super) fn options(
    args: &[String],
    frame_transactions: bool,
) -> Result<(bool, bool), Box<dyn std::error::Error>> {
    let terminal_only = parse_terminal_only(args, frame_transactions)?;
    Ok((terminal_only, parse_lean(args, terminal_only)?))
}

fn parse_option(
    args: &[String],
    flag: &str,
    requires: &str,
    satisfied: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let count = args.iter().filter(|arg| arg.as_str() == flag).count();
    if count > 1
        || args.iter().any(|arg| {
            arg.strip_prefix(flag)
                .is_some_and(|suffix| suffix.starts_with('='))
        })
    {
        return Err(format!("{flag} is a single, valueless flag").into());
    }
    if count != 0 && !satisfied {
        return Err(format!("{flag} requires {requires}").into());
    }
    Ok(count == 1)
}
