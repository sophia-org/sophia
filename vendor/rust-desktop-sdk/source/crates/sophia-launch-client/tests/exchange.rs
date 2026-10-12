//! Independent wire scripts. They supply records, not a production verifier;
//! the protected Session/Factotum interoperability join belongs to Sophia.
use sophia_9p_client::records::Errno;
use sophia_launch_client::{Client, Error};
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
enum Case {
    Good,
    Retry,
    AlwaysRetry,
    BadChallenge,
    RelayShortWrite,
    RelayDenied,
    StatusDenied,
    AttachDenied,
    Silent,
}
struct Request {
    kind: u8,
    tag: u16,
    body: Vec<u8>,
}
fn next(s: &mut UnixStream) -> Option<Request> {
    let mut n = [0; 4];
    match s.read_exact(&mut n) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return None,
        Err(e) => panic!("script wait: {e}"),
    }
    let n = u32::from_le_bytes(n) as usize;
    assert!((7..=65536).contains(&n));
    let mut frame = vec![0; n - 4];
    s.read_exact(&mut frame).unwrap();
    Some(Request {
        kind: frame[0],
        tag: u16::from_le_bytes(frame[1..3].try_into().unwrap()),
        body: frame[3..].to_vec(),
    })
}
fn request(s: &mut UnixStream, kind: u8) -> Request {
    let r = next(s).expect("expected request");
    assert_eq!(r.kind, kind);
    r
}
fn reply(s: &mut UnixStream, r: &Request, kind: u8, body: &[u8]) {
    let mut frame = ((body.len() + 7) as u32).to_le_bytes().to_vec();
    frame.push(kind);
    frame.extend(r.tag.to_le_bytes());
    frame.extend(body);
    s.write_all(&frame).unwrap();
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}
fn string(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend((bytes.len() as u16).to_le_bytes());
    out.extend(bytes);
}
fn claims(bytes: &[u8], expected: &[&[u8]]) {
    let mut want = Vec::new();
    for value in expected {
        string(&mut want, value);
    }
    want.extend(u32::MAX.to_le_bytes());
    assert_eq!(bytes, want);
}
fn qid(kind: u8) -> Vec<u8> {
    let mut q = vec![0; 13];
    q[0] = kind;
    q[5] = 1;
    q
}
fn read_reply(s: &mut UnixStream, r: &Request, bytes: &[u8]) {
    let mut b = (bytes.len() as u32).to_le_bytes().to_vec();
    b.extend(bytes);
    reply(s, r, 117, &b);
}
fn version(s: &mut UnixStream) {
    let r = request(s, 100);
    let mut b = r.body[..4].to_vec();
    string(&mut b, b"9P2000.L");
    reply(s, &r, 101, &b);
}
fn challenge(round: u8) -> Vec<u8> {
    let mut b = vec![1, 0, 0, 0, 1, 0, 0, 0];
    b.extend((0..32).map(|n| n + round));
    b
}
fn eof(s: &mut UnixStream) {
    assert!(next(s).is_none(), "failure cannot fall back to attach");
}
fn role(mut s: UnixStream, case: Case, rounds: u8, signal: std::sync::mpsc::SyncSender<()>) {
    for round in 0..rounds {
        version(&mut s);
        let mut auth = request(&mut s, 102);
        claims(&auth.body[4..], &[b"lock", b"lock-provider"]);
        let _ = signal.send(());
        if case == Case::Silent {
            eof(&mut s);
            return;
        }
        if case == Case::AlwaysRetry {
            loop {
                reply(&mut s, &auth, 7, &11u32.to_le_bytes());
                let Some(next) = next(&mut s) else { return };
                assert_eq!(next.kind, 102);
                claims(&next.body[4..], &[b"lock", b"lock-provider"]);
                auth = next;
            }
        }
        if case == Case::Retry {
            reply(&mut s, &auth, 7, &11u32.to_le_bytes());
            auth = request(&mut s, 102);
            claims(&auth.body[4..], &[b"lock", b"lock-provider"]);
        }
        let afid = u32_at(&auth.body, 0);
        assert_ne!(afid, u32::MAX);
        reply(&mut s, &auth, 103, &qid(8));
        let read = request(&mut s, 116);
        assert_eq!(
            (
                u32_at(&read.body, 0),
                u64_at(&read.body, 4),
                u32_at(&read.body, 12)
            ),
            (afid, 0, 40)
        );
        let mut nonce = challenge(round);
        if case == Case::BadChallenge {
            nonce[4] = 2;
        }
        read_reply(&mut s, &read, &nonce);
        if matches!(
            case,
            Case::BadChallenge | Case::RelayShortWrite | Case::RelayDenied
        ) {
            eof(&mut s);
            return;
        }
        let status = request(&mut s, 116);
        assert_eq!(
            (
                u32_at(&status.body, 0),
                u64_at(&status.body, 4),
                u32_at(&status.body, 12)
            ),
            (afid, 40, 8)
        );
        let code = if case == Case::StatusDenied { 13u32 } else { 0 };
        let mut b = 1u32.to_le_bytes().to_vec();
        b.extend(code.to_le_bytes());
        read_reply(&mut s, &status, &b);
        if case == Case::StatusDenied {
            eof(&mut s);
            return;
        }
        let attach = request(&mut s, 104);
        assert_eq!(
            u32_at(&attach.body, 4),
            afid,
            "attach must name the verified afid"
        );
        claims(&attach.body[8..], &[b"lock", b"lock-provider"]);
        if case == Case::AttachDenied {
            reply(&mut s, &attach, 7, &116u32.to_le_bytes());
            eof(&mut s);
            return;
        }
        reply(&mut s, &attach, 105, &qid(128));
        let clunk = request(&mut s, 120);
        assert_eq!(u32_at(&clunk.body, 0), afid);
        reply(&mut s, &clunk, 121, &[]);
    }
    eof(&mut s);
}
fn relay(s: UnixStream, case: Case, rounds: u8) {
    relay_rounds(s, case, 0..rounds);
}
fn relay_rounds(mut s: UnixStream, case: Case, rounds: impl IntoIterator<Item = u8>) {
    for round in rounds {
        version(&mut s);
        let attach = request(&mut s, 104);
        assert_eq!(u32_at(&attach.body, 4), u32::MAX);
        claims(&attach.body[8..], &[b"", b"sophia-launch-relay-v1"]);
        let root = u32_at(&attach.body, 0);
        reply(&mut s, &attach, 105, &qid(128));
        let walk = request(&mut s, 110);
        assert_eq!(u32_at(&walk.body, 0), root);
        assert_eq!(&walk.body[8..], &[1, 0, 3, 0, b'r', b'p', b'c']);
        let rpc = u32_at(&walk.body, 4);
        let mut b = 1u16.to_le_bytes().to_vec();
        b.extend(qid(0));
        reply(&mut s, &walk, 111, &b);
        let open = request(&mut s, 12);
        assert_eq!(open.body, [rpc.to_le_bytes(), 2u32.to_le_bytes()].concat());
        let mut b = qid(0);
        b.extend(0u32.to_le_bytes());
        reply(&mut s, &open, 13, &b);
        if matches!(case, Case::BadChallenge | Case::Silent | Case::AlwaysRetry) {
            eof(&mut s);
            return;
        }
        let write = request(&mut s, 118);
        assert_eq!(
            (
                u32_at(&write.body, 0),
                u64_at(&write.body, 4),
                u32_at(&write.body, 12)
            ),
            (rpc, 0, 40)
        );
        assert_eq!(
            &write.body[16..],
            challenge(round),
            "relay must carry only this afid's challenge"
        );
        let count = if case == Case::RelayShortWrite {
            39u32
        } else {
            40
        };
        reply(&mut s, &write, 119, &count.to_le_bytes());
        if case == Case::RelayShortWrite {
            eof(&mut s);
            return;
        }
        let read = request(&mut s, 116);
        assert_eq!(
            (
                u32_at(&read.body, 0),
                u64_at(&read.body, 4),
                u32_at(&read.body, 12)
            ),
            (rpc, 0, 8)
        );
        let mut status = 1u32.to_le_bytes().to_vec();
        status.extend((if case == Case::RelayDenied { 13u32 } else { 0 }).to_le_bytes());
        read_reply(&mut s, &read, &status);
        if matches!(
            case,
            Case::RelayDenied | Case::StatusDenied | Case::AttachDenied
        ) {
            eof(&mut s);
            return;
        }
        for fid in [rpc, root] {
            let clunk = request(&mut s, 120);
            assert_eq!(u32_at(&clunk.body, 0), fid);
            reply(&mut s, &clunk, 121, &[]);
        }
    }
    eof(&mut s);
}

#[test]
fn another_endpoint_cannot_discard_pending_relay_work() {
    let (mut client, jobs) = start(Case::Good, 1, Duration::from_secs(2));
    drive(&mut client).unwrap();
    let mut authenticated = client.into_authenticated().unwrap();
    authenticated
        .relay
        .attach(b"", b"sophia-launch-relay-v1")
        .unwrap();
    let (role, _peer) = UnixStream::pair().unwrap();
    assert!(matches!(
        Client::over_relay(role, authenticated.relay, b"lock", b"lock-provider"),
        Err(Error::Invalid("pending requests on shared launch relay"))
    ));
    drop(authenticated.role);
    for job in jobs {
        job.join().unwrap();
    }
}

#[test]
fn one_launch_relay_authenticates_two_endpoints_without_resetting_the_first() {
    let (first, first_peer) = UnixStream::pair().unwrap();
    let (second, second_peer) = UnixStream::pair().unwrap();
    let (channel, relay_peer) = UnixStream::pair().unwrap();
    for s in [&first_peer, &second_peer, &relay_peer] {
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    }
    let (signal, _received) = std::sync::mpsc::sync_channel(2);
    let next_signal = signal.clone();
    let a = std::thread::spawn(move || role(first_peer, Case::Good, 1, signal));
    let b = std::thread::spawn(move || role(second_peer, Case::Good, 1, next_signal));
    let p = std::thread::spawn(move || relay_rounds(relay_peer, Case::Good, [0, 0]));
    let mut first = Client::over(first, channel, b"lock", b"lock-provider").unwrap();
    drive(&mut first).unwrap();
    let first = first.into_authenticated().unwrap();
    let mut second = Client::over_relay(second, first.relay, b"lock", b"lock-provider").unwrap();
    drive(&mut second).unwrap();
    let second = second.into_authenticated().unwrap();
    assert_eq!(first.role.outstanding(), 0);
    assert_ne!(first.root.0, u32::MAX);
    // The first script expects EOF after its one authentication exchange. A
    // Tversion or any other reset sent there makes that independent peer fail.
    drop(first.role);
    drop(second);
    a.join().unwrap();
    b.join().unwrap();
    p.join().unwrap();
}
fn start(case: Case, rounds: u8, timeout: Duration) -> (Client, Vec<JoinHandle<()>>) {
    let (client, jobs, _) = start_with_signal(case, rounds, timeout);
    (client, jobs)
}
fn start_with_signal(
    case: Case,
    rounds: u8,
    timeout: Duration,
) -> (Client, Vec<JoinHandle<()>>, std::sync::mpsc::Receiver<()>) {
    let (a, b) = UnixStream::pair().unwrap();
    let (p, q) = UnixStream::pair().unwrap();
    for s in [&b, &q] {
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    }
    rustix::io::fcntl_setfd(&p, rustix::io::FdFlags::empty()).unwrap();
    let (signal, received) = std::sync::mpsc::sync_channel(1);
    let jobs = vec![
        std::thread::spawn(move || role(b, case, rounds, signal)),
        std::thread::spawn(move || relay(q, case, rounds)),
    ];
    let client = Client::with_timeout(a, p, b"lock", b"lock-provider", timeout).unwrap();
    (client, jobs, received)
}
fn drive(client: &mut Client) -> Result<(), Error> {
    while !client.ready() {
        client.poll()?;
        client.wait_for_io(Duration::from_secs(1))?;
    }
    Ok(())
}
fn join(jobs: Vec<JoinHandle<()>>) {
    for j in jobs {
        j.join().unwrap();
    }
}

#[test]
fn authenticates_cleans_temporary_fids_and_reauthenticates_same_generation() {
    let (mut c, jobs) = start(Case::Good, 2, Duration::from_secs(2));
    drive(&mut c).unwrap();
    let admitted = c.into_authenticated().unwrap();
    assert_eq!(admitted.role.fid_count(), 1);
    assert_eq!(admitted.relay.fid_count(), 0);
    assert!(
        rustix::io::fcntl_getfd(&admitted.relay)
            .unwrap()
            .contains(rustix::io::FdFlags::CLOEXEC)
    );
    let mut c = admitted.reauthenticate().unwrap();
    drive(&mut c).unwrap();
    let admitted = c.into_authenticated().unwrap();
    assert_eq!(admitted.role.fid_count(), 1);
    drop(admitted);
    join(jobs);
}
#[test]
fn retries_only_auth_eagain_within_the_original_deadline() {
    let (mut c, jobs) = start(Case::Retry, 1, Duration::from_secs(2));
    drive(&mut c).unwrap();
    drop(c.into_authenticated().unwrap());
    join(jobs);
}
#[test]
fn malformed_challenge_and_short_relay_write_cannot_attach() {
    for case in [Case::BadChallenge, Case::RelayShortWrite] {
        let (mut c, jobs) = start(case, 1, Duration::from_secs(2));
        assert!(matches!(drive(&mut c), Err(Error::Invalid(_))));
        assert!(!c.ready());
        drop(c);
        join(jobs);
    }
}
#[test]
fn remote_refusals_at_each_authority_step_are_terminal() {
    for case in [Case::RelayDenied, Case::StatusDenied, Case::AttachDenied] {
        let (mut c, jobs) = start(case, 1, Duration::from_secs(2));
        let expected = Error::Remote(Errno(if case == Case::AttachDenied { 116 } else { 13 }));
        assert_eq!(drive(&mut c), Err(expected.clone()));
        assert_eq!(c.poll(), Err(expected));
        drop(c);
        join(jobs);
    }
}
#[test]
fn a_silent_verifier_cannot_renew_the_total_deadline() {
    let before = Instant::now();
    let (mut c, jobs) = start(Case::Silent, 1, Duration::from_millis(150));
    assert_eq!(drive(&mut c), Err(Error::Timeout));
    assert!(before.elapsed() < Duration::from_secs(2));
    drop(c);
    join(jobs);
}
#[test]
fn cancellation_closes_both_streams_without_an_attach_fallback() {
    let (mut c, jobs, auth_received) = start_with_signal(Case::Silent, 1, Duration::from_secs(2));
    // Reach the pending auth read, then cancel. The scripts require no attach.
    while auth_received.try_recv().is_err() {
        c.poll().unwrap();
        c.wait_for_io(Duration::from_millis(1)).unwrap();
    }
    c.cancel();
    assert_eq!(c.poll(), Err(Error::Cancelled));
    drop(c);
    join(jobs);
}

#[test]
fn repeated_auth_eagain_does_not_extend_the_attempt() {
    let before = Instant::now();
    let (mut c, jobs) = start(Case::AlwaysRetry, 1, Duration::from_millis(150));
    assert_eq!(drive(&mut c), Err(Error::Timeout));
    assert!(before.elapsed() < Duration::from_secs(2));
    drop(c);
    join(jobs);
}

#[test]
fn bounds_are_checked_before_any_wire_request() {
    for (claim, timeout) in [
        (vec![b'x'; 256], Duration::from_secs(1)),
        (vec![], Duration::ZERO),
        (vec![], Duration::from_secs(6)),
    ] {
        let (a, mut b) = UnixStream::pair().unwrap();
        let (p, mut q) = UnixStream::pair().unwrap();
        assert!(matches!(
            Client::with_timeout(a, p, &claim, b"role", timeout),
            Err(Error::Invalid(_))
        ));
        eof(&mut b);
        eof(&mut q);
    }
}
