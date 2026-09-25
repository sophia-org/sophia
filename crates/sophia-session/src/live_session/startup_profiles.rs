fn record_loaded_session_profiles(config: &PersistentXtermSessionConfig) -> Result<(), Box<dyn std::error::Error>> {
    if let Ok(profile_mode) = std::env::var("SOPHIA_HAGIA_PROFILE_MODE") {
        if !matches!(
            profile_mode.as_str(),
            "user" | "system" | "explicit" | "packaged-fallback" | "packaged-promotion"
        ) {
            return Err("SOPHIA_HAGIA_PROFILE_MODE has an invalid value".into());
        }
        let profile_sha256 = std::env::var("SOPHIA_DESKTOP_PROFILE_SHA256")?;
        if profile_sha256.len() != 64
            || !profile_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("SOPHIA_DESKTOP_PROFILE_SHA256 must be lowercase SHA-256".into());
        }
        crate::session_println!(
            "sophia_live_desktop_profile schema=1 status=loaded mode={} generation={} digest={} root_sha256={} sources={}",
            profile_mode,
            config.desktop_profile.generation.raw(),
            config.desktop_profile.digest,
            profile_sha256,
            config.desktop_profile.sources.len()
        );
    }
    crate::session_println!(
        "sophia_session_profile schema=1 status=loaded role=desktop generation={} digest={}",
        config.desktop_profile.generation.raw(),
        config.desktop_profile.digest
    );
    crate::session_println!(
        "sophia_session_profile schema=1 status=loaded role=core generation={} digest={}",
        config.core_config_state.active().generation.raw(),
        config.core_config_state.active().digest
    );
    Ok(())
}
