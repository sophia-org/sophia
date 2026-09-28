use super::{parse_schema, render_rust_rows, wm_rows};

const FILES: &str = include_str!("../../../../protocol/sophia-wm-files-v1.kdl");
const SOCKET: &str = include_str!("../../../../protocol/sophia-wm-v1.kdl");

#[test]
fn file_rows_generate_without_a_socket_schema_or_envelope() {
    let rows = wm_rows::parse(FILES).unwrap();
    assert_eq!(rows.records.len(), 8);
    assert!(!rows.extension_records.is_empty());
    let rust = render_rust_rows(&rows);
    assert!(rust.contains("sophia-wm-files-v1.kdl (row-layouts)"));
    assert!(rust.contains("BinaryCodecError"));
    assert!(rust.contains("crate::byte_cursor"));
    for retired in [
        "IpcCodecError",
        "ipc::",
        "encode_frame",
        "decode_frame",
        "TransactionId",
    ] {
        assert!(
            !rust.contains(retired),
            "unexpected envelope dependency: {retired}"
        );
    }
}

#[test]
fn frozen_socket_layouts_match_the_file_contract() {
    wm_rows::check_legacy(
        &wm_rows::parse(FILES).unwrap(),
        &parse_schema(SOCKET).unwrap(),
    )
    .unwrap();
}

#[test]
fn drift_in_each_row_contract_dimension_is_refused() {
    let socket = parse_schema(SOCKET).unwrap();
    for (before, after) in [
        ("max-outputs=16", "max-outputs=15"),
        ("interface-revision=3", "interface-revision=4"),
        (
            "capability \"actions\" bit=1",
            "capability \"actions\" bit=20",
        ),
        (
            "outcome \"committed\" value=1",
            "outcome \"committed\" value=6",
        ),
        (
            "field \"focus_index\" type=\"u32\"",
            "field \"focus_index\" type=\"u64\"",
        ),
        ("gate=\"launch_placement\"", "gate=\"actions\""),
    ] {
        assert!(FILES.contains(before));
        let changed = wm_rows::parse(&FILES.replacen(before, after, 1)).unwrap();
        assert!(
            wm_rows::check_legacy(&changed, &socket).is_err(),
            "accepted {after}"
        );
    }
}

#[test]
fn missing_duplicate_and_unknown_layout_declarations_fail() {
    for changed in [
        FILES.replace("row-layouts interface", "retired-layouts interface"),
        FILES.replace(
            "    row-layouts interface",
            "    row-layouts {}\n    row-layouts interface",
        ),
        FILES.replace("capability \"bindings\"", "unknown \"bindings\""),
        FILES.replace("gate=\"launch_placement\"", "gate=\"not_a_capability\""),
    ] {
        assert!(wm_rows::parse(&changed).is_err());
    }
}
