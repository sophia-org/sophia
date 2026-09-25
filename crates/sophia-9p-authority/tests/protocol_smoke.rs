use sophia_9p_authority::NinePAuthority;
use sophia_9p_authority::protocol::{decode_t_message, encode_r_message};
use sophia_9p_authority::types::{Fid, RMessage, TMessage, Tag};

#[test]
fn test_version_handshake() {
    let mut auth = NinePAuthority::new();
    let tag = Tag::new(1);
    let msg = TMessage::Version {
        msize: 65536,
        version: "9P2000.L".to_string(),
    };

    let reply = auth.handle_message(tag, msg).expect("Version failed");
    assert_eq!(
        reply,
        RMessage::Version {
            msize: 65536,
            version: "9P2000.L".to_string()
        }
    );
}

#[test]
fn test_window_lifecycle_via_synthetic_tree() {
    let mut auth = NinePAuthority::new();

    // 1. Attach root fid 0
    let attach_reply = auth
        .handle_message(
            Tag::new(1),
            TMessage::Attach {
                fid: Fid::new(0),
                afid: Fid::NOFID,
                uname: "user".to_string(),
                aname: String::new(),
            },
        )
        .expect("Attach failed");
    assert!(matches!(attach_reply, RMessage::Attach { .. }));

    // 2. Walk to "new" with fid 1
    let walk_reply = auth
        .handle_message(
            Tag::new(2),
            TMessage::Walk {
                fid: Fid::new(0),
                newfid: Fid::new(1),
                wnames: vec!["new".to_string()],
            },
        )
        .expect("Walk to new failed");
    assert!(matches!(walk_reply, RMessage::Walk { ref wqids } if wqids.len() == 1));

    // 3. Open fid 1 for reading
    auth.handle_message(
        Tag::new(3),
        TMessage::Open {
            fid: Fid::new(1),
            mode: 0,
        },
    )
    .expect("Open new failed");

    // 4. Read from fid 1 -> allocates window 1
    let read_reply = auth
        .handle_message(
            Tag::new(4),
            TMessage::Read {
                fid: Fid::new(1),
                offset: 0,
                count: 100,
            },
        )
        .expect("Read new failed");

    match read_reply {
        RMessage::Read { data } => {
            let str_val = std::str::from_utf8(&data).expect("Valid utf8");
            assert_eq!(str_val.trim(), "1");
        }
        _ => panic!("Expected Rread"),
    }

    // 5. Walk to "1/ctl" with fid 2
    auth.handle_message(
        Tag::new(5),
        TMessage::Walk {
            fid: Fid::new(0),
            newfid: Fid::new(2),
            wnames: vec!["1".to_string(), "ctl".to_string()],
        },
    )
    .expect("Walk to 1/ctl failed");

    // 6. Open fid 2 for read/write
    auth.handle_message(
        Tag::new(6),
        TMessage::Open {
            fid: Fid::new(2),
            mode: 2,
        },
    )
    .expect("Open 1/ctl failed");

    // 7. Write to 1/ctl: configure size and title
    let ctl_cmd = b"size 1024 768\ntitle TestWindow\n";
    auth.handle_message(
        Tag::new(7),
        TMessage::Write {
            fid: Fid::new(2),
            offset: 0,
            data: ctl_cmd.to_vec(),
        },
    )
    .expect("Write 1/ctl failed");

    // 8. Verify the window state in the authority
    let win = auth.tree.get_window(1).expect("Window 1 must exist");
    assert_eq!(win.size.width, 1024);
    assert_eq!(win.size.height, 768);
    assert_eq!(win.title, "TestWindow");
    assert_eq!(win.buffer.len(), 1024 * 768 * 4);
}

#[test]
fn test_wire_codec_roundtrip() {
    let tag = Tag::new(42);
    let msg = RMessage::Version {
        msize: 8192,
        version: "9P2000".to_string(),
    };

    let encoded = encode_r_message(tag, &msg);
    assert!(encoded.len() > 7);

    // Verify 4-byte size header matches payload length
    let size = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
    assert_eq!(size as usize, encoded.len());

    // Also verify decode_t_message on a synthetic Tversion buffer
    let t_msg = TMessage::Version {
        msize: 8192,
        version: "9P2000".to_string(),
    };
    let mut t_encoded = Vec::new();
    let payload_size = 4 + 2 + 6;
    let total_size = (4 + 1 + 2 + payload_size) as u32;
    t_encoded.extend_from_slice(&total_size.to_le_bytes());
    t_encoded.push(100); // Tversion opcode
    t_encoded.extend_from_slice(&tag.raw().to_le_bytes());
    t_encoded.extend_from_slice(&8192u32.to_le_bytes());
    t_encoded.extend_from_slice(&6u16.to_le_bytes());
    t_encoded.extend_from_slice(b"9P2000");

    let (decoded_tag, decoded_msg) = decode_t_message(&t_encoded).expect("Decode failed");
    assert_eq!(decoded_tag, tag);
    assert_eq!(decoded_msg, t_msg);
}
