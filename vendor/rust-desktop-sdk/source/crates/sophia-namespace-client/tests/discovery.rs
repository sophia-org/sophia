use sophia_namespace_client::*;

const RECORD: &str = "sophia-namespace\t1\t12\t2\nendpoint\troles/inspection\t9p2000.L\tinspection\t1\t31\t2\tavailable\nendpoint\troles/status\t9p2000.L\tinspection\t1\t31\t2\twaiting\n";

#[test]
fn aliases_describe_one_service_without_claiming_permission() {
    let view = Discovery::parse(RECORD.as_bytes()).unwrap();
    assert_eq!(view.revision, 12);
    let a = view.endpoint("roles/inspection").unwrap();
    let b = view.endpoint("roles/status").unwrap();
    assert_eq!((a.service, a.generation), (b.service, b.generation));
    assert_ne!(a.availability, b.availability);
    assert!(view.endpoint("admin").is_none());
}

#[test]
fn partial_records_and_complete_line_truncations_refuse() {
    for n in 0..RECORD.len() {
        assert!(
            Discovery::parse(&RECORD.as_bytes()[..n]).is_err(),
            "accepted truncation {n}"
        );
    }
    for (from, to) in [
        ("\t12\t", "\t012\t"),
        ("\t31\t", "\t0\t"),
        ("\t31\t", "\t+31\t"),
        ("\t31\t", "\t18446744073709551616\t"),
        ("inspection\t1", "inspection\t4294967296"),
        ("namespace\t1", "namespace\t2"),
        ("available", "allowed"),
        ("9p2000.L", "ipc"),
        ("inspection\t1", "inspection x\t1"),
    ] {
        assert!(
            Discovery::parse(RECORD.replace(from, to).as_bytes()).is_err(),
            "accepted {to}"
        );
    }
    assert!(Discovery::parse(format!("{RECORD}\n").as_bytes()).is_err());
    assert!(Discovery::parse(b"sophia-namespace\t1\t1\t0\n").is_ok());
    assert!(Discovery::parse(&[0xff, b'\n']).is_err());
}

#[test]
fn ambiguous_or_escaping_paths_refuse() {
    for path in [
        "/absolute",
        "../other",
        "roles//x",
        "roles/./x",
        "roles/../x",
        "discovery",
        "discovery/x",
        "roles/x\0",
        "roles/x\u{85}",
        "roles/status",
        "roles",
        "roles/status/child",
    ] {
        assert!(
            Discovery::parse(RECORD.replace("roles/inspection", path).as_bytes()).is_err(),
            "accepted {path:?}"
        );
    }
    assert!(
        Discovery::parse(
            RECORD
                .replace("roles/inspection", &"x".repeat(256))
                .as_bytes()
        )
        .is_err()
    );
    assert!(Discovery::parse(&vec![b'x'; MAX_BYTES + 1]).is_err());
    let line = "endpoint\tp\tx11\tx11\t1\t1\t1\trecovering\n";
    for count in [64, 65] {
        let mut text = format!("sophia-namespace\t1\t1\t{count}\n");
        for n in 0..count {
            text.push_str(&line.replace("\tp\t", &format!("\tp{n}\t")));
        }
        assert_eq!(Discovery::parse(text.as_bytes()).is_ok(), count == 64);
    }
}
