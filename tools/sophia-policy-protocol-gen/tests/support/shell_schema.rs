use super::shell_schema;
const SCHEMA: &str = include_str!("../../../../protocol/sophia-shell-v1.kdl");
#[test]
fn complete_revision_seven_and_native_records_are_required() {
    shell_schema::validate(SCHEMA).unwrap();
    for (from, to) in [
        ("interface-revision=7", "interface-revision=6"),
        ("kind=195", "kind=298"),
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
