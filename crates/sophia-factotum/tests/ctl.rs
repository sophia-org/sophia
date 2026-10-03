//! `ctl` and `proto` against 9front's `ctlwrite`, `keylist` and `protolist`.

use sophia_factotum::ctl::{self, CtlEffect, CtlError, ListCursor, ListTooSmall};
use sophia_factotum::keyring::Keyring;

fn list(ring: &Keyring) -> Vec<String> {
    let mut cursor = ListCursor::default();
    let mut lines = Vec::new();
    loop {
        let line = cursor.read_key(ring, 4096).unwrap();
        if line.is_empty() {
            return lines;
        }
        lines.push(String::from_utf8(line).unwrap());
    }
}

#[test]
fn a_key_lists_with_its_secrets_as_queries() {
    let mut ring = Keyring::new();
    ctl::write(&mut ring, "key proto=pass user=tb !password=secret\n").unwrap();
    ctl::write(&mut ring, "key proto=pass user=other").unwrap();
    assert_eq!(
        list(&ring),
        [
            "key proto=pass user=tb !password?\n",
            // No private attributes: 9front's trailing space stays.
            "key proto=pass user=other \n",
        ]
    );
}

#[test]
fn a_key_with_the_same_public_attributes_replaces_the_old_one() {
    let mut ring = Keyring::new();
    ctl::write(&mut ring, "key proto=pass user=tb !password=old").unwrap();
    ctl::write(&mut ring, "key proto=pass user=tb !password=new").unwrap();
    assert_eq!(ring.keys().count(), 1);
    assert_eq!(
        ring.keys().next().unwrap().private.value("!password"),
        Some("new")
    );
}

#[test]
fn malformed_and_refused_writes_name_9fronts_errors() {
    let mut ring = Keyring::new();
    assert_eq!(ctl::write(&mut ring, ""), Ok(CtlEffect::None));
    assert_eq!(ctl::write(&mut ring, "# a comment"), Ok(CtlEffect::None));
    assert_eq!(ctl::write(&mut ring, "debug"), Ok(CtlEffect::ToggleDebug));
    assert_eq!(ctl::write(&mut ring, "frob"), Err(CtlError::UnknownVerb));
    assert_eq!(
        ctl::write(&mut ring, "key proto=pass\nkey proto=pass"),
        Err(CtlError::MultilineWrite)
    );
    assert_eq!(
        ctl::write(&mut ring, "key user=x"),
        Err(CtlError::KeyWithoutProtos)
    );
    let refused = ctl::write(&mut ring, "key proto=nope user=x").unwrap_err();
    assert_eq!(refused.to_string(), "unknown proto nope");
    let refused = ctl::write(&mut ring, "key proto=pam user=x").unwrap_err();
    assert_eq!(refused.to_string(), "proto pam doesn't take keys");
    assert_eq!(ring.keys().count(), 0);
}

#[test]
fn delkey_refuses_a_private_value_and_reports_no_match() {
    let mut ring = Keyring::new();
    ctl::write(&mut ring, "key proto=pass user=tb !password=secret").unwrap();
    assert_eq!(
        ctl::write(&mut ring, "delkey proto=pass !password=secret"),
        Err(CtlError::PrivatePattern),
        "a secret cannot be probed by guessing it"
    );
    assert_eq!(
        ctl::write(&mut ring, "delkey proto=pass !password?"),
        Ok(CtlEffect::None)
    );
    assert_eq!(
        ctl::write(&mut ring, "delkey proto=pass"),
        Err(CtlError::NoKeysToDelete)
    );
}

#[test]
fn a_list_line_must_fit_with_a_byte_to_spare() {
    let mut ring = Keyring::new();
    ctl::write(&mut ring, "key proto=pass user=tb !password=secret").unwrap();
    let line = "key proto=pass user=tb !password?\n";
    let mut cursor = ListCursor::default();
    assert_eq!(cursor.read_key(&ring, line.len()), Err(ListTooSmall));
    assert_eq!(
        cursor.read_key(&ring, line.len() + 1).unwrap(),
        line.as_bytes()
    );
    assert!(cursor.read_key(&ring, 4096).unwrap().is_empty());
}

#[test]
fn the_proto_file_lists_the_table_in_order() {
    let mut cursor = ListCursor::default();
    assert_eq!(cursor.read_proto(3), Err(ListTooSmall));
    assert_eq!(cursor.read_proto(64).unwrap(), b"pass\n");
    assert_eq!(cursor.read_proto(64).unwrap(), b"pam\n");
    assert!(cursor.read_proto(64).unwrap().is_empty());
}
