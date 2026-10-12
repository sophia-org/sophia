use sophia_protocol::namespace_view::*;

const RECORD: &str = "sophia-namespace\t1\t7\t1\nendpoint\troles/inspection\t9p2000.L\tinspection\t1\t31\t2\tavailable\n";

#[test]
fn discovery_round_trip_keeps_aliases_and_service_identity_separate() {
    let mut view = decode_discovery(RECORD.as_bytes()).unwrap();
    let mut alias = view.endpoints[0].clone();
    alias.path = "roles/status".into();
    view.endpoints.push(alias);
    assert_eq!(
        decode_discovery(&encode_discovery(&view).unwrap()).unwrap(),
        view
    );
    assert_eq!(view.endpoints[0].service, view.endpoints[1].service);
    assert!(
        !String::from_utf8(encode_discovery(&view).unwrap())
            .unwrap()
            .contains("grant")
    );
}

#[test]
fn discovery_refuses_partial_records_extra_fields_and_invalid_identities() {
    for end in 0..RECORD.len() {
        assert!(
            decode_discovery(&RECORD.as_bytes()[..end]).is_err(),
            "accepted prefix {end}"
        );
    }
    for (from, to) in [
        ("\t31\t", "\t0\t"),
        ("\t31\t", "\t+31\t"),
        ("\t31\t", "\t031\t"),
        ("\t31\t", "\t18446744073709551616\t"),
        ("available", "permitted"),
        ("namespace\t1", "namespace\t2"),
        ("inspection\t1", "inspection\t4294967296"),
    ] {
        assert!(
            decode_discovery(RECORD.replace(from, to).as_bytes()).is_err(),
            "accepted {to}"
        );
    }
    assert!(decode_discovery(format!("{RECORD}\n").as_bytes()).is_err());
    assert!(
        decode_discovery(
            RECORD
                .replace("available\n", "available\textra\n")
                .as_bytes()
        )
        .is_err()
    );
}

#[test]
fn discovery_paths_are_relative_unique_and_never_parents_of_endpoints() {
    for path in [
        "/absolute",
        "../up",
        "roles/../up",
        "roles//x",
        "roles/./x",
        "discovery",
        "discovery/x",
        "roles/x\r",
        "roles/x\0",
    ] {
        assert!(
            decode_discovery(RECORD.replace("roles/inspection", path).as_bytes()).is_err(),
            "accepted {path:?}"
        );
    }
    let line = RECORD.split_once('\n').unwrap().1;
    assert_eq!(
        decode_discovery(format!("{RECORD}{line}").as_bytes()),
        Err(DiscoveryError::Duplicate)
    );
    assert_eq!(
        decode_discovery(
            format!(
                "{}{}",
                RECORD.replace("\t7\t1\n", "\t7\t2\n"),
                line.replace("roles/inspection", "roles")
            )
            .as_bytes()
        ),
        Err(DiscoveryError::Path)
    );
}

#[test]
fn encoder_and_decoder_enforce_endpoint_and_byte_bounds() {
    let mut view = decode_discovery(RECORD.as_bytes()).unwrap();
    let mut item = view.endpoints[0].clone();
    view.endpoints.clear();
    for n in 0..64 {
        item.path = format!("p{n}");
        view.endpoints.push(item.clone());
    }
    assert!(encode_discovery(&view).is_ok());
    item.path = "one-more".into();
    view.endpoints.push(item);
    assert_eq!(encode_discovery(&view), Err(DiscoveryError::Limit));
    assert_eq!(
        decode_discovery(&vec![b'x'; MAX_DISCOVERY_BYTES + 1]),
        Err(DiscoveryError::Limit)
    );
}
