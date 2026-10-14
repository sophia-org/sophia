use sophia_protocol::admin_files::*;
use sophia_protocol::control_records::{ControlCatalog, ControlCommand, ControlOwner};

#[test]
fn literal_discovery_preserves_identity_and_the_current_command_catalog() {
    let info = AdminInfo {
        session_id: [0, 42],
        connection: 7,
        command_timeout_ms: 10000,
        frame_timeout_ms: 2000,
        idle_timeout_ms: 60000,
    };
    let wire = b"v1 info 0 42 7 10000 2000 60000\n";
    assert_eq!(decode_admin_info(wire).unwrap(), info);
    assert_eq!(encode_admin_info(&info).unwrap(), wire);
    let wire = b"v1 catalog 12 3\nv1 command policy 8:next tab\nv1 command session 6:logout\nv1 command session 10:restart-wm\n";
    let catalog = ControlCatalog {
        generation: 12,
        commands: vec![
            ControlCommand {
                owner: ControlOwner::Policy,
                name: "next tab".into(),
            },
            ControlCommand {
                owner: ControlOwner::Session,
                name: "logout".into(),
            },
            ControlCommand {
                owner: ControlOwner::Session,
                name: "restart-wm".into(),
            },
        ],
    };
    assert_eq!(decode_admin_catalog(wire).unwrap(), catalog);
    assert_eq!(encode_admin_catalog(&catalog).unwrap(), wire);
    assert_eq!(
        decode_admin_catalog(b"v1 catalog 1 0\n").unwrap().commands,
        vec![]
    );
}

#[test]
fn status_distinguishes_no_request_pending_and_uncertain_effects() {
    let ready = AdminStatus::Ready { connection: 7 };
    assert_eq!(decode_admin_status(b"v1 ready 7\n").unwrap(), ready);
    assert_eq!(encode_admin_status(&ready).unwrap(), b"v1 ready 7\n");
    let pending = AdminStatus::Pending {
        connection: 7,
        request: 9,
        generation: 12,
    };
    assert_eq!(
        decode_admin_status(b"v1 pending 7 9 12\n").unwrap(),
        pending
    );
    assert_eq!(
        encode_admin_status(&pending).unwrap(),
        b"v1 pending 7 9 12\n"
    );
    let wire = b"v1 outcome 7 9 12 indeterminate 19:completion not seen\n";
    let status = decode_admin_status(wire).unwrap();
    let AdminStatus::Outcome(ref value) = status else {
        panic!("expected outcome")
    };
    assert_eq!(
        value.outcome,
        sophia_protocol::ControlOutcome::Indeterminate
    );
    assert_eq!(encode_admin_status(&status).unwrap(), wire);
    for wire in [
        "v1 ready 0\n",
        "v1 ready 7 extra\n",
        "v1 ready 7\n\n",
        "v1 pending 7 0 12\n",
        "v1 pending 7 9 0\n",
        "v1 pending 07 9 12\n",
        "v1 pending 7 9 12\r\n",
        "v2 ready 7\n",
        "v1 ready  7\n",
    ] {
        assert!(decode_admin_status(wire.as_bytes()).is_err(), "{wire:?}");
    }
    assert!(encode_admin_status(&AdminStatus::Ready { connection: 0 }).is_err());
    assert!(
        encode_admin_status(&AdminStatus::Pending {
            connection: 7,
            request: 0,
            generation: 12
        })
        .is_err()
    );
}

#[test]
fn catalog_counts_and_names_cannot_expand_or_reorder_command_authority() {
    for wire in [
        "v1 catalog 0 0\n",
        "v2 catalog 1 0\n",
        "v1 catalog 1 01\n",
        "v1 catalog 1 260\n",
        "v1 catalog 1 18446744073709551616\n",
        "v1 catalog 1 1\n",
        "v1 catalog 1 0\n\n",
        "v1 catalog 1 0\nv1 command session 6:logout\n",
        "v1 catalog 1 1\nv1 command session 6:logout",
        "v1 catalog 1 1\nv1 command session 7:logout\n",
        "v1 catalog 1 1\nv1 command session 4:exec\n",
        "v1 catalog 1 1\nv1 command root 6:logout\n",
        "v1 catalog 1 1\nv1 command policy 6:a;b;sh\n",
        "v1 catalog 1 2\nv1 command session 6:logout\nv1 command session 6:logout\n",
        "v1 catalog 1 2\nv1 command session 10:restart-wm\nv1 command session 6:logout\n",
    ] {
        assert!(decode_admin_catalog(wire.as_bytes()).is_err(), "{wire:?}");
    }
    assert!(decode_admin_catalog(&vec![b' '; ADMIN_CATALOG_MAX_BYTES + 1]).is_err());
    assert!(decode_admin_catalog(b"v1 catalog 1 1\nv1 command policy 1:\xff\n").is_err());
    let commands = (0..256)
        .map(|i| ControlCommand {
            owner: ControlOwner::Policy,
            name: format!("{i:03}{}", "x".repeat(125)),
        })
        .chain(
            ["logout", "reload-profile", "restart-wm"].map(|name| ControlCommand {
                owner: ControlOwner::Session,
                name: name.into(),
            }),
        )
        .collect();
    let mut catalog = ControlCatalog {
        generation: u64::MAX,
        commands,
    };
    let wire = encode_admin_catalog(&catalog).unwrap();
    assert!(wire.len() <= ADMIN_CATALOG_MAX_BYTES);
    assert_eq!(decode_admin_catalog(&wire).unwrap(), catalog);
    catalog.commands[0].name.push('x');
    assert!(encode_admin_catalog(&catalog).is_err());
    catalog.commands[0].name.pop();
    catalog.commands.insert(
        256,
        ControlCommand {
            owner: ControlOwner::Policy,
            name: "zzz".into(),
        },
    );
    assert!(encode_admin_catalog(&catalog).is_err());
}

#[test]
fn discovery_refuses_unusable_identity_limits_and_extra_fields() {
    for wire in [
        "v1 info 0 0 7 10000 2000 60000\n",
        "v1 info 0 42 0 10000 2000 60000\n",
        "v1 info 0 42 7 0 2000 60000\n",
        "v1 info 0 42 7 10000 0 60000\n",
        "v1 info 0 42 7 10000 2000 0\n",
        "v1 info 0 42 7 4294967296 2000 60000\n",
        "v1 info 0 42 7 10000 2000 60000 extra\n",
        "v1 info 0 042 7 10000 2000 60000\n",
        "v1 info 0 42 7 10000 2000 60000",
        "v1 info 0 42 7 10000 2000 60000\nnext\n",
    ] {
        assert!(decode_admin_info(wire.as_bytes()).is_err(), "{wire:?}");
    }
    let mut info = AdminInfo {
        session_id: [u64::MAX; 2],
        connection: u64::MAX,
        command_timeout_ms: u32::MAX,
        frame_timeout_ms: u32::MAX,
        idle_timeout_ms: u32::MAX,
    };
    let bytes = encode_admin_info(&info).unwrap();
    assert!(bytes.len() <= ADMIN_LINE_MAX_BYTES);
    assert_eq!(decode_admin_info(&bytes).unwrap(), info);
    info.session_id = [0, 0];
    assert!(encode_admin_info(&info).is_err());
}
