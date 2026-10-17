use sophia_9p_client::pipeline::PipelineLimits;
use sophia_admin_client::{
    command::Custody,
    connecting::Connecting,
    protocol::{Command, Outcome, Owner},
    session::{Error, Session},
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    os::unix::net::UnixStream,
    thread,
    time::{Duration, Instant},
};
fn send(s: &mut UnixStream, kind: u8, tag: &[u8], body: &[u8]) {
    let mut out = ((body.len() + 7) as u32).to_le_bytes().to_vec();
    out.push(kind);
    out.extend(tag);
    out.extend(body);
    s.write_all(&out).unwrap();
}
fn qid(dir: bool) -> Vec<u8> {
    let mut q = vec![0; 13];
    q[0] = if dir { 128 } else { 0 };
    q
}
fn fixture(fault: u8) -> (Session, thread::JoinHandle<(usize, usize)>, Instant) {
    let (client, mut peer) = UnixStream::pair().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let job = thread::spawn(move || {
        let mut files = HashMap::<u32, String>::new();
        let mut objects = HashMap::<u32, Vec<u8>>::new();
        let mut versions = 0;
        let mut attaches = 0;
        let mut writes = 0;
        let mut catalogs = 0;
        let mut id = 0;
        let mut polls = 0;
        let mut owner = String::new();
        loop {
            let mut h = [0; 7];
            match peer.read_exact(&mut h) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => panic!("{e}"),
            }
            let mut b = vec![0; u32::from_le_bytes(h[..4].try_into().unwrap()) as usize - 7];
            peer.read_exact(&mut b).unwrap();
            let fid = u32::from_le_bytes(b[..4].try_into().unwrap_or([0; 4]));
            match h[4] {
                100 => {
                    versions += 1;
                    assert_eq!(versions, 1);
                    send(&mut peer, 101, &h[5..], &b);
                }
                104 => {
                    attaches += 1;
                    assert_eq!(attaches, 1);
                    send(&mut peer, 105, &h[5..], &qid(true));
                }
                110 => {
                    let newfid = u32::from_le_bytes(b[4..8].try_into().unwrap());
                    files.insert(newfid, String::from_utf8(b[12..].to_vec()).unwrap());
                    let mut r = vec![1, 0];
                    r.extend(qid(false));
                    send(&mut peer, 111, &h[5..], &r);
                }
                12 => {
                    let name = files.get(&fid).unwrap();
                    assert_eq!(
                        u32::from_le_bytes(b[4..8].try_into().unwrap()),
                        u32::from(name == "ctl")
                    );
                    let data = match name.as_str() {
                        "info" => b"v1 info 1 0 7 10000 2000 60000\n".to_vec(),
                        "commands" => {
                            catalogs += 1;
                            b"v1 catalog 12 2\nv1 command policy 8:next tab\nv1 command session 14:reload-profile\n".to_vec()
                        }
                        "status" => {
                            assert!(id > 0);
                            polls += 1;
                            if polls == 1 {
                                format!("v1 pending 7 {id} 12\n").into_bytes()
                            } else {
                                format!(
                                    "v1 outcome 7 {id} {} {} {}:ok\n",
                                    if fault == 3 { 13 } else { 12 },
                                    if fault == 6 {
                                        "completed"
                                    } else if owner == "policy" {
                                        "committed"
                                    } else {
                                        "unchanged"
                                    },
                                    if fault == 5 { 3 } else { 2 }
                                )
                                .into_bytes()
                            }
                        }
                        "ctl" => vec![],
                        _ => panic!("unknown file {name}"),
                    };
                    objects.insert(fid, data);
                    let mut r = qid(false);
                    r.extend(if name == "ctl" { 512u32 } else { 7 }.to_le_bytes());
                    send(&mut peer, 13, &h[5..], &r);
                }
                116 => {
                    let data = objects.get(&fid).unwrap();
                    let off = u64::from_le_bytes(b[4..12].try_into().unwrap()) as usize;
                    let n = u32::from_le_bytes(b[12..16].try_into().unwrap()) as usize;
                    assert!(n <= 7);
                    let data = &data[off..data.len().min(off + n)];
                    let mut r = (data.len() as u32).to_le_bytes().to_vec();
                    r.extend(data);
                    send(&mut peer, 117, &h[5..], &r);
                }
                118 => {
                    assert_eq!(files[&fid], "ctl");
                    assert_eq!(&b[4..12], &[0; 8]);
                    writes += 1;
                    let text = std::str::from_utf8(&b[16..]).unwrap();
                    let fields: Vec<_> = text.splitn(7, ' ').collect();
                    id = fields[3].parse::<u64>().unwrap();
                    assert_eq!(id as usize, writes);
                    assert_eq!(fields[2], "7");
                    assert_eq!(fields[4], "12");
                    owner = fields[5].to_owned();
                    polls = 0;
                    if fault == 1 {
                        send(&mut peer, 7, &h[5..], &11u32.to_le_bytes());
                    } else if fault == 2 {
                        break;
                    } else if fault == 4 {
                        let count = u32::from_le_bytes(b[12..16].try_into().unwrap());
                        send(&mut peer, 119, &h[5..], &(count - 1).to_le_bytes());
                    } else {
                        send(&mut peer, 119, &h[5..], &b[12..16]);
                    }
                }
                120 => {
                    objects.remove(&fid);
                    files.remove(&fid);
                    send(&mut peer, 121, &h[5..], &[]);
                }
                _ => panic!("unexpected op {}", h[4]),
            }
        }
        (writes, catalogs)
    });
    let start = Instant::now();
    let mut opening = Connecting::new(client, PipelineLimits::default(), 0, 2000).unwrap();
    while !opening.ready() {
        assert!(start.elapsed() < Duration::from_secs(2));
        opening
            .dispatch(start.elapsed().as_millis() as u64, 512)
            .unwrap();
        thread::yield_now();
    }
    assert!(opening.io_interest().unwrap().immediate);
    assert_eq!(opening.next_wake(), None);
    let session = opening.take_session().unwrap();
    assert!(opening.take_session().is_none());
    (session, job, start)
}
fn run(s: &mut Session, start: Instant, done: impl Fn(&Session) -> bool) {
    while !done(s) {
        assert!(start.elapsed() < Duration::from_secs(2));
        let _ = s.dispatch(start.elapsed().as_millis() as u64, 1);
        thread::yield_now();
    }
}
#[test]
fn sequential_commands_retain_catalog_then_refresh_after_session_outcome() {
    let (mut s, peer, start) = fixture(0);
    run(&mut s, start, |s| {
        s.catalog_generation().is_some() || s.error().is_some()
    });
    assert!(s.error().is_none());
    assert_eq!(s.commands().count(), 2);
    assert_eq!(s.next_wake(), None);
    assert_eq!(
        s.invoke(
            Command {
                owner: Owner::Policy,
                name: "unknown"
            },
            start.elapsed().as_millis() as u64
        ),
        Err(Error::Unadvertised)
    );
    for (id, owner, name) in [
        (1, Owner::Policy, "next tab"),
        (2, Owner::Session, "reload-profile"),
        (3, Owner::Policy, "next tab"),
    ] {
        if id == 3 {
            assert_eq!(s.catalog_generation(), None);
            s.refresh_catalog(start.elapsed().as_millis() as u64)
                .unwrap();
            run(&mut s, start, |s| {
                s.catalog_generation().is_some() || s.error().is_some()
            });
        }
        assert_eq!(
            s.invoke(Command { owner, name }, start.elapsed().as_millis() as u64)
                .unwrap(),
            id
        );
        assert_eq!(
            s.invoke(Command { owner, name }, start.elapsed().as_millis() as u64),
            Err(Error::Busy)
        );
        run(&mut s, start, |s| {
            s.latest_custody() == Some((id, Custody::Settled)) || s.error().is_some()
        });
        assert!(s.error().is_none(), "{:?}", s.error());
        assert_eq!(
            s.outcome(),
            Some((
                if owner == Owner::Policy {
                    Outcome::Committed
                } else {
                    Outcome::Unchanged
                },
                "ok"
            ))
        );
        assert_eq!(s.next_wake(), None);
    }
    s.close();
    assert_eq!(s.latest_custody(), Some((3, Custody::Settled)));
    assert_eq!(peer.join().unwrap(), (3, 2));
}
#[test]
fn refusal_loss_short_write_and_invalid_status_are_terminal_without_replay() {
    for fault in 1..=6 {
        let (mut s, peer, start) = fixture(fault);
        run(&mut s, start, |s| s.catalog_generation().is_some());
        s.invoke(
            Command {
                owner: Owner::Policy,
                name: "next tab",
            },
            start.elapsed().as_millis() as u64,
        )
        .unwrap();
        run(&mut s, start, |s| s.error().is_some());
        assert!(s.fd().is_none());
        assert_eq!(
            s.latest_custody(),
            Some((
                1,
                if fault == 1 {
                    Custody::Refused
                } else {
                    Custody::Unknown
                }
            ))
        );
        assert!(
            s.invoke(
                Command {
                    owner: Owner::Policy,
                    name: "next tab"
                },
                start.elapsed().as_millis() as u64
            )
            .is_err()
        );
        assert_eq!(peer.join().unwrap(), (1, 1));
    }
}
