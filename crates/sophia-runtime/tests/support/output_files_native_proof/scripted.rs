//! A scripted output file source: the real export behind the public 9P
//! server, wrapped so that one chosen publication is announced under the
//! previous publication's Qid, in both its ObjectPublished event and the
//! topology walk. The real export allocates every Qid fresh, so it cannot
//! show the peer refusing reuse. Nothing else on the wire changes; without
//! reuse the same source must carry a complete commit-restore stage.

use super::*;
use sophia_9p::unix::Server;
use sophia_9p::{
    Access, AttachContext, Attachment, DirEntry, Entry, Errno, Export, Limits, OpenFlags,
    ReadOutcome, WalkName,
};
use sophia_protocol::output_files::{
    OutputFileClass, OutputFileKind, decode_output_file_publication, decode_output_file_record,
};
use std::os::unix::net::UnixListener;

/// ObjectPublished: 32-byte header, then kind, reserved and topology epoch.
const PUBLICATION_QID_OFFSET: usize = 32 + 16;

struct ReusingExport {
    inner: OutputFileExport,
    /// Every event byte read so far, from offset zero.
    events: Vec<u8>,
    parsed: usize,
    last_announced: Option<u64>,
    /// (real Qid, announced Qid) for the one reused publication.
    reuse: Option<(u64, u64)>,
    /// (absolute event offset of a Qid field, announced Qid).
    patches: Vec<(usize, u64)>,
}

impl ReusingExport {
    fn new(inner: OutputFileExport) -> Self {
        Self {
            inner,
            events: Vec::new(),
            parsed: 0,
            last_announced: None,
            reuse: None,
            patches: Vec::new(),
        }
    }

    /// Publish, announcing this object under the last announced Qid.
    fn publish_reusing(&mut self, snapshot: &OutputAuthoritySnapshot) -> Result<(), Errno> {
        let announced = self.last_announced.ok_or(Errno::EINVAL)?;
        let real = self.inner.publish(snapshot)?;
        self.reuse = Some((real, announced));
        Ok(())
    }

    /// Track the event stream and patch the reused publication's Qid in
    /// every read that covers it.
    fn observe(&mut self, offset: u64, bytes: &mut [u8]) -> Result<(), Errno> {
        let start = usize::try_from(offset).map_err(|_| Errno::EIO)?;
        if start > self.events.len() {
            return Err(Errno::EIO);
        }
        let end = start + bytes.len();
        if end > self.events.len() {
            let known = self.events.len() - start;
            self.events.extend_from_slice(&bytes[known..]);
        }
        while self.events.len() - self.parsed >= 4 {
            let at = self.parsed;
            let size = u32::from_le_bytes(self.events[at..at + 4].try_into().unwrap()) as usize;
            if self.events.len() - at < size {
                break;
            }
            let record =
                decode_output_file_record(&self.events[at..at + size], OutputFileClass::Event)
                    .map_err(|_| Errno::EIO)?;
            if record.header.kind == OutputFileKind::ObjectPublished {
                let published =
                    decode_output_file_publication(record.body).map_err(|_| Errno::EIO)?;
                let announced = match self.reuse {
                    Some((real, announced)) if real == published.qid_path => {
                        self.patches.push((at + PUBLICATION_QID_OFFSET, announced));
                        announced
                    }
                    _ => published.qid_path,
                };
                self.last_announced = Some(announced);
            }
            self.parsed += size;
        }
        for &(at, announced) in &self.patches {
            for (index, byte) in announced.to_le_bytes().into_iter().enumerate() {
                if (start..end).contains(&(at + index)) {
                    bytes[at + index - start] = byte;
                }
            }
        }
        Ok(())
    }
}

impl Export for ReusingExport {
    type Node = OutputFileNode;
    type Handle = OutputFileHandle;

    fn attach(&mut self, context: &AttachContext<'_>) -> Result<Attachment<Self::Node>, Errno> {
        self.inner.attach(context)
    }

    fn check(&mut self, access: &Access<'_, Self::Node>) -> Result<(), Errno> {
        self.inner.check(access)
    }

    fn lookup(&mut self, directory: &Self::Node, name: WalkName<'_>) -> Result<Self::Node, Errno> {
        self.inner.lookup(directory, name)
    }

    fn describe(&self, node: &Self::Node, handle: Option<&Self::Handle>) -> Entry {
        let mut entry = self.inner.describe(node, handle);
        if matches!(node, OutputFileNode::Topology)
            && let Some((real, announced)) = self.reuse
            && entry.qid_path == real
        {
            entry.qid_path = announced;
        }
        entry
    }

    fn open(&mut self, node: &Self::Node, flags: OpenFlags) -> Result<Self::Handle, Errno> {
        self.inner.open(node, flags)
    }

    fn read(
        &mut self,
        node: &Self::Node,
        handle: &mut Self::Handle,
        offset: u64,
        count: u32,
    ) -> Result<ReadOutcome, Errno> {
        match self.inner.read(node, handle, offset, count)? {
            ReadOutcome::Ready(mut bytes) if matches!(node, OutputFileNode::Events) => {
                self.observe(offset, &mut bytes)?;
                Ok(ReadOutcome::Ready(bytes))
            }
            outcome => Ok(outcome),
        }
    }

    fn write(
        &mut self,
        node: &Self::Node,
        handle: &mut Self::Handle,
        offset: u64,
        data: &[u8],
    ) -> Result<u32, Errno> {
        self.inner.write(node, handle, offset, data)
    }

    fn readdir(
        &mut self,
        directory: &Self::Node,
        handle: &mut Self::Handle,
        cookie: u64,
        max_entries: usize,
    ) -> Result<Vec<DirEntry>, Errno> {
        self.inner.readdir(directory, handle, cookie, max_entries)
    }

    fn release(&mut self, node: Self::Node, handle: Option<Self::Handle>) {
        self.inner.release(node, handle)
    }
}

/// The commit-restore stage against the scripted source. With `reuse`, the
/// first committed publication (B) is announced under the baseline's Qid.
pub fn commit_restore(label: &str, reuse: bool) -> Run {
    let stage = "commit-restore";
    let (a, b) = (layout_a(), layout_b());
    let directory = std::env::temp_dir().join(format!(
        "sophia-native-proof-scripted-{}-{label}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).expect("scripted endpoint directory");
    let socket = directory.join("output");
    let listener = UnixListener::bind(&socket).expect("scripted endpoint");
    listener.set_nonblocking(true).unwrap();
    let mut peer = PeerProcess::spawn(&argv(stage, None, A_EPOCH, Some(&b)), Some(&socket));
    let mut run = empty_run();
    let result = (|| -> Result<(), String> {
        let deadline = Instant::now() + RUN_BOUND;
        let stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(format!("accept: {error}")),
            }
            if Instant::now() >= deadline || peer.try_wait()?.is_some() {
                return Err("peer never connected".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        let export = OutputFileExport::new(
            1,
            OutputFileLimits::default(),
            a.snapshot(A_EPOCH),
            OutputFileQids::default(),
        )
        .map_err(|error| format!("export: {error:?}"))?;
        // The runtime transport's per-connection 9P limits.
        let limits = Limits::new(65_536, 512, 16, 32, 131_072, 1)
            .map_err(|error| format!("limits: {error:?}"))?;
        let mut server =
            Server::new(ReusingExport::new(export), limits).map_err(|e| e.to_string())?;
        let connection = server
            .adopt(stream)
            .map_err(|refused| refused.error.to_string())?;
        server
            .export_mut()
            .inner
            .bind_connection(connection)
            .map_err(|error| format!("bind: {error:?}"))?;
        let expected = [
            (
                b.candidate(A_EPOCH, OutputTopologyIntent::Apply),
                b.snapshot(A_EPOCH + 1),
            ),
            (
                a.candidate(A_EPOCH + 1, OutputTopologyIntent::Apply),
                a.snapshot(A_EPOCH + 2),
            ),
        ];
        // A publication waits while the previous one is still unread, as
        // the service's does.
        let mut pending: Option<(OutputAuthoritySnapshot, bool)> = None;
        let mut exited_at = None;
        loop {
            if Instant::now() >= deadline {
                return Err("scripted run exceeded its outer deadline".into());
            }
            server.export_mut().inner.expire(Instant::now());
            server
                .turn(Some(Duration::from_millis(1)))
                .map_err(|e| format!("turn: {e}"))?;
            while let Some(delivery) = server.export_mut().inner.take_delivery() {
                match delivery {
                    OutputFileSubmission::Negotiated(_) => run.connected += 1,
                    OutputFileSubmission::Replayed => {}
                    OutputFileSubmission::Proposal {
                        proposal,
                        admission: OutputProposalAdmission::Active,
                    } => {
                        let (want, published) = expected
                            .get(run.deliveries.len())
                            .ok_or("more than two deliveries")?
                            .clone();
                        if proposal.message.candidate != want {
                            return Err(format!("unexpected candidate: {proposal:?}"));
                        }
                        run.deliveries.push(proposal.clone());
                        server
                            .export_mut()
                            .inner
                            .settle(
                                proposal.transaction,
                                OutputV1Outcome {
                                    connection_epoch: 1,
                                    topology_epoch: published.topology_epoch,
                                    kind: OutputV1OutcomeKind::Committed,
                                    reason: 0,
                                },
                            )
                            .map_err(|error| format!("settle: {error:?}"))?;
                        pending = Some((published, reuse && run.deliveries.len() == 1));
                        server.wake().wake();
                    }
                    other => return Err(format!("unexpected delivery: {other:?}")),
                }
            }
            if let Some((snapshot, reused)) = &pending {
                let export = server.export_mut();
                let published = if *reused {
                    export.publish_reusing(snapshot)
                } else {
                    export.inner.publish(snapshot).map(|_| ())
                };
                match published {
                    Ok(()) => {
                        pending = None;
                        server.wake().wake();
                    }
                    Err(Errno::EAGAIN) => {}
                    Err(error) => return Err(format!("publish: {error:?}")),
                }
            }
            take_lines(&peer, stage, &mut run, |_| Ok(()))?;
            if peer.try_wait()?.is_some() {
                let since = *exited_at.get_or_insert_with(Instant::now);
                if since.elapsed() >= Duration::from_millis(50) {
                    return Ok(());
                }
            }
        }
    })();
    if result.is_err() {
        let _ = peer.kill();
    }
    drop(listener);
    let finished = peer.finish(Instant::now() + CLEANUP_BOUND);
    let removed = std::fs::remove_dir_all(&directory);
    let run = conclude(run, finished, Some(stage), result.err());
    removed.expect("remove scripted endpoint directory");
    run.check_records(false);
    assert_eq!(run.connected, 1, "{run:#?}");
    run
}
