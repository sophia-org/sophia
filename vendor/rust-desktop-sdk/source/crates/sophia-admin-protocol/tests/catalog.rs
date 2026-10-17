use sophia_admin_protocol::{Command, Owner, decode_catalog};

#[test]
fn literal_catalog_and_borrowed_lookup() {
    let wire = b"v1 catalog 12 2\nv1 command policy 8:next tab\nv1 command session 10:restart-wm\n";
    let catalog = decode_catalog(wire).unwrap();
    assert_eq!(catalog.generation(), 12);
    assert_eq!(
        catalog.commands(),
        &[
            Command {
                owner: Owner::Policy,
                name: "next tab"
            },
            Command {
                owner: Owner::Session,
                name: "restart-wm"
            }
        ]
    );
    assert_eq!(
        catalog.commands()[1].name.as_ptr(),
        wire[wire.len() - 11..].as_ptr()
    );
    assert!(catalog.contains(Command {
        owner: Owner::Policy,
        name: "next tab"
    }));
    assert!(!catalog.contains(Command {
        owner: Owner::Session,
        name: "next tab"
    }));
    assert!(!catalog.contains(Command {
        owner: Owner::Policy,
        name: "missing"
    }));
    assert!(
        decode_catalog(b"v1 catalog 18446744073709551615 0\n")
            .unwrap()
            .commands()
            .is_empty()
    );
    for end in 0..wire.len() {
        assert!(decode_catalog(&wire[..end]).is_err());
    }
}

#[test]
fn full_catalog_and_policy_limit() {
    let mut wire = String::from("v1 catalog 1 259\n");
    for i in 0..256 {
        wire.push_str(&format!("v1 command policy 4:p{i:03}\n"));
    }
    wire.push_str("v1 command session 6:logout\nv1 command session 14:reload-profile\nv1 command session 10:restart-wm\n");
    let catalog = decode_catalog(wire.as_bytes()).unwrap();
    assert_eq!(catalog.commands().len(), 259);
    assert!(catalog.contains(Command {
        owner: Owner::Policy,
        name: "p255"
    }));
    let mut excessive = String::from("v1 catalog 1 257\n");
    for i in 0..257 {
        excessive.push_str(&format!("v1 command policy 4:p{i:03}\n"));
    }
    assert!(decode_catalog(excessive.as_bytes()).is_err());
    for size in [128, 129] {
        let wire = format!(
            "v1 catalog 1 1\nv1 command policy {size}:{}\n",
            "a".repeat(size)
        );
        assert_eq!(decode_catalog(wire.as_bytes()).is_ok(), size == 128);
    }
}

#[test]
fn reject_whole_invalid_catalog() {
    for bad in [
        "v1 catalog 0 0\n",
        "v1 catalog 01 0\n",
        "v1 catalog 1 00\n",
        "v1 catalog 1 260\n",
        "v1 catalog 1 18446744073709551616\n",
        "v1 catalog 1 0 \n",
        "v1 catalog 1 1\n",
        "v1 catalog 1 0\nv1 command policy 1:a\n",
        "v1 catalog 1 2\nv1 command policy 1:a\nv1 command policy 1:a\n",
        "v1 catalog 1 2\nv1 command policy 1:b\nv1 command policy 1:a\n",
        "v1 catalog 1 2\nv1 command session 6:logout\nv1 command policy 1:a\n",
        "v1 catalog 1 1\nv1 command session 6:launch\n",
        "v1 catalog 1 1\nv1 command other 1:a\n",
        "v1 catalog 1 1\nv1 command policy 0:\n",
        "v1 catalog 1 1\nv1 command policy 1:ab\n",
        "v1 catalog 1 1\nv1 command policy 2: a\n",
        "v1 catalog 1 1\nv1 command policy 2:é\n",
        "v1 catalog 1 0\n\n",
    ] {
        assert!(decode_catalog(bad.as_bytes()).is_err(), "{bad:?}");
    }
    assert!(decode_catalog(&[b'x'; 39433]).is_err());
}
