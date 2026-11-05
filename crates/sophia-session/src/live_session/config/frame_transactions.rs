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
