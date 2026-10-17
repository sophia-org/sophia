use sophia_admin_protocol::{Invocation, Owner, decode_invocation, encode_invocation};

#[test]
fn literal_borrow_and_encoding() {
    let bytes = b"v1 invoke 7 9 12 policy 8:next tab\n";
    let value = decode_invocation(bytes).unwrap();
    assert_eq!(
        value,
        Invocation {
            connection: 7,
            request: 9,
            generation: 12,
            owner: Owner::Policy,
            name: "next tab"
        }
    );
    assert_eq!(value.name.as_ptr(), bytes[26..].as_ptr());
    let mut out = [0xaa; 512];
    assert_eq!(encode_invocation(value, &mut out), Ok(bytes.len()));
    assert_eq!(&out[..bytes.len()], bytes);
    assert_eq!(out[bytes.len()], 0xaa);
    for size in 0..bytes.len() {
        out.fill(0xaa);
        assert!(encode_invocation(value, &mut out[..size]).is_err());
        assert!(out.iter().all(|&b| b == 0xaa));
        assert!(decode_invocation(&bytes[..size]).is_err());
    }
}

#[test]
fn boundaries_and_invalid_input() {
    let name = "a".repeat(128);
    let value = Invocation {
        connection: u64::MAX,
        request: u64::MAX,
        generation: u64::MAX,
        owner: Owner::Session,
        name: &name,
    };
    let mut out = [0; 512];
    let n = encode_invocation(value, &mut out).unwrap();
    assert_eq!(decode_invocation(&out[..n]).unwrap(), value);
    for bad in [
        "v1 invoke 0 9 12 policy 8:next tab\n",
        "v1 invoke 7 0 12 policy 8:next tab\n",
        "v1 invoke 7 9 0 policy 8:next tab\n",
        "v1 invoke 07 9 12 policy 8:next tab\n",
        "v1 invoke 18446744073709551616 9 12 policy 8:next tab\n",
        "v1 invoke 7 9 12 policy 08:next tab\n",
        "v1 invoke 7 9 12 other 8:next tab\n",
        "v1 invoke 7 9 12 policy 7:next tab\n",
        "v1 invoke 7 9 12 policy 0:\n",
        "v1 invoke 7 9 12 policy 2: a\n",
        "v1 invoke 7 9 12 policy 2:a \n",
        "v1 invoke 7 9 12 policy 2:a/\n",
        "v1 invoke 7 9 12 policy 2:é\n",
        "v1 invoke 7 9 12 policy 1:a\n\n",
    ] {
        assert!(decode_invocation(bad.as_bytes()).is_err(), "{bad:?}");
    }
    for bad in ["", " a", "a ", "a\0", "a\t", "é", &"a".repeat(129)] {
        assert!(encode_invocation(Invocation { name: bad, ..value }, &mut out).is_err());
    }
}
