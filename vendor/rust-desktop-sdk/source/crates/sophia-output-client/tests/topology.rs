use sophia_9p_client::pipeline::Reply;
use sophia_9p_records::{Errno, Fid, Qid, QidKind};
use sophia_output_client::topology::{Error, Fetch, Operation};
use sophia_output_protocol::output_files::*;

fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .flat_map(|word| {
            assert_eq!(word.len() % 2, 0);
            (0..word.len())
                .step_by(2)
                .map(|offset| u8::from_str_radix(&word[offset..offset + 2], 16).unwrap())
        })
        .collect()
}

fn literal() -> Vec<u8> {
    let mut bytes = hex("
        0700000000000000 0200000000000000 0100 0100 0100 0000
        0300000000000000 0400000000000000 0700 ff00 0400 0100
        0500000000000000 0000 0000 00000000 44502d31
    ");
    bytes.extend([0; 60]); // The remaining fixed 64-byte label slot.
    bytes.extend(hex("
        0500000000000000 20030000 58020000 60ea0000 0100 0000
        0200000000000000 0600000000000000 00000000 00000000 20030000 58020000 0100 0000
        0300000000000000 0300 0000
    "));
    bytes.extend([0; 36]); // Three unused member slots.
    assert_eq!(bytes.len(), 236);
    bytes
}

fn object() -> Vec<u8> {
    let body = literal();
    let mut bytes = vec![0; 32];
    bytes[..4].copy_from_slice(&268u32.to_le_bytes());
    bytes[4] = 1;
    bytes[6] = 2;
    bytes[8] = 9;
    bytes.extend(body);
    bytes
}
fn fetch() -> Fetch {
    Fetch::new(
        9,
        OutputFilePublication {
            topology_epoch: 7,
            qid_path: 55,
        },
        0,
        100,
    )
    .unwrap()
}
fn qid() -> Qid {
    Qid {
        kind: QidKind::File,
        version: 0,
        path: 55,
    }
}
fn reply(f: &mut Fetch, value: Reply, now: u64) -> Result<(), Error> {
    let fid = if f.operation(now, 4096)?.unwrap() == Operation::Walk {
        Fid(5)
    } else {
        Fid::NOFID
    };
    f.issued(now, 4096, fid)?;
    f.reply(value, now)
}
fn open(f: &mut Fetch, iounit: u32) {
    reply(f, Reply::Walk(vec![qid()]), 0).unwrap();
    reply(f, Reply::Lopen { qid: qid(), iounit }, 0).unwrap();
}
#[test]
fn fragmented_object_stays_hidden_until_eof_and_clunk() {
    let mut f = fetch();
    open(&mut f, 13);
    let bytes = object();
    let mut offset = 0;
    for chunk in bytes.chunks(13) {
        assert_eq!(
            f.operation(0, 4096),
            Ok(Some(Operation::Read {
                fid: Fid(5),
                offset,
                count: 13
            }))
        );
        reply(&mut f, Reply::Read(chunk.to_vec()), 0).unwrap();
        offset += chunk.len() as u64;
        assert!(f.result().is_none());
    }
    reply(&mut f, Reply::Read(vec![]), 0).unwrap();
    assert!(f.result().is_none());
    assert_eq!(f.operation(0, 4096), Ok(Some(Operation::Clunk(Fid(5)))));
    reply(&mut f, Reply::Clunk, 0).unwrap();
    let value = f.result().unwrap();
    assert_eq!(value.connection_epoch, 9);
    assert_eq!(value.snapshot.topology_epoch, 7);
    assert_eq!(value.snapshot.heads[0].label, "DP-1");
    assert_eq!(f.deadline(), None);
}
#[test]
fn publication_identity_is_checked_at_walk_open_and_decode() {
    for stage in 0..4 {
        let mut f = fetch();
        let wrong = Qid { path: 56, ..qid() };
        let result = match stage {
            0 => reply(&mut f, Reply::Walk(vec![wrong]), 0),
            1 => {
                reply(&mut f, Reply::Walk(vec![qid()]), 0).unwrap();
                reply(
                    &mut f,
                    Reply::Lopen {
                        qid: wrong,
                        iounit: 0,
                    },
                    0,
                )
            }
            _ => {
                open(&mut f, 0);
                let mut bytes = object();
                bytes[if stage == 2 { 8 } else { 32 }] += 1;
                reply(&mut f, Reply::Read(bytes), 0).unwrap();
                reply(&mut f, Reply::Read(vec![]), 0)
            }
        };
        assert_eq!(result, Err(Error::Invalid));
        assert!(f.result().is_none());
    }
}
#[test]
fn withheld_open_clunks_then_rewalks_with_original_deadline() {
    let mut f = fetch();
    reply(&mut f, Reply::Error(Errno(11)), 0).unwrap();
    assert_eq!(f.retry_at(), Some(4));
    assert_eq!(f.operation(3, 4096), Ok(None));
    reply(&mut f, Reply::Walk(vec![qid()]), 4).unwrap();
    reply(&mut f, Reply::Error(Errno(11)), 4).unwrap();
    assert_eq!(f.retry_at(), Some(12));
    assert_eq!(f.operation(12, 4096), Ok(Some(Operation::Clunk(Fid(5)))));
    reply(&mut f, Reply::Clunk, 12).unwrap();
    assert_eq!(f.operation(12, 4096), Ok(Some(Operation::Walk)));
    reply(&mut f, Reply::Walk(vec![qid()]), 12).unwrap();
    reply(
        &mut f,
        Reply::Lopen {
            qid: qid(),
            iounit: 0,
        },
        12,
    )
    .unwrap();
    assert_eq!(f.retry_at(), None);
    assert_eq!(f.deadline(), Some(100));
    assert_eq!(f.operation(100, 4096), Err(Error::Deadline));
}
#[test]
fn malformed_length_early_eof_and_long_reads_are_refused() {
    for case in 0..4 {
        let mut f = fetch();
        open(&mut f, if case == 3 { 1 } else { 0 });
        let mut bytes = object();
        let result = match case {
            0 => {
                bytes[..4].copy_from_slice(&u32::MAX.to_le_bytes());
                reply(&mut f, Reply::Read(bytes), 0)
            }
            1 => {
                bytes.push(0);
                reply(&mut f, Reply::Read(bytes), 0)
            }
            2 => {
                reply(&mut f, Reply::Read(bytes[..40].to_vec()), 0).unwrap();
                reply(&mut f, Reply::Read(vec![]), 0)
            }
            _ => reply(&mut f, Reply::Read(bytes), 0),
        };
        assert_eq!(result, Err(Error::Invalid));
        assert!(f.result().is_none());
    }
}
#[test]
fn clunk_refusal_and_late_reply_never_expose_topology() {
    for late in [false, true] {
        let mut f = fetch();
        open(&mut f, 0);
        reply(&mut f, Reply::Read(object()), 0).unwrap();
        reply(&mut f, Reply::Read(vec![]), 0).unwrap();
        f.issued(99, 4096, Fid::NOFID).unwrap();
        let result = if late {
            f.reply(Reply::Clunk, 100)
        } else {
            f.reply(Reply::Error(Errno(116)), 99)
        };
        assert_eq!(
            result,
            Err(if late {
                Error::Deadline
            } else {
                Error::Remote(116)
            })
        );
        assert!(f.result().is_none());
    }
}
