//! The nonblocking, pipelined client against a scripted peer written from
//! the specification: version and connect, replies matched out of order,
//! the server's errno, flushing, the review-4 regressions (R4-1..R4-7), and
//! replies that break the protocol or the stream on purpose. No server core
//! is involved; the tests that run the pipeline against Sophia's real server
//! stay with that server.

use std::io::{ErrorKind, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sophia_9p_client::pipeline::{Pipeline, PipelineError, PipelineLimits, Reply};
use sophia_9p_client::records::{Errno, Fid, Qid, QidKind, Tag};

// ---- a scripted peer, written from the specification ----

/// One request as the scripted peer reads it, or `None` once the client has
/// closed its end.
fn next_request(stream: &mut UnixStream) -> Option<(u8, u16, Vec<u8>)> {
    let mut size = [0; 4];
    stream.read_exact(&mut size).ok()?;
    let mut rest = vec![0; u32::from_le_bytes(size) as usize - 4];
    stream.read_exact(&mut rest).ok()?;
    Some((
        rest[0],
        u16::from_le_bytes([rest[1], rest[2]]),
        rest[3..].to_vec(),
    ))
}

/// One request as the scripted peer reads it: type, tag and body.
fn request(stream: &mut UnixStream) -> (u8, u16, Vec<u8>) {
    next_request(stream).expect("a request")
}

fn reply(kind: u8, tag: u16, body: &[u8]) -> Vec<u8> {
    let mut frame = ((7 + body.len()) as u32).to_le_bytes().to_vec();
    frame.push(kind);
    frame.extend_from_slice(&tag.to_le_bytes());
    frame.extend_from_slice(body);
    frame
}

fn rversion(msize: u32, version: &[u8]) -> Vec<u8> {
    let mut body = msize.to_le_bytes().to_vec();
    body.extend_from_slice(&(version.len() as u16).to_le_bytes());
    body.extend_from_slice(version);
    reply(101, u16::MAX, &body)
}

fn rlerror(tag: u16, errno: u32) -> Vec<u8> {
    reply(7, tag, &errno.to_le_bytes())
}

fn rwalk(tag: u16, qids: &[[u8; 13]]) -> Vec<u8> {
    let mut body = (qids.len() as u16).to_le_bytes().to_vec();
    for qid in qids {
        body.extend_from_slice(qid);
    }
    reply(111, tag, &body)
}

fn rlopen(tag: u16, qid: [u8; 13], iounit: u32) -> Vec<u8> {
    let mut body = qid.to_vec();
    body.extend_from_slice(&iounit.to_le_bytes());
    reply(13, tag, &body)
}

fn rread(tag: u16, data: &[u8]) -> Vec<u8> {
    let mut body = (data.len() as u32).to_le_bytes().to_vec();
    body.extend_from_slice(data);
    reply(117, tag, &body)
}

fn string(out: &mut Vec<u8>, value: &[u8]) {
    out.extend_from_slice(&(value.len() as u16).to_le_bytes());
    out.extend_from_slice(value);
}

/// `fields` little-endian `u32`s, then `rest`: most request bodies' shape.
fn words(fields: &[u32], rest: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    for field in fields {
        body.extend_from_slice(&field.to_le_bytes());
    }
    body.extend_from_slice(rest);
    body
}

/// Answers every request with what `answer` makes of it, until the client
/// closes its end.
fn serve(stream: &mut UnixStream, answer: impl Fn(u8, u16, &[u8]) -> Vec<u8>) {
    while let Some((kind, tag, body)) = next_request(stream) {
        if stream.write_all(&answer(kind, tag, &body)).is_err() {
            return;
        }
    }
}

/// Keeps the peer's end open, reading and discarding, until the client
/// closes its own. A peer that closed right behind its last reply would
/// race the pipeline's greedy read-ahead into an end of file.
fn hold(stream: &mut UnixStream) {
    let mut rest = Vec::new();
    let _ = stream.read_to_end(&mut rest);
}

const QID_DIR: [u8; 13] = [0x80, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
const QID_FILE: [u8; 13] = [0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0];
const DIR: Qid = Qid {
    kind: QidKind::Directory,
    version: 0,
    path: 1,
};
const FILE: Qid = Qid {
    kind: QidKind::File,
    version: 0,
    path: 2,
};

/// Runs `script` as the peer of a new pipeline's stream, past a version
/// negotiation `script` must still perform itself.
fn scripted(
    script: impl FnOnce(&mut UnixStream) + Send + 'static,
    limits: PipelineLimits,
) -> (Result<Pipeline, PipelineError>, JoinHandle<()>) {
    let (client, mut server) = UnixStream::pair().unwrap();
    let thread = std::thread::spawn(move || script(&mut server));
    (
        Pipeline::over(client, limits, Duration::from_millis(500)),
        thread,
    )
}

fn versioned(stream: &mut UnixStream) {
    request(stream);
    stream.write_all(&rversion(65536, b"9P2000.L")).unwrap();
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(2)
}

/// A fresh directory for socket paths, unique to this process and test.
fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("sophia-9p-pipeline-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).unwrap();
    directory
}

// ---- version and connect ----

#[test]
fn version_takes_the_smaller_msize_and_bounds_reads_and_writes_by_it() {
    let (client, thread) = scripted(
        |stream| {
            let (kind, tag, body) = request(stream);
            assert_eq!((kind, tag), (100, u16::MAX), "Tversion under NOTAG");
            let mut offer = 65536u32.to_le_bytes().to_vec();
            string(&mut offer, b"9P2000.L");
            assert_eq!(body, offer);
            stream.write_all(&rversion(8192, b"9P2000.L")).unwrap();
            serve(stream, |kind, tag, body| match kind {
                116 => {
                    assert_eq!(body[12..], (8192u32 - 11).to_le_bytes(), "clamped read");
                    rread(tag, b"abc")
                }
                118 => {
                    assert_eq!(body.len(), 16 + 8192 - 23, "the largest write");
                    reply(119, tag, &body[12..16])
                }
                other => panic!("unexpected request type {other}"),
            });
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    assert_eq!(pipeline.msize(), 8192);

    let tag = pipeline.read(Fid(0), 0, u32::MAX).unwrap();
    assert_eq!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Read(b"abc".to_vec())
    );

    // size[4] type[1] tag[2] fid[4] offset[8] count[4]: 23 bytes before the
    // data, so 8169 bytes fill the negotiated msize exactly.
    assert_eq!(
        pipeline.write(Fid(0), 0, &[0; 8192 - 23 + 1]),
        Err(PipelineError::Limit("request larger than msize"))
    );
    assert_eq!(pipeline.outstanding(), 0, "nothing was queued");
    let tag = pipeline.write(Fid(0), 0, &[0; 8192 - 23]).unwrap();
    assert_eq!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Write(8192 - 23)
    );
    drop(pipeline);
    thread.join().unwrap();
}

#[test]
fn a_version_other_than_the_offer_is_refused() {
    type Answer = fn() -> Vec<u8>;
    let cases: [(&str, Answer, PipelineError); 11] = [
        (
            "unknown",
            || rversion(65536, b"unknown"),
            PipelineError::Protocol("dialect other than 9P2000.L"),
        ),
        (
            "9P2000",
            || rversion(65536, b"9P2000"),
            PipelineError::Protocol("dialect other than 9P2000.L"),
        ),
        (
            "9P2000.u",
            || rversion(65536, b"9P2000.u"),
            PipelineError::Protocol("dialect other than 9P2000.L"),
        ),
        (
            "a suffixed dialect",
            || rversion(65536, b"9P2000.L.Google.7"),
            PipelineError::Protocol("dialect other than 9P2000.L"),
        ),
        (
            "msize over the offer",
            || rversion(65537, b"9P2000.L"),
            PipelineError::Protocol("negotiated msize out of range"),
        ),
        (
            "msize under the floor",
            || rversion(4095, b"9P2000.L"),
            PipelineError::Protocol("negotiated msize out of range"),
        ),
        (
            "another tag",
            || {
                let mut frame = rversion(65536, b"9P2000.L");
                frame[5..7].copy_from_slice(&0u16.to_le_bytes());
                frame
            },
            PipelineError::Protocol("Rversion shape"),
        ),
        (
            // The server's refusal keeps its wire errno, as in `Client`.
            "refused by Rlerror",
            || rlerror(u16::MAX, Errno::EPROTO.0),
            PipelineError::Remote(Errno::EPROTO),
        ),
        (
            "trailing bytes",
            || {
                let mut body = 65536u32.to_le_bytes().to_vec();
                string(&mut body, b"9P2000.L");
                body.push(0);
                reply(101, u16::MAX, &body)
            },
            PipelineError::Protocol("Rversion shape"),
        ),
        (
            // A peer's oversized frame is a protocol violation during the
            // handshake just as after it.
            "frame over the offered msize",
            || {
                let mut frame = rversion(65536, b"9P2000.L");
                frame[..4].copy_from_slice(&65537u32.to_le_bytes());
                frame
            },
            PipelineError::Protocol("reply frame size"),
        ),
        (
            "a closed peer",
            Vec::new,
            PipelineError::Io(ErrorKind::UnexpectedEof),
        ),
    ];
    for (case, answer, error) in cases {
        let (client, thread) = scripted(
            move |stream| {
                request(stream);
                let _ = stream.write_all(&answer());
            },
            PipelineLimits::default(),
        );
        assert_eq!(client.err(), Some(error), "{case}");
        thread.join().unwrap();
    }
    for msize in [PipelineLimits::MIN_MSIZE, 65536] {
        let (client, thread) = scripted(
            move |stream| {
                request(stream);
                stream.write_all(&rversion(msize, b"9P2000.L")).unwrap();
            },
            PipelineLimits::default(),
        );
        assert_eq!(client.unwrap().msize(), msize, "either bound is accepted");
        thread.join().unwrap();
    }
}

#[test]
fn connecting_reports_why_and_is_bounded_by_the_handshake_deadline() {
    let directory = scratch("connect");
    let short = Duration::from_millis(200);
    assert_eq!(
        Pipeline::connect(&directory.join("missing"), PipelineLimits::default(), short).err(),
        Some(PipelineError::Io(ErrorKind::NotFound))
    );

    // A socket file whose listener is gone: nothing accepts there.
    let stale = directory.join("stale");
    drop(UnixListener::bind(&stale).unwrap());
    assert_eq!(
        Pipeline::connect(&stale, PipelineLimits::default(), short).err(),
        Some(PipelineError::Io(ErrorKind::ConnectionRefused))
    );

    // A listener that never accepts or answers: the connect completes into
    // its queue and the version negotiation meets the deadline.
    let silent = directory.join("silent");
    let _listener = UnixListener::bind(&silent).unwrap();
    let started = Instant::now();
    assert_eq!(
        Pipeline::connect(&silent, PipelineLimits::default(), short).err(),
        Some(PipelineError::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(2));

    // Limits are refused before any socket is touched.
    let refused = PipelineLimits {
        max_outstanding: 0,
        ..PipelineLimits::default()
    };
    assert_eq!(
        Pipeline::connect(&silent, refused, short).err(),
        Some(PipelineError::Limit("no outstanding requests"))
    );
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn connecting_to_a_listener_negotiates_and_attaches() {
    let directory = scratch("listen");
    let path = directory.join("export");
    let listener = UnixListener::bind(&path).unwrap();
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        request(&mut stream);
        stream.write_all(&rversion(16384, b"9P2000.L")).unwrap();
        let (_, tag, _) = request(&mut stream);
        stream.write_all(&reply(105, tag, &QID_DIR)).unwrap();
        hold(&mut stream);
    });
    let mut pipeline =
        Pipeline::connect(&path, PipelineLimits::default(), Duration::from_millis(500)).unwrap();
    assert_eq!(pipeline.msize(), 16384);
    let (tag, root) = pipeline.attach(b"", b"").unwrap();
    assert_eq!(root, Fid(0));
    assert_eq!(pipeline.wait(tag, deadline()).unwrap(), Reply::Attach(DIR));
    drop(pipeline);
    thread.join().unwrap();
    let _ = std::fs::remove_dir_all(directory);
}

// ---- pipelined round trips ----

/// Eight requests queued at once, each checked on the wire, answered in
/// reverse: every reply reaches the tag it answers, `take_reply` hands them
/// back in arrival order, and fid bookkeeping follows the answers.
#[test]
fn replies_answered_out_of_order_reach_their_own_tags() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let requests: Vec<_> = (0..8).map(|_| request(stream)).collect();
            let mut attach = words(&[0, u32::MAX], &[]);
            string(&mut attach, b"mason");
            string(&mut attach, b"shell");
            attach.extend_from_slice(&u32::MAX.to_le_bytes());
            let mut walk = words(&[0, 1], &2u16.to_le_bytes());
            string(&mut walk, b"dir");
            string(&mut walk, b"leaf");
            let mut sink_walk = words(&[0, 2], &1u16.to_le_bytes());
            string(&mut sink_walk, b"sink");
            let expected: [(u8, Vec<u8>); 8] = [
                (104, attach),
                (110, walk),
                (12, words(&[1, 0], &[])),
                (116, words(&[1, 5, 0, 64], &[])),
                (110, sink_walk),
                (12, words(&[2, 1], &[])),
                (118, words(&[2, 9, 0, 5], b"hello")),
                (120, words(&[1], &[])),
            ];
            for ((kind, _, body), (want_kind, want_body)) in requests.iter().zip(&expected) {
                assert_eq!((kind, body), (want_kind, want_body));
            }
            let tag = |index: usize| requests[index].1;
            let answers = [
                reply(105, tag(0), &QID_DIR),
                rwalk(tag(1), &[QID_DIR, QID_FILE]),
                rlopen(tag(2), QID_FILE, 8192),
                rread(tag(3), b"data"),
                rwalk(tag(4), &[QID_FILE]),
                rlopen(tag(5), QID_FILE, 0),
                reply(119, tag(6), &5u32.to_le_bytes()),
                reply(121, tag(7), &[]),
            ];
            for answer in answers.iter().rev() {
                stream.write_all(answer).unwrap();
            }
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let (attach_tag, root) = pipeline.attach(b"mason", b"shell").unwrap();
    let (walk_tag, leaf) = pipeline.walk(root, &[b"dir", b"leaf"]).unwrap();
    let lopen_tag = pipeline.lopen(leaf, 0).unwrap();
    let read_tag = pipeline.read(leaf, 5, 64).unwrap();
    let (sink_walk_tag, sink) = pipeline.walk(root, &[b"sink"]).unwrap();
    let sink_lopen_tag = pipeline.lopen(sink, 1).unwrap();
    let write_tag = pipeline.write(sink, 9, b"hello").unwrap();
    let clunk_tag = pipeline.clunk(leaf).unwrap();
    assert_eq!((root, leaf, sink), (Fid(0), Fid(1), Fid(2)));
    assert_eq!(pipeline.outstanding(), 8);

    // The attach is answered last, so by then every other reply is in.
    assert_eq!(
        pipeline.wait(attach_tag, deadline()).unwrap(),
        Reply::Attach(DIR)
    );
    let mut replies = Vec::new();
    while let Some(entry) = pipeline.take_reply() {
        replies.push(entry);
    }
    assert_eq!(
        replies,
        vec![
            (clunk_tag, Reply::Clunk),
            (write_tag, Reply::Write(5)),
            (
                sink_lopen_tag,
                Reply::Lopen {
                    qid: FILE,
                    iounit: 0
                }
            ),
            (sink_walk_tag, Reply::Walk(vec![FILE])),
            (read_tag, Reply::Read(b"data".to_vec())),
            (
                lopen_tag,
                Reply::Lopen {
                    qid: FILE,
                    iounit: 8192
                }
            ),
            (walk_tag, Reply::Walk(vec![DIR, FILE])),
        ]
    );
    assert_eq!(pipeline.outstanding(), 0);
    assert_eq!(pipeline.fid_count(), 2, "root and sink; leaf was clunked");
    assert!(!pipeline.is_poisoned());
    drop(pipeline);
    thread.join().unwrap();
}

/// A walk that stops short answers with the qids it managed; its new fid
/// never took hold and is released. One that walks every name keeps it.
#[test]
fn a_partial_walk_releases_its_new_fid() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            serve(stream, |kind, tag, body| match (kind, body[8]) {
                (110, 3) => rwalk(tag, &[QID_DIR, QID_DIR]),
                (110, _) => rwalk(tag, &[QID_FILE]),
                (other, _) => panic!("unexpected request type {other}"),
            });
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let (tag, _short) = pipeline.walk(Fid(9), &[b"a", b"b", b"c"]).unwrap();
    assert_eq!(pipeline.fid_count(), 1, "reserved at admission");
    assert_eq!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Walk(vec![DIR, DIR])
    );
    assert_eq!(pipeline.fid_count(), 0, "released by the partial walk");
    let (tag, whole) = pipeline.walk(Fid(9), &[b"a"]).unwrap();
    assert_eq!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Walk(vec![FILE])
    );
    assert_eq!(pipeline.fid_count(), 1, "{whole:?} kept");
    drop(pipeline);
    thread.join().unwrap();
}

/// `wait` passing its deadline is only that caller giving up: the pipeline
/// stays usable, and the reply is delivered once it arrives.
#[test]
fn a_wait_that_times_out_leaves_the_pipeline_usable() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (_, attach_tag, _) = request(stream);
            // Only once the client sends something else is the attach answered.
            let (_, clunk_tag, _) = request(stream);
            stream.write_all(&reply(105, attach_tag, &QID_DIR)).unwrap();
            stream.write_all(&reply(121, clunk_tag, &[])).unwrap();
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let (attach_tag, _root) = pipeline.attach(b"", b"").unwrap();
    let started = Instant::now();
    assert_eq!(
        pipeline.wait(attach_tag, Instant::now() + Duration::from_millis(50)),
        Err(PipelineError::Timeout)
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(!pipeline.is_poisoned());
    let clunk_tag = pipeline.clunk(Fid(7)).unwrap();
    assert_eq!(
        pipeline.wait(attach_tag, deadline()).unwrap(),
        Reply::Attach(DIR)
    );
    assert_eq!(pipeline.wait(clunk_tag, deadline()).unwrap(), Reply::Clunk);
    assert_eq!(pipeline.outstanding(), 0);
    drop(pipeline);
    thread.join().unwrap();
}

// ---- the server's errno ----

/// `Rlerror` carries a Linux errno from the server, not the host's: it is
/// delivered exactly as `Reply::Error`, never as an I/O kind or a timeout
/// (EAGAIN is not the host's `WouldBlock`); it releases the fid an attach or
/// walk reserved, and leaves the pipeline usable.
#[test]
fn a_server_errno_is_delivered_exactly_and_leaves_the_pipeline_usable() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (_, tag, _) = request(stream);
            stream.write_all(&rlerror(tag, 116)).unwrap();
            let (_, tag, _) = request(stream);
            stream.write_all(&reply(105, tag, &QID_DIR)).unwrap();
            let (_, tag, _) = request(stream);
            stream.write_all(&rlerror(tag, 11)).unwrap();
            let (_, tag, _) = request(stream);
            stream.write_all(&rlerror(tag, 4242)).unwrap();
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let (tag, _refused) = pipeline.attach(b"", b"").unwrap();
    assert_eq!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Error(Errno::ESTALE)
    );
    assert_eq!(pipeline.fid_count(), 0, "the refused attach's fid is freed");

    let (tag, root) = pipeline.attach(b"", b"").unwrap();
    assert_eq!(pipeline.wait(tag, deadline()).unwrap(), Reply::Attach(DIR));
    let (tag, _refused) = pipeline.walk(root, &[b"busy"]).unwrap();
    assert_eq!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Error(Errno::EAGAIN)
    );
    assert_eq!(pipeline.fid_count(), 1, "the refused walk's fid is freed");
    let tag = pipeline.read(root, 0, 64).unwrap();
    assert_eq!(
        pipeline.wait(tag, deadline()).unwrap(),
        Reply::Error(Errno(4242)),
        "an errno the host may not know is still carried as sent"
    );
    assert!(!pipeline.is_poisoned());
    assert_eq!(pipeline.outstanding(), 0);
    drop(pipeline);
    thread.join().unwrap();
}

// ---- replies that break the protocol ----

#[test]
fn a_malformed_reply_poisons_the_pipeline() {
    type Queue = fn(&mut Pipeline) -> Tag;
    type Answer = fn(u16) -> Vec<u8>;
    let attach: Queue = |pipeline| pipeline.attach(b"", b"").unwrap().0;
    let read: Queue = |pipeline| pipeline.read(Fid(0), 0, 4).unwrap();
    let write: Queue = |pipeline| pipeline.write(Fid(0), 0, b"abc").unwrap();
    let walk: Queue = |pipeline| pipeline.walk(Fid(0), &[b"x"]).unwrap().0;
    let cases: [(&str, Queue, Answer, PipelineError); 11] = [
        (
            "another tag",
            attach,
            |tag| reply(105, tag.wrapping_add(1), &QID_DIR),
            PipelineError::Protocol("reply to a tag not outstanding"),
        ),
        (
            "NOTAG",
            attach,
            |_| reply(105, u16::MAX, &QID_DIR),
            PipelineError::Protocol("reply to a tag not outstanding"),
        ),
        (
            "another type",
            attach,
            |tag| rlopen(tag, QID_DIR, 0),
            PipelineError::Protocol("unexpected reply type"),
        ),
        (
            "short body",
            attach,
            |tag| reply(105, tag, &[0]),
            PipelineError::Protocol("Rattach shape"),
        ),
        (
            "an unknown qid type",
            attach,
            |tag| {
                let mut qid = QID_DIR;
                qid[0] = 0x40;
                reply(105, tag, &qid)
            },
            PipelineError::Protocol("Rattach shape"),
        ),
        (
            "long error",
            attach,
            |tag| reply(7, tag, &[2, 0, 0, 0, 0]),
            PipelineError::Protocol("Rlerror shape"),
        ),
        (
            "oversize frame",
            attach,
            |tag| {
                let mut frame = reply(105, tag, &QID_DIR);
                frame[..4].copy_from_slice(&70_000u32.to_le_bytes());
                frame
            },
            PipelineError::Protocol("reply frame size"),
        ),
        (
            "five bytes read for four",
            read,
            |tag| rread(tag, b"abcde"),
            PipelineError::Protocol("Rread count"),
        ),
        (
            "a read count its data does not match",
            read,
            |tag| reply(117, tag, &[3, 0, 0, 0, 1, 2]),
            PipelineError::Protocol("Rread shape"),
        ),
        (
            "four bytes written of three",
            write,
            |tag| reply(119, tag, &4u32.to_le_bytes()),
            PipelineError::Protocol("Rwrite count"),
        ),
        (
            "more qids than names",
            walk,
            |tag| rwalk(tag, &[QID_DIR, QID_FILE]),
            PipelineError::Protocol("Rwalk count"),
        ),
    ];
    for (case, queue, answer, error) in cases {
        let (client, thread) = scripted(
            move |stream| {
                versioned(stream);
                let (_, tag, _) = request(stream);
                let _ = stream.write_all(&answer(tag));
                hold(stream);
            },
            PipelineLimits::default(),
        );
        let mut pipeline = client.unwrap();
        let tag = queue(&mut pipeline);
        assert_eq!(pipeline.wait(tag, deadline()), Err(error), "{case}");
        assert!(pipeline.is_poisoned(), "{case}");
        assert_eq!(
            pipeline.attach(b"", b""),
            Err(PipelineError::Poisoned),
            "{case}"
        );
        assert_eq!(pipeline.poll(), Err(PipelineError::Poisoned), "{case}");
        drop(pipeline);
        thread.join().unwrap();
    }
}

/// The frame limit is the negotiated msize, not the offer: a frame one byte
/// past it, or one shorter than a header, poisons before its body is read.
#[test]
fn a_frame_outside_the_negotiated_msize_poisons_the_pipeline() {
    for size in [8193u32, 6] {
        let (client, thread) = scripted(
            move |stream| {
                request(stream);
                stream.write_all(&rversion(8192, b"9P2000.L")).unwrap();
                let (_, tag, _) = request(stream);
                let mut frame = reply(105, tag, &QID_DIR);
                frame[..4].copy_from_slice(&size.to_le_bytes());
                let _ = stream.write_all(&frame);
                hold(stream);
            },
            PipelineLimits::default(),
        );
        let mut pipeline = client.unwrap();
        let (tag, _root) = pipeline.attach(b"", b"").unwrap();
        assert_eq!(
            pipeline.wait(tag, deadline()),
            Err(PipelineError::Protocol("reply frame size")),
            "{size}"
        );
        assert!(pipeline.is_poisoned(), "{size}");
        drop(pipeline);
        thread.join().unwrap();
    }
}

// ---- a peer that goes away ----

#[test]
fn a_frame_cut_by_the_peer_closing_is_a_disconnect() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (_, tag, _) = request(stream);
            // The header and two bytes of the thirteen-byte qid, then gone.
            stream.write_all(&reply(105, tag, &QID_DIR)[..9]).unwrap();
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let (tag, _root) = pipeline.attach(b"", b"").unwrap();
    assert_eq!(
        pipeline.wait(tag, deadline()),
        Err(PipelineError::Io(ErrorKind::UnexpectedEof))
    );
    assert!(pipeline.is_poisoned());
    assert!(pipeline.take_reply().is_none(), "nothing half-delivered");
    thread.join().unwrap();
}

#[test]
fn a_closed_server_surfaces_as_an_io_error() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            // The thread ends here, closing its half of the pair without ever
            // answering the request that follows.
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let (tag, _root) = pipeline.attach(b"", b"").unwrap();
    // Depending on whether the write or the read notices first, a closed
    // peer surfaces as either a broken pipe or an end of file; either way it
    // is an `Io` error, and it poisons the pipeline.
    match pipeline.wait(tag, deadline()) {
        Err(PipelineError::Io(kind)) => assert!(
            matches!(kind, ErrorKind::BrokenPipe | ErrorKind::UnexpectedEof),
            "unexpected io error kind: {kind:?}"
        ),
        other => panic!("expected an Io error, got {other:?}"),
    }
    assert!(pipeline.is_poisoned());
    thread.join().unwrap();
}

/// A peer may answer and then close at once; the answer, already read in
/// the same `poll` that met the end of file, still counts (review 4, Sophia
/// a8b3f7f1).
#[test]
fn a_reply_followed_at_once_by_close_is_still_delivered() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (_, tag, _) = request(stream);
            stream.write_all(&reply(105, tag, &QID_DIR)).unwrap();
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let (tag, _fid) = pipeline.attach(b"", b"").unwrap();
    assert_eq!(pipeline.wait(tag, deadline()), Ok(Reply::Attach(DIR)));
    thread.join().unwrap();
}

// ---- flushing ----

/// flush(5): answered by `Rflush` alone, the flushed request never took
/// hold; neither tag is left reserved.
#[test]
fn a_flush_answered_alone_cancels_its_request_and_frees_both_tags() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (kind, read_tag, _) = request(stream);
            assert_eq!(kind, 116);
            let (kind, flush_tag, body) = request(stream);
            assert_eq!((kind, body), (108, read_tag.to_le_bytes().to_vec()));
            stream.write_all(&reply(109, flush_tag, &[])).unwrap();
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let read_tag = pipeline.read(Fid(0), 0, 64).unwrap();
    let flush_tag = pipeline.flush(read_tag).unwrap();
    assert_eq!(pipeline.outstanding(), 2);
    assert_eq!(pipeline.wait(flush_tag, deadline()).unwrap(), Reply::Flush);
    assert!(
        pipeline.take_reply().is_none(),
        "the read was never answered"
    );
    assert_eq!(pipeline.outstanding(), 0);
    drop(pipeline);
    thread.join().unwrap();
}

/// flush(5): a flushed request's own answer that beats the `Rflush` is still
/// delivered, but its tag stays reserved -- and cannot be flushed again --
/// until the `Rflush` settles it.
#[test]
fn an_answer_that_beats_its_rflush_keeps_the_tag_until_the_rflush() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (_, read_tag, _) = request(stream);
            let (_, flush_tag, _) = request(stream);
            stream.write_all(&rread(read_tag, b"late")).unwrap();
            // The Rflush comes only once the client sends something more.
            let (_, clunk_tag, _) = request(stream);
            stream.write_all(&reply(109, flush_tag, &[])).unwrap();
            stream.write_all(&reply(121, clunk_tag, &[])).unwrap();
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let read_tag = pipeline.read(Fid(0), 0, 64).unwrap();
    let flush_tag = pipeline.flush(read_tag).unwrap();
    assert_eq!(
        pipeline.wait(read_tag, deadline()).unwrap(),
        Reply::Read(b"late".to_vec())
    );
    assert_eq!(
        pipeline.outstanding(),
        2,
        "the answered read's tag is held for its Rflush"
    );
    assert_eq!(
        pipeline.flush(read_tag),
        Err(PipelineError::Limit("tag not outstanding"))
    );
    let clunk_tag = pipeline.clunk(Fid(0)).unwrap();
    assert_eq!(pipeline.wait(flush_tag, deadline()).unwrap(), Reply::Flush);
    assert_eq!(pipeline.wait(clunk_tag, deadline()).unwrap(), Reply::Clunk);
    assert_eq!(pipeline.outstanding(), 0);
    assert!(!pipeline.is_poisoned());
    drop(pipeline);
    thread.join().unwrap();
}

#[test]
fn a_flush_answered_by_anything_but_rflush_poisons() {
    type Answer = fn(u16, u16) -> Vec<u8>;
    let cases: [(&str, Answer, PipelineError); 3] = [
        (
            "Rlerror",
            |_, flush_tag| rlerror(flush_tag, 22),
            PipelineError::Protocol("Rlerror answering Tflush"),
        ),
        (
            "another type",
            |_, flush_tag| reply(121, flush_tag, &[]),
            PipelineError::Protocol("unexpected reply type"),
        ),
        (
            "a second answer to the flushed request",
            |read_tag, _| {
                let mut twice = rread(read_tag, b"a");
                twice.extend(rread(read_tag, b"b"));
                twice
            },
            PipelineError::Protocol("reply to an already-settled tag"),
        ),
    ];
    for (case, answer, error) in cases {
        let (client, thread) = scripted(
            move |stream| {
                versioned(stream);
                let (_, read_tag, _) = request(stream);
                let (_, flush_tag, _) = request(stream);
                let _ = stream.write_all(&answer(read_tag, flush_tag));
                hold(stream);
            },
            PipelineLimits::default(),
        );
        let mut pipeline = client.unwrap();
        let read_tag = pipeline.read(Fid(0), 0, 64).unwrap();
        let flush_tag = pipeline.flush(read_tag).unwrap();
        assert_eq!(pipeline.wait(flush_tag, deadline()), Err(error), "{case}");
        assert!(pipeline.is_poisoned(), "{case}");
        drop(pipeline);
        thread.join().unwrap();
    }
}

/// Only an outstanding, unflushed, non-flush, non-clunk tag can be flushed;
/// every refusal happens before queueing and changes nothing. The clunk
/// case is R4-3's: a clunk already released its fid locally (clunk(5)), so
/// flushing it away unanswered would leave client and server disagreeing
/// about whether the fid exists.
#[test]
fn a_flush_is_refused_for_a_tag_it_cannot_settle() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            serve(stream, |kind, tag, _| match kind {
                120 => reply(121, tag, &[]),
                116 | 108 => Vec::new(),
                other => panic!("unexpected request type {other}"),
            });
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    assert_eq!(
        pipeline.flush(Tag(77)),
        Err(PipelineError::Limit("tag not outstanding"))
    );
    let read_tag = pipeline.read(Fid(0), 0, 64).unwrap();
    let flush_tag = pipeline.flush(read_tag).unwrap();
    assert_eq!(
        pipeline.flush(read_tag),
        Err(PipelineError::Limit("tag already being flushed"))
    );
    assert_eq!(
        pipeline.flush(flush_tag),
        Err(PipelineError::Limit("tag not outstanding"))
    );
    let clunk_tag = pipeline.clunk(Fid(0)).unwrap();
    assert_eq!(
        pipeline.flush(clunk_tag),
        Err(PipelineError::Limit("cannot flush a clunk"))
    );
    assert_eq!(
        pipeline.outstanding(),
        3,
        "no side effect from the refusals"
    );
    assert_eq!(pipeline.wait(clunk_tag, deadline()).unwrap(), Reply::Clunk);
    assert!(!pipeline.is_poisoned());
    drop(pipeline);
    thread.join().unwrap();
}

// ---- regression tests for the Codex review of ab1c4243a (R4-1..R4-7) ----

/// R4-1: a tag survives a full wrap of the tag space while its own reply
/// sits undrained. `candidate_tag` used to check only `in_flight`, which a
/// matched reply had already left, so once every other tag cycled round a
/// new request could be handed the same number and `wait` on it would
/// return the stale, unrelated old reply. Requests go out in batches, so
/// the wrap costs dozens of round trips rather than tens of thousands.
#[test]
fn a_completed_but_undrained_tag_survives_a_full_tag_wrap() {
    const BATCH: usize = 1000;
    const BATCHES: usize = 66; // 66,000 tags: past the 65,535-tag space.
    let limits = PipelineLimits {
        max_outstanding: 2 * BATCH as u16,
        ..PipelineLimits::default()
    };
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            serve(stream, |_, tag, _| reply(121, tag, &[]));
        },
        limits,
    );
    let mut pipeline = client.unwrap();

    let t0 = pipeline.clunk(Fid(0)).unwrap();
    // A second clunk, waited for, guarantees (FIFO, one connection) that
    // t0's own Rclunk already arrived and is sitting undrained.
    let t1 = pipeline.clunk(Fid(0)).unwrap();
    assert_eq!(pipeline.wait(t1, deadline()).unwrap(), Reply::Clunk);

    for _ in 0..BATCHES {
        let tags: Vec<Tag> = (0..BATCH)
            .map(|_| pipeline.clunk(Fid(0)).unwrap())
            .collect();
        assert!(
            !tags.contains(&t0),
            "t0's tag was handed to a new request while its reply was still undrained"
        );
        for tag in tags {
            assert_eq!(pipeline.wait(tag, deadline()).unwrap(), Reply::Clunk);
        }
    }
    assert_eq!(pipeline.outstanding(), 1, "only t0, still undrained");

    // Only draining t0 itself frees its number.
    assert_eq!(pipeline.take_reply(), Some((t0, Reply::Clunk)));
    assert_eq!(pipeline.outstanding(), 0);
    let reused = pipeline.clunk(Fid(0)).unwrap();
    assert_eq!(pipeline.wait(reused, deadline()).unwrap(), Reply::Clunk);
    drop(pipeline);
    thread.join().unwrap();
}

/// R4-2: a worst-case reply reservation is made at admission, before any
/// reply exists, so undrained replies are bounded by `max_buffered_input`
/// and not only by the outstanding count. A sixteen-name walk reserves
/// 16*13 bytes plus the per-reply floor; answered by `Rlerror`, each trues
/// down to the floor alone, freeing room before anything is drained.
#[test]
fn undrained_replies_are_bounded_by_bytes_not_only_by_count() {
    let limits = PipelineLimits {
        max_outstanding: 10_000,
        max_fids: 10_000,
        msize: PipelineLimits::MIN_MSIZE,
        max_buffered_input: PipelineLimits::MIN_MSIZE as usize,
        ..PipelineLimits::default()
    };
    let (client, thread) = scripted(
        |stream| {
            request(stream);
            stream
                .write_all(&rversion(PipelineLimits::MIN_MSIZE, b"9P2000.L"))
                .unwrap();
            serve(stream, |kind, tag, _| {
                assert_eq!(kind, 110);
                rlerror(tag, Errno::ENOENT.0)
            });
        },
        limits,
    );
    let mut pipeline = client.unwrap();
    let names: Vec<&[u8]> = vec![b"a"; 16];
    let mut tags = Vec::new();
    loop {
        match pipeline.walk(Fid(0), &names) {
            Ok((tag, _fid)) => tags.push(tag),
            Err(PipelineError::Limit("reply would not fit max_buffered_input")) => break,
            Err(other) => panic!("unexpected refusal: {other:?}"),
        }
        assert!(tags.len() <= 1000, "never hit the byte budget");
    }
    // 4096 / (16 + 16*13) = 18 walks at most.
    assert_eq!(tags.len(), 18, "the byte budget, not the count, bound this");
    assert_eq!(pipeline.outstanding(), tags.len());
    assert_eq!(pipeline.fid_count(), tags.len());

    // Answered, but only the last one drained: the rest still sit in the
    // pipeline, each now charged at Rlerror's floor.
    assert_eq!(
        pipeline.wait(*tags.last().unwrap(), deadline()).unwrap(),
        Reply::Error(Errno::ENOENT)
    );
    assert_eq!(pipeline.fid_count(), 0, "every refused walk freed its fid");
    assert_eq!(
        pipeline.outstanding(),
        tags.len() - 1,
        "undrained replies hold their tags"
    );
    assert!(
        pipeline.walk(Fid(0), &names).is_ok(),
        "truing up to Rlerror's small footprint should free room for more"
    );
    drop(pipeline);
    thread.join().unwrap();
}

/// R4-3 (the two `Rflush` orderings; the clunk half is
/// `a_flush_is_refused_for_a_tag_it_cannot_settle`): whichever answer wins
/// decides whether the attach's fid is kept or released, and either way the
/// flush's own `Rflush` is what settles it.
#[test]
fn flush_keeps_or_frees_the_fid_depending_on_which_answer_wins() {
    // The attach's own Rattach beats the flush's Rflush: its reply is still
    // delivered normally, and its fid is kept.
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (_, attach_tag, _) = request(stream);
            let (_, flush_tag, _) = request(stream);
            stream.write_all(&reply(105, attach_tag, &QID_DIR)).unwrap();
            stream.write_all(&reply(109, flush_tag, &[])).unwrap();
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let (attach_tag, _root) = pipeline.attach(b"", b"").unwrap();
    let flush_tag = pipeline.flush(attach_tag).unwrap();
    assert_eq!(pipeline.wait(flush_tag, deadline()).unwrap(), Reply::Flush);
    assert_eq!(
        pipeline.fid_count(),
        1,
        "the attach won the race, so its fid is kept"
    );
    assert_eq!(
        pipeline.take_reply(),
        Some((attach_tag, Reply::Attach(DIR)))
    );
    assert_eq!(pipeline.outstanding(), 0);
    drop(pipeline);
    thread.join().unwrap();

    // Only the Rflush ever arrives: the attach never took hold, and its fid
    // reservation is released.
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (_, _attach_tag, _) = request(stream);
            let (_, flush_tag, _) = request(stream);
            stream.write_all(&reply(109, flush_tag, &[])).unwrap();
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let (attach_tag, _root) = pipeline.attach(b"", b"").unwrap();
    let flush_tag = pipeline.flush(attach_tag).unwrap();
    assert_eq!(pipeline.wait(flush_tag, deadline()).unwrap(), Reply::Flush);
    assert_eq!(
        pipeline.fid_count(),
        0,
        "unanswered: the attach never took hold"
    );
    assert!(
        pipeline.take_reply().is_none(),
        "the flushed attach was never answered"
    );
    assert_eq!(pipeline.outstanding(), 0);
    drop(pipeline);
    thread.join().unwrap();
}

/// R4-4: sitting exactly at the ordinary outstanding limit, a flush of the
/// one request occupying it can still be queued -- it draws on its own
/// reserved tag space, output bytes and reply budget -- so a stuck request
/// is never uncancellable.
#[test]
fn a_flush_is_always_queueable_at_the_outstanding_limit() {
    let limits = PipelineLimits {
        max_outstanding: 1,
        ..PipelineLimits::default()
    };
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (kind, read_tag, _) = request(stream);
            assert_eq!(kind, 116);
            let (kind, flush_tag, body) = request(stream);
            assert_eq!((kind, body), (108, read_tag.to_le_bytes().to_vec()));
            stream.write_all(&reply(109, flush_tag, &[])).unwrap();
            let (_, tag, _) = request(stream);
            stream.write_all(&rread(tag, b"kept")).unwrap();
            hold(stream);
        },
        limits,
    );
    let mut pipeline = client.unwrap();
    let read_tag = pipeline.read(Fid(0), 0, 64).unwrap();
    assert_eq!(
        pipeline.read(Fid(0), 0, 64),
        Err(PipelineError::Limit("max outstanding requests reached")),
        "at the ordinary limit already"
    );
    let flush_tag = pipeline.flush(read_tag).unwrap();
    assert_eq!(pipeline.wait(flush_tag, deadline()).unwrap(), Reply::Flush);
    assert_eq!(pipeline.outstanding(), 0, "both freed once settled");
    let again = pipeline.read(Fid(0), 0, 64).unwrap();
    assert_eq!(
        pipeline.wait(again, deadline()).unwrap(),
        Reply::Read(b"kept".to_vec())
    );
    drop(pipeline);
    thread.join().unwrap();
}

/// R4-5: an oversized `data` is refused on a cheap preflight (frame size vs
/// `msize`) before it is ever copied into a request body.
#[test]
fn a_huge_write_is_refused_without_copying_its_data() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (kind, tag, _) = request(stream);
            assert_eq!(kind, 104, "the refused write never reached the wire");
            stream.write_all(&reply(105, tag, &QID_DIR)).unwrap();
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let huge = vec![0u8; 64 << 20];
    let started = Instant::now();
    assert_eq!(
        pipeline.write(Fid(0), 0, &huge),
        Err(PipelineError::Limit("request larger than msize"))
    );
    assert!(
        started.elapsed() < Duration::from_millis(200),
        "took {:?} to refuse a 64 MiB write: looks copied first",
        started.elapsed()
    );
    assert_eq!(pipeline.outstanding(), 0, "nothing was queued");
    assert!(!pipeline.is_poisoned());
    let (attach_tag, _root) = pipeline.attach(b"", b"").unwrap();
    assert_eq!(
        pipeline.wait(attach_tag, deadline()).unwrap(),
        Reply::Attach(DIR)
    );
    drop(pipeline);
    thread.join().unwrap();
}

/// R4-6: a declared qid count far past `MAX_WALK`, with a body far too short
/// to carry it, is rejected on that shape check -- before anything is
/// allocated from the count -- and poisons like any other malformed reply.
#[test]
fn an_rwalk_declaring_far_more_qids_than_it_carries_poisons() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (_, walk_tag, _) = request(stream);
            // nwqid = 65535, no qids following: nine bytes total.
            let _ = stream.write_all(&reply(111, walk_tag, &65535u16.to_le_bytes()));
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let (walk_tag, _reserved) = pipeline.walk(Fid(0), &[b"x"]).unwrap();
    assert_eq!(
        pipeline.wait(walk_tag, deadline()),
        Err(PipelineError::Protocol("Rwalk shape"))
    );
    assert!(pipeline.is_poisoned());
    drop(pipeline);
    thread.join().unwrap();
}

/// R4-7: walk(5) allows `nwqid == 0` only for a zero-name clone. A failure
/// at the first name must be `Rlerror`, so an empty `Rwalk` answering a
/// nonempty walk is a protocol violation, while the same empty `Rwalk`
/// answering a clone is its success and keeps the new, distinct fid.
#[test]
fn an_empty_rwalk_clones_a_fid_but_poisons_a_nonempty_walk() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (_, attach_tag, _) = request(stream);
            stream.write_all(&reply(105, attach_tag, &QID_DIR)).unwrap();
            let (_, clone_tag, body) = request(stream);
            assert_eq!(body, words(&[0, 1], &0u16.to_le_bytes()));
            stream.write_all(&rwalk(clone_tag, &[])).unwrap();
            let (_, walk_tag, _) = request(stream);
            let _ = stream.write_all(&rwalk(walk_tag, &[]));
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    let (attach_tag, root) = pipeline.attach(b"", b"").unwrap();
    assert_eq!(
        pipeline.wait(attach_tag, deadline()).unwrap(),
        Reply::Attach(DIR)
    );
    let (clone_tag, cloned) = pipeline.walk(root, &[]).unwrap();
    assert_eq!(
        pipeline.wait(clone_tag, deadline()).unwrap(),
        Reply::Walk(vec![])
    );
    assert_ne!(cloned, root, "a distinct fid, naming the same node");
    assert_eq!(pipeline.fid_count(), 2, "the clone's fid is kept");

    let (walk_tag, _reserved) = pipeline.walk(root, &[b"x"]).unwrap();
    assert_eq!(
        pipeline.wait(walk_tag, deadline()),
        Err(PipelineError::Protocol("empty Rwalk for a nonempty walk"))
    );
    assert!(pipeline.is_poisoned());
    drop(pipeline);
    thread.join().unwrap();
}

// ---- secret writes ----

#[test]
fn a_secret_write_sends_the_same_frame_and_scrubs_until_it_has_gone() {
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (plain_kind, plain_tag, plain) = request(stream);
            let (secret_kind, secret_tag, secret) = request(stream);
            assert_eq!((plain_kind, secret_kind), (118, 118));
            assert_eq!(plain, secret, "the same Twrite body");
            stream
                .write_all(&reply(119, secret_tag, &6u32.to_le_bytes()))
                .unwrap();
            stream
                .write_all(&reply(119, plain_tag, &6u32.to_le_bytes()))
                .unwrap();
            hold(stream);
        },
        PipelineLimits::default(),
    );
    let mut pipeline = client.unwrap();
    assert!(!pipeline.scrubbing_output());
    // A refused secret queues nothing and leaves nothing to scrub.
    assert_eq!(
        pipeline.write_secret(Fid(0), 0, &vec![0; pipeline.msize() as usize]),
        Err(PipelineError::Limit("request larger than msize"))
    );
    assert!(!pipeline.scrubbing_output());
    let plain = pipeline.write(Fid(3), 0, b"secret").unwrap();
    assert!(
        !pipeline.scrubbing_output(),
        "an ordinary write is not scrubbed"
    );
    let secret = pipeline.write_secret(Fid(3), 0, b"secret").unwrap();
    assert!(pipeline.scrubbing_output(), "queued, not yet sent");
    assert_eq!(pipeline.wait(secret, deadline()).unwrap(), Reply::Write(6));
    assert!(!pipeline.scrubbing_output(), "sent, so nothing is left");
    assert_eq!(pipeline.wait(plain, deadline()).unwrap(), Reply::Write(6));
    drop(pipeline);
    thread.join().unwrap();
}

#[test]
fn a_queued_secret_never_moves_when_later_requests_fill_the_output_buffer() {
    let limits = PipelineLimits::default();
    let (client, thread) = scripted(
        |stream| {
            versioned(stream);
            let (kind, tag, body) = request(stream);
            assert_eq!(kind, 118);
            assert_eq!(&body[16..], b"secret", "the secret frame arrives intact");
            stream
                .write_all(&reply(119, tag, &6u32.to_le_bytes()))
                .unwrap();
            serve(stream, |_, tag, body| {
                let count = u32::from_le_bytes(body[12..16].try_into().unwrap());
                assert!(body[16..].iter().all(|byte| *byte == 0x5a));
                reply(119, tag, &count.to_le_bytes())
            });
        },
        limits,
    );
    let mut pipeline = client.unwrap();
    let secret = pipeline.write_secret(Fid(3), 0, b"secret").unwrap();
    let capacity = pipeline.output_capacity();
    assert!(
        capacity >= limits.max_buffered_output,
        "the bound is reserved up front"
    );
    // Nothing is sent until a wait, so these queue behind the secret until
    // the output bound refuses one.
    let chunk = vec![0x5a; pipeline.msize() as usize - 23];
    let mut plain = Vec::new();
    loop {
        match pipeline.write(Fid(3), 0, &chunk) {
            Ok(tag) => plain.push(tag),
            Err(PipelineError::Limit("output buffer full")) => break,
            Err(error) => panic!("{error:?}"),
        }
        assert_eq!(pipeline.output_capacity(), capacity, "never reallocated");
    }
    assert!(plain.len() >= 2, "the buffer filled past its first growth");
    assert_eq!(pipeline.wait(secret, deadline()).unwrap(), Reply::Write(6));
    for tag in plain {
        assert_eq!(
            pipeline.wait(tag, deadline()).unwrap(),
            Reply::Write(chunk.len() as u32)
        );
    }
    assert!(!pipeline.scrubbing_output());
    drop(pipeline);
    thread.join().unwrap();
}
