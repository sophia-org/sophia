//! The nonblocking, pipelined client, against the real driver and static
//! export, and against a scripted server that breaks the protocol on
//! purpose.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sophia_9p::pipeline::{Pipeline, PipelineError, PipelineLimits, Reply};
use sophia_9p::unix::{Server, Wake};
use sophia_9p::{Errno, Fid, Limits, QidKind, Tag};
use support::StaticExport;

/// The static export served by the real driver on its own thread.
struct Served {
    export: StaticExport,
    wake: Wake,
    thread: Option<JoinHandle<()>>,
    server: Option<Server<StaticExport>>,
}

impl Served {
    fn new() -> Self {
        let export = StaticExport::new();
        let server = Server::new(export.clone(), Limits::default()).unwrap();
        Self {
            export,
            wake: server.wake(),
            thread: None,
            server: Some(server),
        }
    }

    /// A client stream adopted by the server; call before `start`.
    fn stream(&mut self) -> UnixStream {
        let (client, server) = UnixStream::pair().unwrap();
        assert!(
            self.server.as_mut().unwrap().adopt(server).is_ok(),
            "adopted"
        );
        client
    }

    fn start(&mut self) {
        let mut server = self.server.take().unwrap();
        self.thread = Some(std::thread::spawn(move || server.run().unwrap()));
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.wake.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn pipeline_with(limits: PipelineLimits) -> (Served, Pipeline) {
    let mut served = Served::new();
    let stream = served.stream();
    served.start();
    let pipeline = Pipeline::over(stream, limits, Duration::from_secs(5)).unwrap();
    (served, pipeline)
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[test]
fn pipelined_attach_walk_lopen_read_write_and_clunk_succeed_in_order() {
    let (served, mut pipeline) = pipeline_with(PipelineLimits::default());
    let (attach_tag, root) = pipeline.attach(b"", b"").unwrap();
    let (walk_tag, leaf) = pipeline.walk(root, &[b"dir", b"leaf"]).unwrap();
    let lopen_tag = pipeline.lopen(leaf, 0).unwrap();
    let read_tag = pipeline.read(leaf, 0, 64).unwrap();
    let (sink_walk_tag, sink) = pipeline.walk(root, &[b"sink"]).unwrap();
    let sink_lopen_tag = pipeline.lopen(sink, 1).unwrap();
    let write_tag = pipeline.write(sink, 0, b"hello").unwrap();
    let clunk_tag = pipeline.clunk(leaf).unwrap();
    let sink_clunk_tag = pipeline.clunk(sink).unwrap();
    let root_clunk_tag = pipeline.clunk(root).unwrap();

    assert_eq!(
        pipeline.wait(root_clunk_tag, deadline()).unwrap(),
        Reply::Clunk
    );

    let mut replies: HashMap<Tag, Reply> = HashMap::new();
    while let Some((tag, reply)) = pipeline.take_reply() {
        replies.insert(tag, reply);
    }
    match replies.remove(&attach_tag).unwrap() {
        Reply::Attach(qid) => assert_eq!(qid.kind, QidKind::Directory),
        other => panic!("attach: {other:?}"),
    }
    match replies.remove(&walk_tag).unwrap() {
        Reply::Walk(qids) => {
            assert_eq!(qids.len(), 2);
            assert_eq!(qids[1].kind, QidKind::File);
        }
        other => panic!("walk: {other:?}"),
    }
    match replies.remove(&lopen_tag).unwrap() {
        Reply::Lopen { qid, .. } => assert_eq!(qid.kind, QidKind::File),
        other => panic!("lopen: {other:?}"),
    }
    assert_eq!(
        replies.remove(&read_tag).unwrap(),
        Reply::Read(vec![0, 1, 2, 3])
    );
    match replies.remove(&sink_walk_tag).unwrap() {
        Reply::Walk(qids) => assert_eq!(qids.len(), 1),
        other => panic!("sink walk: {other:?}"),
    }
    match replies.remove(&sink_lopen_tag).unwrap() {
        Reply::Lopen { .. } => {}
        other => panic!("sink lopen: {other:?}"),
    }
    assert_eq!(replies.remove(&write_tag).unwrap(), Reply::Write(5));
    assert_eq!(replies.remove(&clunk_tag).unwrap(), Reply::Clunk);
    assert_eq!(replies.remove(&sink_clunk_tag).unwrap(), Reply::Clunk);
    assert!(replies.is_empty(), "every tag accounted for: {replies:?}");
    assert_eq!(served.export.state().sink, b"hello");
    assert!(!pipeline.is_poisoned());
}

#[test]
fn writes_complete_while_a_read_waits_on_events_then_the_event_arrives() {
    let (served, mut pipeline) = pipeline_with(PipelineLimits::default());
    let (_, root) = pipeline.attach(b"", b"").unwrap();
    let (_, events) = pipeline.walk(root, &[b"events"]).unwrap();
    pipeline.lopen(events, 0).unwrap();
    let (_, sink) = pipeline.walk(root, &[b"sink"]).unwrap();
    pipeline.lopen(sink, 1).unwrap();

    let read_tag = pipeline.read(events, 0, 64).unwrap();
    let write_tags: Vec<Tag> = (0u64..3)
        .map(|index| pipeline.write(sink, index * 4, b"data").unwrap())
        .collect();

    let last_write = *write_tags.last().unwrap();
    // `wait` returns the last write's own reply directly; it does not also
    // leave a copy for `take_reply`.
    assert_eq!(
        pipeline.wait(last_write, deadline()).unwrap(),
        Reply::Write(4)
    );

    let mut seen = Vec::new();
    while let Some(entry) = pipeline.take_reply() {
        seen.push(entry);
    }
    for tag in &write_tags[..write_tags.len() - 1] {
        assert!(
            seen.iter().any(|(seen_tag, _)| seen_tag == tag),
            "{tag:?} missing from {seen:?}"
        );
    }
    assert!(
        seen.iter().all(|(tag, _)| *tag != read_tag),
        "the read completed before its event arrived: {seen:?}"
    );
    assert_eq!(
        pipeline.outstanding(),
        1,
        "only the read is still outstanding"
    );

    served.export.push_event(b"woken");
    served.wake.wake();
    assert_eq!(
        pipeline.wait(read_tag, deadline()).unwrap(),
        Reply::Read(b"woken".to_vec())
    );
    assert_eq!(served.export.state().sink, b"datadatadata");
}

#[test]
fn flush_of_an_outstanding_read_frees_its_tag_without_a_reply() {
    let (served, mut pipeline) = pipeline_with(PipelineLimits::default());
    let (_, root) = pipeline.attach(b"", b"").unwrap();
    let (_, events) = pipeline.walk(root, &[b"events"]).unwrap();
    pipeline.lopen(events, 0).unwrap();

    let read_tag = pipeline.read(events, 0, 64).unwrap();
    let flush_tag = pipeline.flush(read_tag).unwrap();
    assert_eq!(pipeline.wait(flush_tag, deadline()).unwrap(), Reply::Flush);

    assert_eq!(pipeline.outstanding(), 0, "both tags are free again");
    let mut drained = Vec::new();
    while let Some(entry) = pipeline.take_reply() {
        drained.push(entry);
    }
    assert!(
        drained.iter().all(|(tag, _)| *tag != read_tag),
        "the flushed read was answered: {drained:?}"
    );

    // The event was never consumed, so a fresh read on the same fid still
    // sees it, and the freed tag is usable again.
    served.export.push_event(b"kept");
    let again = pipeline.read(events, 0, 64).unwrap();
    assert_eq!(
        pipeline.wait(again, deadline()).unwrap(),
        Reply::Read(b"kept".to_vec())
    );
}

#[test]
fn pipelined_requests_refuse_beyond_the_outstanding_limit_without_side_effects() {
    let limits = PipelineLimits {
        max_outstanding: 2,
        ..PipelineLimits::default()
    };
    let (_served, mut pipeline) = pipeline_with(limits);
    let (tag_a, _fid_a) = pipeline.attach(b"", b"").unwrap();
    let (tag_b, _fid_b) = pipeline.attach(b"", b"").unwrap();
    assert_eq!(pipeline.outstanding(), 2);
    assert_eq!(pipeline.fid_count(), 2);

    assert_eq!(
        pipeline.attach(b"", b""),
        Err(PipelineError::Limit("max outstanding requests reached"))
    );
    assert_eq!(pipeline.outstanding(), 2, "no side effect from the refusal");
    assert_eq!(pipeline.fid_count(), 2, "no side effect from the refusal");

    match pipeline.wait(tag_b, deadline()).unwrap() {
        Reply::Attach(_) => {}
        other => panic!("{other:?}"),
    }
    let mut remaining = Vec::new();
    while let Some(entry) = pipeline.take_reply() {
        remaining.push(entry);
    }
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].0, tag_a);
    assert_eq!(pipeline.outstanding(), 0);
}

#[test]
fn a_write_larger_than_msize_is_refused_before_queueing() {
    let limits = PipelineLimits {
        msize: 4096,
        ..PipelineLimits::default()
    };
    let (_served, mut pipeline) = pipeline_with(limits);
    assert_eq!(pipeline.msize(), 4096);
    let oversized = vec![0u8; 4096];
    assert_eq!(
        pipeline.write(Fid(0), 0, &oversized),
        Err(PipelineError::Limit("request larger than msize"))
    );
    assert_eq!(pipeline.outstanding(), 0, "nothing was queued");
    assert!(!pipeline.is_poisoned());

    // Still usable afterward.
    let (attach_tag, _root) = pipeline.attach(b"", b"").unwrap();
    assert!(matches!(
        pipeline.wait(attach_tag, deadline()).unwrap(),
        Reply::Attach(_)
    ));
}

#[test]
fn rlerror_is_delivered_as_a_reply_and_does_not_poison() {
    let (_served, mut pipeline) = pipeline_with(PipelineLimits::default());
    let (_, root) = pipeline.attach(b"", b"").unwrap();
    let before = pipeline.fid_count();
    let (walk_tag, _reserved) = pipeline.walk(root, &[b"hidden"]).unwrap();
    assert_eq!(
        pipeline.fid_count(),
        before + 1,
        "reserved until the reply settles it"
    );
    assert_eq!(
        pipeline.wait(walk_tag, deadline()).unwrap(),
        Reply::Error(Errno::EACCES)
    );
    assert_eq!(
        pipeline.fid_count(),
        before,
        "freed: the walk never took hold"
    );
    assert!(!pipeline.is_poisoned());

    let (ok_tag, _leaf) = pipeline.walk(root, &[b"info"]).unwrap();
    assert!(matches!(
        pipeline.wait(ok_tag, deadline()).unwrap(),
        Reply::Walk(_)
    ));
}

#[test]
fn fid_reuse_only_after_clunk_or_a_failed_walk() {
    let limits = PipelineLimits {
        max_fids: 2,
        ..PipelineLimits::default()
    };
    let (_served, mut pipeline) = pipeline_with(limits);
    let (_, root) = pipeline.attach(b"", b"").unwrap();

    let (walk_tag, dir) = pipeline.walk(root, &[b"dir"]).unwrap();
    assert_eq!(pipeline.fid_count(), 2);
    assert_eq!(
        pipeline.walk(root, &[b"info"]),
        Err(PipelineError::Limit("max fids reached"))
    );
    assert!(matches!(
        pipeline.wait(walk_tag, deadline()).unwrap(),
        Reply::Walk(_)
    ));

    // Clunk frees the slot the moment it is queued, not when Rclunk arrives.
    let clunk_tag = pipeline.clunk(dir).unwrap();
    assert_eq!(pipeline.fid_count(), 1, "freed before the reply");
    let (info_tag, info) = pipeline.walk(root, &[b"info"]).unwrap();
    assert_eq!(pipeline.fid_count(), 2);
    assert!(matches!(
        pipeline.wait(clunk_tag, deadline()).unwrap(),
        Reply::Clunk
    ));
    assert!(matches!(
        pipeline.wait(info_tag, deadline()).unwrap(),
        Reply::Walk(_)
    ));

    // A failed walk also frees its reservation, but only once settled.
    assert_eq!(
        pipeline.walk(root, &[b"hidden"]),
        Err(PipelineError::Limit("max fids reached")),
        "still at the cap: root + info"
    );
    let clunk_info = pipeline.clunk(info).unwrap();
    assert_eq!(pipeline.fid_count(), 1);
    let (hidden_tag, _reserved) = pipeline.walk(root, &[b"hidden"]).unwrap();
    assert_eq!(pipeline.fid_count(), 2, "reserved even though it will fail");
    assert_eq!(
        pipeline.wait(hidden_tag, deadline()).unwrap(),
        Reply::Error(Errno::EACCES)
    );
    assert_eq!(pipeline.fid_count(), 1, "freed: the walk never took hold");
    assert!(matches!(
        pipeline.wait(clunk_info, deadline()).unwrap(),
        Reply::Clunk
    ));
    assert_eq!(pipeline.fid_count(), 1);
}

#[test]
fn wait_buffers_other_replies_in_arrival_order() {
    let (_served, mut pipeline) = pipeline_with(PipelineLimits::default());
    let (attach_tag, root) = pipeline.attach(b"", b"").unwrap();
    let (info_tag, _info) = pipeline.walk(root, &[b"info"]).unwrap();
    let (dir_tag, _dir) = pipeline.walk(root, &[b"dir"]).unwrap();
    let (sink_tag, _sink) = pipeline.walk(root, &[b"sink"]).unwrap();

    assert!(matches!(
        pipeline.wait(sink_tag, deadline()).unwrap(),
        Reply::Walk(_)
    ));

    let order: Vec<Tag> =
        std::iter::from_fn(|| pipeline.take_reply().map(|(tag, _)| tag)).collect();
    assert_eq!(order, vec![attach_tag, info_tag, dir_tag]);
}

#[test]
fn as_fd_exposes_the_socket_for_the_callers_own_poll() {
    use std::os::fd::{AsFd, AsRawFd};
    let (_served, pipeline) = pipeline_with(PipelineLimits::default());
    assert!(pipeline.as_fd().as_raw_fd() >= 0);
}

#[test]
fn invalid_pipeline_limits_are_refused_before_connecting() {
    let mut served = Served::new();
    let stream = served.stream();
    served.start();
    let bad = PipelineLimits {
        msize: 2048,
        ..PipelineLimits::default()
    };
    assert_eq!(
        Pipeline::over(stream, bad, Duration::from_millis(200)).err(),
        Some(PipelineError::Limit("msize outside 4096..=16 MiB"))
    );
}

// ---- a scripted server, written from the specification ----

/// One request as the scripted server reads it: type, tag and body.
fn request(stream: &mut UnixStream) -> (u8, u16, Vec<u8>) {
    let mut size = [0; 4];
    stream.read_exact(&mut size).unwrap();
    let mut rest = vec![0; u32::from_le_bytes(size) as usize - 4];
    stream.read_exact(&mut rest).unwrap();
    (
        rest[0],
        u16::from_le_bytes([rest[1], rest[2]]),
        rest[3..].to_vec(),
    )
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

const QID_DIR: [u8; 13] = [0x80, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];

/// Runs `script` as the server of a new pipeline's stream, past a version
/// negotiation `script` must still perform itself (`Pipeline::over` does not
/// attach, exactly like `Client::over`).
fn scripted(
    script: impl FnOnce(&mut UnixStream) + Send + 'static,
) -> (Result<Pipeline, PipelineError>, JoinHandle<()>) {
    let (client, mut server) = UnixStream::pair().unwrap();
    let thread = std::thread::spawn(move || script(&mut server));
    (
        Pipeline::over(
            client,
            PipelineLimits::default(),
            Duration::from_millis(500),
        ),
        thread,
    )
}

fn versioned(stream: &mut UnixStream) {
    request(stream);
    stream.write_all(&rversion(65536, b"9P2000.L")).unwrap();
}

#[test]
fn a_malformed_reply_poisons_the_pipeline() {
    type Answer = fn(u16) -> Vec<u8>;
    let cases: [(&str, Answer, PipelineError); 4] = [
        (
            "another tag",
            |tag| reply(105, tag.wrapping_add(1), &QID_DIR),
            PipelineError::Protocol("reply to a tag not outstanding"),
        ),
        (
            "another type",
            |tag| {
                let mut body = QID_DIR.to_vec();
                body.extend_from_slice(&0u32.to_le_bytes());
                reply(13, tag, &body)
            },
            PipelineError::Protocol("unexpected reply type"),
        ),
        (
            "short body",
            |tag| reply(105, tag, &[0]),
            PipelineError::Protocol("Rattach shape"),
        ),
        (
            "oversize frame",
            |tag| {
                let mut frame = reply(105, tag, &QID_DIR);
                frame[..4].copy_from_slice(&70_000u32.to_le_bytes());
                frame
            },
            PipelineError::Protocol("reply frame size"),
        ),
    ];
    for (case, answer, error) in cases {
        let (client, thread) = scripted(move |stream| {
            versioned(stream);
            let (_, tag, _) = request(stream);
            let _ = stream.write_all(&answer(tag));
        });
        let mut pipeline = client.unwrap();
        let (tag, _fid) = pipeline.attach(b"", b"").unwrap();
        assert_eq!(pipeline.wait(tag, deadline()), Err(error.clone()), "{case}");
        assert!(pipeline.is_poisoned(), "{case}");
        assert_eq!(
            pipeline.attach(b"", b""),
            Err(PipelineError::Poisoned),
            "{case}"
        );
        thread.join().unwrap();
    }
}

#[test]
fn a_closed_server_surfaces_as_an_io_error() {
    let (client, thread) = scripted(|stream| {
        versioned(stream);
        // The thread ends here, closing its half of the pair without ever
        // answering the request that follows.
    });
    let mut pipeline = client.unwrap();
    let (tag, _root) = pipeline.attach(b"", b"").unwrap();
    // Depending on whether the write or the read notices first, a closed
    // peer surfaces as either a broken pipe or an end of file; either way it
    // is an `Io` error, and it poisons the pipeline.
    match pipeline.wait(tag, deadline()) {
        Err(PipelineError::Io(kind)) => assert!(
            matches!(
                kind,
                std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::UnexpectedEof
            ),
            "unexpected io error kind: {kind:?}"
        ),
        other => panic!("expected an Io error, got {other:?}"),
    }
    assert!(pipeline.is_poisoned());
    thread.join().unwrap();
}
