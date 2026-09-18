use super::shell_schema;
const SCHEMA: &str = include_str!("../../../../protocol/sophia-shell-v1.kdl");
#[test]
fn complete_revision_eight_and_native_records_are_required() {
    shell_schema::validate(SCHEMA).unwrap();
    for (from, to) in [
        ("interface-revision=8", "interface-revision=7"),
        ("kind=195", "kind=298"),
        ("kind=200", "kind=299"),
        (
            "NativeLauncherFocus\" kind=191 direction=\"session-to-shell\"",
            "NativeLauncherFocus\" kind=191 direction=\"shell-to-session\"",
        ),
        ("NativeLauncherClosed\"", "OmittedNativeLauncherClosed\""),
    ] {
        assert_eq!(SCHEMA.matches(from).count(), 1);
        assert!(
            shell_schema::validate(&SCHEMA.replace(from, to)).is_err(),
            "{from}"
        );
    }
}
