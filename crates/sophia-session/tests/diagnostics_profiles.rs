use sophia_session::diagnostics::reduced_record;

#[test]
fn desktop_profile_capture_retains_every_supported_source_mode() {
    for mode in [
        "user",
        "system",
        "explicit",
        "packaged-fallback",
        "packaged-promotion",
    ] {
        let record = format!(
            "sophia_live_desktop_profile schema=1 status=loaded mode={mode} generation=4 digest={}",
            "a".repeat(64)
        );
        assert_eq!(
            reduced_record(&format!("{record} path=/private/profile title=secret")),
            Some(record),
        );
    }
}

#[test]
fn desktop_profile_modes_do_not_admit_other_values_or_other_record_fields() {
    // Values retained elsewhere in diagnostics are not profile-source modes.
    for mode in ["private-label", "true", "loaded", "native", ""] {
        assert_eq!(
            reduced_record(&format!("sophia_live_desktop_profile schema=1 mode={mode}")),
            Some("sophia_live_desktop_profile schema=1".into()),
        );
    }
    assert_eq!(
        reduced_record("sophia_other mode=packaged-promotion"),
        Some("sophia_other".into()),
    );
}
