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

    // The earlier attach/walk/lopen replies are still sitting undrained (and
    // so still counted as outstanding: R4-1), so drain everything before
    // checking that every tag -- including the flushed read's and the
    // flush's own -- is free again.
    let mut drained = Vec::new();
    while let Some(entry) = pipeline.take_reply() {
        drained.push(entry);
    }
    assert!(
        drained.iter().all(|(tag, _)| *tag != read_tag),
        "the flushed read was answered: {drained:?}"
    );
    assert_eq!(pipeline.outstanding(), 0, "every tag is free again");

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

// ---- regression tests for the Codex review of ab1c4243a (R4-1..R4-7) ----

/// R4-2: a worst-case byte reservation is made at admission time (before any
/// reply exists), not just once bytes actually arrive, so undrained replies
/// -- even the small, mostly zero-payload kind this flags -- cannot grow
/// `completed`'s memory past `max_buffered_input`. Sixteen-name walks are
/// the heaviest fixed-shape request (16*13 + the base cost per reply, on
/// top of whatever a real reply turns out to need), so a tiny
/// `max_buffered_input` bounds how many can be admitted at once, far below
/// the generous outstanding-count ceiling.
#[test]
fn undrained_replies_are_bounded_by_bytes_not_only_by_count() {
    let limits = PipelineLimits {
        max_outstanding: 10_000,
        max_fids: 10_000,
        msize: PipelineLimits::MIN_MSIZE,
        max_buffered_input: PipelineLimits::MIN_MSIZE as usize,
        ..PipelineLimits::default()
    };
    let (_served, mut pipeline) = pipeline_with(limits);
    let (attach_tag, root) = pipeline.attach(b"", b"").unwrap();
    assert!(matches!(
        pipeline.wait(attach_tag, deadline()).unwrap(),
        Reply::Attach(_)
    ));

    let names: Vec<&[u8]> = vec![b"a"; 16];
    let mut tags = Vec::new();
    loop {
        match pipeline.walk(root, &names) {
            Ok((tag, _fid)) => tags.push(tag),
            Err(PipelineError::Limit("reply would not fit max_buffered_input")) => break,
            Err(other) => panic!("unexpected refusal: {other:?}"),
        }
        assert!(tags.len() <= 1000, "never hit the byte budget");
    }
    let admitted = tags.len();
    assert!(admitted > 0, "the first walk alone should have fit");
    assert!(
        admitted < 1000,
        "admitted {admitted} walks: the byte budget, not the 10,000 outstanding \
         ceiling, should have bound this"
    );
    assert_eq!(pipeline.outstanding(), admitted);

    // Answering them (Rlerror ENOENT: "a" is not root's child) trues each
    // reservation down from the worst case to Rlerror's small floor,
    // freeing room for more even though nothing has been drained yet.
    for &tag in &tags {
        assert_eq!(
            pipeline.wait(tag, deadline()).unwrap(),
            Reply::Error(Errno::ENOENT)
        );
    }
    assert!(
        pipeline.walk(root, &names).is_ok(),
        "truing up down to Rlerror's small footprint should free room for more"
    );
}

/// R4-3 (clunk half): flushing a clunk's tag is refused before queueing,
/// since a clunk already released its fid locally the moment it was sent
/// (clunk(5)); if the clunk were then flushed away unanswered, this pipeline
/// and the server could disagree forever about whether the fid still
/// exists.
#[test]
fn flush_of_a_clunk_is_refused_before_queueing() {
    let (_served, mut pipeline) = pipeline_with(PipelineLimits::default());
    let (attach_tag, root) = pipeline.attach(b"", b"").unwrap();
    assert!(matches!(
        pipeline.wait(attach_tag, deadline()).unwrap(),
        Reply::Attach(_)
    ));
    let clunk_tag = pipeline.clunk(root).unwrap();
    assert_eq!(
        pipeline.flush(clunk_tag),
        Err(PipelineError::Limit("cannot flush a clunk"))
    );
    assert_eq!(pipeline.outstanding(), 1, "no side effect from the refusal");
    assert_eq!(pipeline.wait(clunk_tag, deadline()).unwrap(), Reply::Clunk);
}

/// R4-4: even sitting exactly at the ordinary outstanding limit, a flush of
/// the one request occupying it can always be queued -- it draws on its own
/// reserved tag space, output bytes and reply-byte budget (see the module
/// doc) -- so a stuck request (here, a read waiting forever on `events`) is
/// never uncancellable.
#[test]
fn a_flush_is_always_queueable_at_the_outstanding_limit() {
    let limits = PipelineLimits {
        max_outstanding: 1,
        ..PipelineLimits::default()
    };
    let (served, mut pipeline) = pipeline_with(limits);
    let (attach_tag, root) = pipeline.attach(b"", b"").unwrap();
    assert!(matches!(
        pipeline.wait(attach_tag, deadline()).unwrap(),
        Reply::Attach(_)
    ));
    let (walk_tag, events) = pipeline.walk(root, &[b"events"]).unwrap();
    assert!(matches!(
        pipeline.wait(walk_tag, deadline()).unwrap(),
        Reply::Walk(_)
    ));
    let lopen_tag = pipeline.lopen(events, 0).unwrap();
    assert!(matches!(
        pipeline.wait(lopen_tag, deadline()).unwrap(),
        Reply::Lopen { .. }
    ));

    let read_tag = pipeline.read(events, 0, 64).unwrap();
    assert_eq!(pipeline.outstanding(), 1, "at the ordinary limit already");
    let flush_tag = pipeline.flush(read_tag).unwrap();
    assert_eq!(pipeline.wait(flush_tag, deadline()).unwrap(), Reply::Flush);
    assert_eq!(pipeline.outstanding(), 0, "both freed once settled");

    // The read was genuinely cancelled server-side too, not just locally: a
    // fresh read still waits for a fresh event.
    served.export.push_event(b"kept");
    let again = pipeline.read(events, 0, 64).unwrap();
    assert_eq!(
        pipeline.wait(again, deadline()).unwrap(),
        Reply::Read(b"kept".to_vec())
    );
}

/// R4-5: an oversized `data` is refused on a cheap preflight (frame size vs
/// `msize`) before it is ever copied into a request body. A copy of a slice
/// this size would be measurable; a preflight-only check is not.
#[test]
fn a_huge_write_is_refused_without_copying_its_data() {
    let (_served, mut pipeline) = pipeline_with(PipelineLimits::default());
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
    assert!(matches!(
        pipeline.wait(attach_tag, deadline()).unwrap(),
        Reply::Attach(_)
    ));
}

/// R4-7 (clone half): a genuine zero-name walk still works against the real
/// server core -- the empty-`Rwalk`-is-a-violation rule below applies only
/// to a nonempty walk.
#[test]
fn a_zero_name_walk_still_clones_the_fid() {
    let (_served, mut pipeline) = pipeline_with(PipelineLimits::default());
    let (_, root) = pipeline.attach(b"", b"").unwrap();
    let (clone_tag, cloned) = pipeline.walk(root, &[]).unwrap();
    assert_eq!(
        pipeline.wait(clone_tag, deadline()).unwrap(),
        Reply::Walk(vec![])
    );
    assert_ne!(
        cloned, root,
        "still a distinct fid, even though it names the same node"
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

/// R4-1: a tag survives a full wrap of the tag space while its own reply
/// sits undrained. `candidate_tag` used to check only `in_flight`, which
/// `resolve` had already removed the tag from the moment a reply matched --
/// so once every other tag had cycled through and come back around, a new
/// request could be handed the same number and `wait` on it would return
/// the stale, unrelated old reply before the new request was even written.
/// Well past the ~65535-tag space guarantees at least one full wrap.
const WRAP_TOTAL: usize = 66_000;

#[test]
fn a_completed_but_undrained_tag_survives_a_full_tag_wrap() {
    // The client sends t0, t1, `WRAP_TOTAL` more, then one "reused" clunk:
    // `WRAP_TOTAL + 3` real requests. One further iteration the client never
    // uses leaves the server blocked reading a request that never comes,
    // rather than closing the socket right after its last real reply --
    // which would race the pipeline's own greedy read-ahead into a spurious
    // EOF even though the reply it wanted had already arrived.
    let (client, thread) = scripted(|stream| {
        versioned(stream);
        for _ in 0..(WRAP_TOTAL + 4) {
            let (_, tag, _) = request(stream);
            stream.write_all(&reply(121, tag, &[])).unwrap();
        }
    });
    let mut pipeline = client.unwrap();

    let t0 = pipeline.clunk(Fid(0)).unwrap();
    // A second clunk, waited for, guarantees (FIFO, one connection) that
    // t0's own Rclunk already arrived and is sitting undrained.
    let t1 = pipeline.clunk(Fid(0)).unwrap();
    assert_eq!(pipeline.wait(t1, deadline()).unwrap(), Reply::Clunk);

    let mut saw_t0_again = false;
    for _ in 0..WRAP_TOTAL {
        let tag = pipeline.clunk(Fid(0)).unwrap();
        if tag == t0 {
            saw_t0_again = true;
        }
        assert_eq!(pipeline.wait(tag, deadline()).unwrap(), Reply::Clunk);
    }
    assert!(
        !saw_t0_again,
        "t0's tag was handed to a new request while its reply was still undrained"
    );

    // Only draining t0 itself frees its number.
    assert_eq!(pipeline.take_reply(), Some((t0, Reply::Clunk)));
    let reused = pipeline.clunk(Fid(0)).unwrap();
    assert_eq!(pipeline.wait(reused, deadline()).unwrap(), Reply::Clunk);
    // Dropping the pipeline closes its socket, unblocking the server's
    // extra, never-satisfied read; only then is the thread's exit awaited.
    drop(pipeline);
    let _ = thread.join();
}

/// R4-3 (the two `Rflush` orderings): whichever answer wins decides whether
/// the attach's fid is kept or released, and either way the flush's own
/// `Rflush` is what settles it.
#[test]
fn flush_keeps_or_frees_the_fid_depending_on_which_answer_wins() {
    // The attach's own Rattach beats the flush's Rflush: its reply is still
    // delivered normally, and its fid is kept. A trailing phantom request
    // the client never sends leaves the server blocked afterward, rather
    // than closing the socket right behind its last reply -- which would
    // race the pipeline's own greedy read-ahead into a spurious EOF even
    // though the reply it wanted had already arrived.
    let (client, thread) = scripted(|stream| {
        versioned(stream);
        let (_, attach_tag, _) = request(stream);
        let (_, flush_tag, _) = request(stream);
        stream.write_all(&reply(105, attach_tag, &QID_DIR)).unwrap();
        stream.write_all(&reply(109, flush_tag, &[])).unwrap();
        request(stream);
    });
    let mut pipeline = client.unwrap();
    let (attach_tag, _root) = pipeline.attach(b"", b"").unwrap();
    let flush_tag = pipeline.flush(attach_tag).unwrap();
    assert_eq!(pipeline.wait(flush_tag, deadline()).unwrap(), Reply::Flush);
    assert_eq!(
        pipeline.fid_count(),
        1,
        "the attach won the race, so its fid is kept"
    );
    match pipeline.take_reply() {
        Some((tag, Reply::Attach(qid))) => {
            assert_eq!(tag, attach_tag);
            assert_eq!(qid.kind, QidKind::Directory);
        }
        other => panic!("expected the attach's own reply, got {other:?}"),
    }
    drop(pipeline);
    let _ = thread.join();

    // Only the Rflush ever arrives: the attach never took hold, and its fid
    // reservation is released.
    let (client, thread) = scripted(|stream| {
        versioned(stream);
        let (_, _attach_tag, _) = request(stream);
        let (_, flush_tag, _) = request(stream);
        stream.write_all(&reply(109, flush_tag, &[])).unwrap();
        request(stream);
    });
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
    drop(pipeline);
    let _ = thread.join();
}

/// R4-6: a declared qid count far past `MAX_WALK`, with a body far too short
/// to carry it, is rejected on that shape check -- before anything is
/// allocated from the count -- and poisons like any other malformed reply.
#[test]
fn an_rwalk_declaring_far_more_qids_than_it_carries_poisons() {
    let (client, thread) = scripted(|stream| {
        versioned(stream);
        let (_, attach_tag, _) = request(stream);
        stream.write_all(&reply(105, attach_tag, &QID_DIR)).unwrap();
        let (_, walk_tag, _) = request(stream);
        // nwqid = 65535, no qids following: nine bytes total.
        let _ = stream.write_all(&reply(111, walk_tag, &65535u16.to_le_bytes()));
    });
    let mut pipeline = client.unwrap();
    let (attach_tag, root) = pipeline.attach(b"", b"").unwrap();
    assert!(matches!(
        pipeline.wait(attach_tag, deadline()).unwrap(),
        Reply::Attach(_)
    ));
    let (walk_tag, _reserved) = pipeline.walk(root, &[b"x"]).unwrap();
    assert_eq!(
        pipeline.wait(walk_tag, deadline()),
        Err(PipelineError::Protocol("Rwalk shape"))
    );
    assert!(pipeline.is_poisoned());
    thread.join().unwrap();
}

/// R4-7 (the violation half; the clone half is
/// `a_zero_name_walk_still_clones_the_fid`): walk(5) allows `nwqid == 0`
/// only for a zero-name clone. A failure at the first name must instead be
/// `Rlerror`, so an empty `Rwalk` answering a nonempty walk cannot be a
/// genuine partial walk and is a protocol violation.
#[test]
fn an_empty_rwalk_for_a_nonempty_walk_poisons() {
    let (client, thread) = scripted(|stream| {
        versioned(stream);
        let (_, attach_tag, _) = request(stream);
        stream.write_all(&reply(105, attach_tag, &QID_DIR)).unwrap();
        let (_, walk_tag, _) = request(stream);
        let _ = stream.write_all(&reply(111, walk_tag, &0u16.to_le_bytes()));
    });
    let mut pipeline = client.unwrap();
    let (attach_tag, root) = pipeline.attach(b"", b"").unwrap();
    assert!(matches!(
        pipeline.wait(attach_tag, deadline()).unwrap(),
        Reply::Attach(_)
    ));
    let (walk_tag, _reserved) = pipeline.walk(root, &[b"x"]).unwrap();
    assert_eq!(
        pipeline.wait(walk_tag, deadline()),
        Err(PipelineError::Protocol("empty Rwalk for a nonempty walk"))
    );
    assert!(pipeline.is_poisoned());
    thread.join().unwrap();
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

#[test]
fn a_reply_followed_at_once_by_close_is_still_delivered() {
    let (client, thread) = scripted(|stream| {
        versioned(stream);
        let (_, tag, _) = request(stream);
        stream.write_all(&reply(105, tag, &QID_DIR)).unwrap();
    });
    let mut pipeline = client.unwrap();
    let (tag, _fid) = pipeline.attach(b"", b"").unwrap();
    assert!(matches!(
        pipeline.wait(tag, deadline()),
        Ok(Reply::Attach(_))
    ));
    thread.join().unwrap();
}
