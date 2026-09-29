use super::{render_record_golden, render_rust_rows, wm_rows};

const FILES: &str = include_str!("../../../../protocol/sophia-wm-files-v1.kdl");

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
fn file_rows_preserve_the_independent_record_corpus() {
    let rows = wm_rows::parse(FILES).unwrap();
    assert_eq!(
        render_record_golden(&rows).unwrap(),
        include_str!("../../../../protocol/golden/sophia-wm-v1.records")
    );
}

#[test]
fn invalid_file_row_shapes_are_refused() {
    for (before, after) in [
        ("interface-major=1", "interface-major=0"),
        ("kind=1 max=16", "kind=0 max=16"),
        ("kind=1 max=16", "kind=65280 max=16"),
        ("kind=1 max=16", "kind=1 max=0"),
        (
            "field \"focus_index\" type=\"u32\" sample=3",
            "field \"focus_index\" type=\"u16\" sample=65536",
        ),
        ("reserved=#true sample=0", "reserved=#true sample=1"),
        ("count=128", "count=127"),
    ] {
        assert!(FILES.contains(before), "missing fixture input: {before}");
        assert!(
            wm_rows::parse(&FILES.replacen(before, after, 1)).is_err(),
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
