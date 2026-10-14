use sophia_config::namespace_policy::*;
use sophia_config::{ConfigGeneration, CoreConfigDelta, parse_core_config};

fn parse(body: &str) -> Result<sophia_config::CoreConfigSnapshot, sophia_config::ConfigParseError> {
    parse_core_config(
        format!("schema 2\nsession {{ namespace-policy {{\n{body}\n}}; }}\n").as_bytes(),
        ConfigGeneration::INITIAL,
    )
}

#[test]
fn identities_select_policy_and_metadata_never_grants_it() {
    let config = parse(
        r#"
        application "registered:browser" recipe="restricted"
        application "desktop:org.example.Browser.desktop" recipe="restricted"
        recipe "restricted" profile="confined-x" group=7 { request "clipboard"; }
        recipe "tools" profile="observer" developer-launch=#true
    "#,
    )
    .unwrap();
    let policy = &config.session.namespace_policy;
    for identity in ["registered:browser", "desktop:org.example.Browser.desktop"] {
        let selected = policy.select_application(identity, None).unwrap().unwrap();
        assert_eq!(
            selected.profile,
            NamespaceRecipeProfile::ConfinedX { group: 7 }
        );
        assert_eq!(selected.request, vec![RecipePortalCapability::Clipboard]);
        assert!(selected.publish.is_empty());
        assert_eq!(
            policy.select_application(identity, Some("restricted")),
            Ok(Some(selected))
        );
        assert_eq!(
            policy.select_application(identity, Some("tools")),
            Err(NamespaceSelectionError::UnapprovedRequest)
        );
    }
    assert_eq!(
        policy.select_application("desktop:unmapped.desktop", None),
        Ok(None)
    );
    assert_eq!(
        policy.select_application("desktop:unmapped.desktop", Some("restricted")),
        Err(NamespaceSelectionError::UnapprovedRequest)
    );
    assert_eq!(
        policy.select_application("/usr/bin/browser", None),
        Err(NamespaceSelectionError::InvalidIdentity)
    );
    let observer = policy.select_developer("tools").unwrap();
    assert_eq!(
        observer.request,
        vec![RecipePortalCapability::ScreenCapture]
    );
    assert!(observer.publish.is_empty());
    assert!(observer.capture_outputs.is_empty());
    assert_eq!(
        policy.select_developer("restricted"),
        Err(NamespaceSelectionError::DeveloperLaunchDenied)
    );
    assert_eq!(
        policy.select_developer("unknown"),
        Err(NamespaceSelectionError::UnknownRecipe)
    );
}

#[test]
fn capture_approval_is_explicit_output_scoped_and_restart_bound() {
    let old = parse(r#"recipe "tools" profile="observer" developer-launch=#true"#).unwrap();
    let approved = parse(
        r#"recipe "tools" profile="observer" developer-launch=#true {
        capture-output "DP-1"
        capture-output "pci-0000:03:00.0/HDMI-A-1"
    }"#,
    )
    .unwrap();
    assert_eq!(
        approved.session.namespace_policy.recipes["tools"].capture_outputs,
        ["DP-1", "pci-0000:03:00.0/HDMI-A-1"]
    );
    assert!(CoreConfigDelta::between(&old, &approved).restart_required);
    for bad in [
        r#"recipe "tools" profile="shared-x" { capture-output "DP-1"; }"#,
        r#"recipe "tools" profile="confined-x" group=7 { capture-output "DP-1"; }"#,
        r#"recipe "tools" profile="observer" { capture-output "*"; }"#,
        r#"recipe "tools" profile="observer" { capture-output ""; }"#,
        r#"recipe "tools" profile="observer" { capture-output "DP-1"; capture-output "DP-1"; }"#,
        r#"recipe "tools" profile="observer" { capture-output "DP-1" grant=#true; }"#,
    ] {
        assert!(parse(bad).is_err(), "accepted {bad}");
    }
    let entries = (0..MAX_CAPTURE_OUTPUTS)
        .map(|i| format!("capture-output \"DP-{i}\";\n"))
        .collect::<String>();
    assert!(
        parse(&format!(
            "recipe \"tools\" profile=\"observer\" {{ {entries} }}"
        ))
        .is_ok()
    );
    assert!(
        parse(&format!(
            "recipe \"tools\" profile=\"observer\" {{ {entries} capture-output \"extra\"; }}"
        ))
        .is_err()
    );
}

#[test]
fn policy_refuses_ambiguous_or_escalating_configuration() {
    for bad in [
        r#"recipe "x" profile="shared-x" group=7"#,
        r#"recipe "x" profile="confined-x""#,
        r#"recipe "x" profile="confined-x" group=0"#,
        r#"recipe "x" profile="operator""#,
        r#"recipe "x" profile="observer" { publish "screen-capture"; }"#,
        r#"recipe "x" profile="observer" { request "clipboard"; }"#,
        r#"recipe "x" profile="shared-x" { request "inject"; }"#,
        r#"recipe "x" profile="shared-x" { request "clipboard"; request "clipboard"; }"#,
        r#"recipe "x" profile="shared-x"; recipe "x" profile="shared-x""#,
        r#"application "registered:a" recipe="missing""#,
        r#"application "desktop:../a.desktop" recipe="x"; recipe "x" profile="shared-x""#,
        r#"application "/bin/a" recipe="x"; recipe "x" profile="shared-x""#,
        r#"application "registered:a" recipe="x"; application "registered:a" recipe="x"; recipe "x" profile="shared-x""#,
        r#"recipe "x" profile="shared-x" developer-launch="true""#,
        r#"recipe "x" profile="shared-x" developer-launch=#true"#,
        r#"recipe "x" profile="shared-x" authority="admin""#,
    ] {
        assert!(parse(bad).is_err(), "accepted {bad}");
    }
}

#[test]
fn naming_sources_stay_under_the_trusted_configuration_root() {
    for bad in ["/tmp/x", "../x", "a/../x", "a//x", "./x", "a/", ""] {
        assert!(
            parse(&format!(
                "recipe \"x\" profile=\"shared-x\" source=\"{bad}\""
            ))
            .is_err(),
            "accepted {bad}"
        );
    }
    let valid = parse(r#"recipe "x" profile="shared-x" source="recipes/x.ns""#).unwrap();
    assert_eq!(
        valid.session.namespace_policy.recipes["x"]
            .source
            .as_deref(),
        Some("recipes/x.ns")
    );
}

#[test]
fn recipe_and_identity_counts_are_bounded_and_reload_requires_restart() {
    let mut body = String::new();
    for i in 0..MAX_NAMESPACE_RECIPES {
        body.push_str(&format!("recipe \"r{i}\" profile=\"shared-x\";\n"));
    }
    for i in 0..MAX_NAMESPACE_APPLICATIONS {
        body.push_str(&format!(
            "application \"desktop:a{i}.desktop\" recipe=\"r0\";\n"
        ));
    }
    let config = parse(&body).unwrap();
    assert!(parse(&format!("{body} recipe \"extra\" profile=\"shared-x\";\n")).is_err());
    assert!(
        parse(&format!(
            "{body} application \"desktop:extra.desktop\" recipe=\"r0\";\n"
        ))
        .is_err()
    );
    let default = parse("").unwrap();
    assert_eq!(
        default.session.namespace_policy,
        NamespacePolicyConfig::default()
    );
    assert!(CoreConfigDelta::between(&default, &config).restart_required);
    assert!(!CoreConfigDelta::between(&config, &config).restart_required);
}
