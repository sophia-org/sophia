//! Scripted, device-free runs of the generic native proof peer. The test
//! owner stands in for Session's live owner: it checks each delivered
//! candidate, settles it and republishes as a stage requires. These runs
//! prove the peer's stage contract and the transport with a supplied owner;
//! they carry no physical evidence. The native gate runs the same peer under
//! Session.

pub mod scripted;

use sophia_protocol::output_files::OutputFileLimits;
use sophia_protocol::*;
use sophia_runtime::*;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The fixture's published topology epoch, which baselines name.
pub const A_EPOCH: u64 = 7;
const LINE_BYTES: usize = 256;
const MAX_LINES: usize = 1024;
const STDERR_BYTES: u64 = 64 * 1024;
/// Longer than any peer deadline these scripts pass (default 30 s).
pub const RUN_BOUND: Duration = Duration::from_secs(45);
/// Exit, reap and pipe-reader bound once a run has ended or failed.
pub const CLEANUP_BOUND: Duration = Duration::from_secs(5);

/// One enabled head per group, both heads connected. Modes 1 (800x600) and
/// 2 (1024x768) exist on each head.
#[derive(Clone, Debug)]
pub struct Layout {
    pub heads: Vec<(u64, u64)>,
    pub groups: Vec<(u64, Rect, u64)>,
    pub primary: usize,
}

pub fn layout_a() -> Layout {
    Layout {
        heads: vec![(1, 1), (2, 1)],
        groups: vec![(1, rect(0, 800, 600), 1), (2, rect(800, 800, 600), 2)],
        primary: 0,
    }
}

/// Differs from A in head 1's mode, both geometries and the primary output.
pub fn layout_b() -> Layout {
    Layout {
        heads: vec![(1, 2), (2, 1)],
        groups: vec![(1, rect(0, 1024, 768), 1), (2, rect(1024, 800, 600), 2)],
        primary: 1,
    }
}

fn rect(x: i32, width: i32, height: i32) -> Rect {
    Rect {
        x,
        y: 0,
        width,
        height,
    }
}

impl Layout {
    /// The peer's compact layout arguments with the given prefix ("a"/"b").
    pub fn args(&self, prefix: &str) -> Vec<String> {
        let heads = self
            .heads
            .iter()
            .map(|(head, mode)| format!("{head}:{mode}:normal:disabled"))
            .collect::<Vec<_>>()
            .join(",");
        let groups = self
            .groups
            .iter()
            .map(|(output, r, head)| {
                format!(
                    "{output}@{},{},{}x{}={head}/exact",
                    r.x, r.y, r.width, r.height
                )
            })
            .collect::<Vec<_>>()
            .join(";");
        vec![
            format!("--{prefix}-heads={heads}"),
            format!("--{prefix}-groups={groups}"),
            format!("--{prefix}-primary={}", self.primary),
        ]
    }

    pub fn snapshot(&self, topology_epoch: u64) -> OutputAuthoritySnapshot {
        let modes = [(1, 800, 600), (2, 1024, 768)]
            .into_iter()
            .map(|(mode, width, height)| OutputModeDescriptor {
                mode: DisplayModeId::from_raw(mode),
                pixel_size: Size { width, height },
                refresh_millihz: 60_000,
                preferred: mode == 1,
            })
            .collect::<Vec<_>>();
        OutputAuthoritySnapshot {
            topology_epoch,
            primary_output: OutputId::from_raw(self.groups[self.primary].0),
            heads: [1, 2]
                .into_iter()
                .map(|head| {
                    let current = self.heads.iter().find(|(h, _)| *h == head);
                    OutputHeadDescriptor {
                        head: DisplayHeadId::from_raw(head),
                        generation: 1,
                        label: format!("panel-{head}"),
                        connected: true,
                        enabled: current.is_some(),
                        vrr_capable: false,
                        transforms: OutputTransformSet::ALL,
                        current_mode: current.map(|(_, mode)| DisplayModeId::from_raw(*mode)),
                        modes: modes.clone(),
                    }
                })
                .collect(),
            groups: self
                .groups
                .iter()
                .map(|(output, logical, head)| OutputLogicalGroupState {
                    output: OutputId::from_raw(*output),
                    generation: 1,
                    logical: *logical,
                    members: vec![OutputGroupMember {
                        head: DisplayHeadId::from_raw(*head),
                        mapping: OutputHeadMapping::Exact,
                    }],
                })
                .collect(),
        }
    }

    /// The candidate the peer must build from this layout and its argv
    /// transform/VRR intent (normal, disabled).
    pub fn candidate(
        &self,
        base_topology_epoch: u64,
        intent: OutputTopologyIntent,
    ) -> OutputTopologyCandidate {
        OutputTopologyCandidate {
            base_topology_epoch,
            intent,
            primary_group_index: self.primary as u16,
            heads: self
                .heads
                .iter()
                .map(|(head, mode)| OutputHeadTargetProposal {
                    head: DisplayHeadId::from_raw(*head),
                    head_generation: 1,
                    mode: DisplayModeId::from_raw(*mode),
                    transform: OutputTransform::Normal,
                    vrr: OutputVrrPolicy::Disabled,
                })
                .collect(),
            groups: self
                .groups
                .iter()
                .map(|(output, logical, head)| OutputLogicalGroupProposal {
                    output: OutputId::from_raw(*output),
                    logical: *logical,
                    members: vec![OutputGroupMember {
                        head: DisplayHeadId::from_raw(*head),
                        mapping: OutputHeadMapping::Exact,
                    }],
                })
                .collect(),
        }
    }
}

/// Stage argv: stage, optional deadline, epoch, A and optional B.
pub fn argv(stage: &str, deadline_ms: Option<u64>, epoch: u64, b: Option<&Layout>) -> Vec<String> {
    let mut args = vec![format!("--stage={stage}")];
    if let Some(ms) = deadline_ms {
        args.push(format!("--deadline-ms={ms}"));
    }
    args.push(format!("--a-topology-epoch={epoch}"));
    args.extend(layout_a().args("a"));
    if let Some(b) = b {
        args.extend(b.args("b"));
    }
    args
}

pub fn peer_binary() -> PathBuf {
    std::env::var_os("SOPHIA_OUTPUT_NATIVE_PROOF_PEER")
        .map(PathBuf::from)
        .expect("SOPHIA_OUTPUT_NATIVE_PROOF_PEER names the built proof peer")
}

/// One bounded evidence record: its event name and fields.
#[derive(Clone, Debug)]
pub struct Line {
    pub raw: String,
    pub event: String,
    pub fields: BTreeMap<String, String>,
}

impl Line {
    /// `stage` is the expected stage, or None where argv was refused.
    pub fn parse(raw: String, stage: Option<&str>) -> Result<Self, String> {
        let mut tokens = raw.split(' ');
        if tokens.next() != Some("sophia_output_proof") {
            return Err(format!("foreign record: {raw}"));
        }
        let mut fields = BTreeMap::new();
        for token in tokens {
            let (key, value) = token
                .split_once('=')
                .ok_or_else(|| format!("bare token in {raw}"))?;
            if fields.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(format!("repeated key in {raw}"));
            }
        }
        if fields.get("schema").map(String::as_str) != Some("1")
            || stage.is_some_and(|stage| fields.get("stage").map(String::as_str) != Some(stage))
            || fields
                .get("t")
                .and_then(|t| t.parse::<u64>().ok())
                .is_none()
        {
            return Err(format!("bad record header: {raw}"));
        }
        let event = fields.remove("event").ok_or("record without event")?;
        Ok(Self { raw, event, fields })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(String::as_str)
    }
}

pub type RawLine = Result<String, &'static str>;

/// The peer child and its pipe readers. Dropping it on any path, including
/// a panic, kills and reaps a child that has not been reaped.
pub struct PeerProcess {
    child: Child,
    status: Option<ExitStatus>,
    lines: mpsc::Receiver<RawLine>,
    reader: Option<JoinHandle<()>>,
    diagnostics: Option<JoinHandle<String>>,
}

/// What a finished peer left: status, unread records, stderr, and every
/// cleanup failure in order.
pub struct Finished {
    pub status: Option<ExitStatus>,
    pub lines: Vec<RawLine>,
    pub stderr: String,
    pub errors: Vec<String>,
}

impl PeerProcess {
    pub fn spawn(args: &[String], socket: Option<&Path>) -> Self {
        let mut command = Command::new(peer_binary());
        command
            .args(args)
            .env_remove("SOPHIA_OUTPUT_SOCKET")
            .env_remove("SOPHIA_OUTPUT_9P_SOCKET")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(socket) = socket {
            command.env("SOPHIA_OUTPUT_9P_SOCKET", socket);
        }
        let mut child = command.spawn().expect("spawn proof peer");
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (sender, lines) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut raw = Vec::new();
                match (&mut reader)
                    .take(LINE_BYTES as u64)
                    .read_until(b'\n', &mut raw)
                {
                    Ok(0) => return,
                    Ok(_) if raw.last() == Some(&b'\n') => {
                        raw.pop();
                        let line = String::from_utf8(raw).map_err(|_| "non-UTF-8 record");
                        if sender.send(line).is_err() {
                            return;
                        }
                    }
                    _ => {
                        let _ = sender.send(Err("unterminated or oversized record"));
                        return;
                    }
                }
            }
        });
        let diagnostics = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stderr.take(STDERR_BYTES).read_to_string(&mut text);
            text
        });
        Self {
            child,
            status: None,
            lines,
            reader: Some(reader),
            diagnostics: Some(diagnostics),
        }
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn try_line(&self) -> Option<RawLine> {
        self.lines.try_recv().ok()
    }

    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, String> {
        if self.status.is_none() {
            self.status = self.child.try_wait().map_err(|e| e.to_string())?;
        }
        Ok(self.status)
    }

    /// Only an unreaped child is signalled: never a recycled PID.
    pub fn kill(&mut self) -> Result<(), String> {
        if self.status.is_none() {
            self.child.kill().map_err(|e| format!("kill peer: {e}"))?;
        }
        Ok(())
    }

    /// Wait for exit until `deadline`, then kill; reap; join both readers
    /// within the cleanup bound. A reader still blocked is left detached and
    /// reported, never waited on without bound.
    pub fn finish(mut self, deadline: Instant) -> Finished {
        let mut errors = Vec::new();
        loop {
            match self.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(1))
                }
                Ok(None) => {
                    errors.push("peer outlived its cleanup deadline; killed".to_owned());
                    if let Err(error) = self.kill() {
                        errors.push(error);
                    }
                    match self.child.wait() {
                        Ok(status) => self.status = Some(status),
                        Err(error) => errors.push(format!("reap peer: {error}")),
                    }
                    break;
                }
                Err(error) => {
                    errors.push(format!("wait peer: {error}"));
                    let _ = self.kill();
                    self.status = self.child.wait().ok();
                    break;
                }
            }
        }
        let join_deadline = Instant::now() + CLEANUP_BOUND;
        let reader = self.reader.take().unwrap();
        let diagnostics = self.diagnostics.take().unwrap();
        while !(reader.is_finished() && diagnostics.is_finished()) && Instant::now() < join_deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
        if reader.is_finished() {
            let _ = reader.join();
        } else {
            errors.push("stdout reader still blocked after cleanup bound".to_owned());
        }
        let stderr = if diagnostics.is_finished() {
            diagnostics.join().unwrap_or_default()
        } else {
            errors.push("stderr reader still blocked after cleanup bound".to_owned());
            String::new()
        };
        Finished {
            status: self.status,
            lines: self.lines.try_iter().collect(),
            stderr,
            errors,
        }
    }
}

impl Drop for PeerProcess {
    fn drop(&mut self) {
        if self.status.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Owner inputs in arrival order: service events and peer records.
pub enum Input<'a> {
    Event(OutputFileServiceEvent),
    Line(&'a Line),
}

pub struct Owner<'a> {
    service: &'a OutputFileService,
    peer: &'a mut PeerProcess,
    pub connection_epoch: Option<u64>,
    pub killed: bool,
}

impl Owner<'_> {
    pub fn settle(
        &self,
        transaction: TransactionId,
        topology_epoch: u64,
        kind: OutputV1OutcomeKind,
    ) -> Result<(), String> {
        let connection_epoch = self.connection_epoch.ok_or("settle without connection")?;
        self.command(OutputFileServiceCommand::Settle {
            transaction,
            outcome: OutputV1Outcome {
                connection_epoch,
                topology_epoch,
                kind,
                reason: 0,
            },
        })
    }

    pub fn publish(&self, snapshot: OutputAuthoritySnapshot) -> Result<(), String> {
        self.command(OutputFileServiceCommand::PublishSnapshot(snapshot))
    }

    fn command(&self, mut command: OutputFileServiceCommand) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.service.command(command) {
                Ok(()) => return Ok(()),
                Err(returned) => command = returned,
            }
            if Instant::now() >= deadline {
                return Err("owner command handoff timeout".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Stand-in for the supervisor's termination after native apply: the
    /// peer is still connected and has not been answered.
    pub fn terminate_peer(&mut self) -> Result<(), String> {
        self.peer.kill()?;
        self.killed = true;
        Ok(())
    }
}

pub struct Run {
    pub status: ExitStatus,
    pub lines: Vec<Line>,
    pub stderr: String,
    pub deliveries: Vec<AdmittedOutputProposal>,
    pub admission_rejections: Vec<TransactionId>,
    pub connected: u32,
    pub disconnected: u32,
}

impl Run {
    pub fn code(&self) -> Option<i32> {
        self.status.code()
    }

    pub fn events(&self) -> Vec<&str> {
        self.lines.iter().map(|line| line.event.as_str()).collect()
    }

    pub fn only(&self, event: &str) -> &Line {
        let found = self
            .lines
            .iter()
            .filter(|line| line.event == event)
            .collect::<Vec<_>>();
        assert_eq!(found.len(), 1, "one {event} record expected: {self:#?}");
        found[0]
    }

    pub fn fail_reason(&self) -> Option<&str> {
        self.lines
            .iter()
            .find(|line| line.event == "fail")
            .and_then(|line| line.get("reason"))
    }

    pub fn published(&self) -> Vec<(&str, &str)> {
        self.lines
            .iter()
            .filter(|line| line.event == "topology")
            .map(|line| {
                (
                    line.get("topology_epoch").unwrap(),
                    line.get("match").unwrap(),
                )
            })
            .collect()
    }

    /// Every evidence rule shared by all runs.
    pub fn check_records(&self, killed: bool) {
        assert!(!self.lines.is_empty(), "no records: {self:#?}");
        let times = self
            .lines
            .iter()
            .map(|line| line.get("t").unwrap().parse::<u64>().unwrap())
            .collect::<Vec<_>>();
        assert!(times.windows(2).all(|w| w[0] <= w[1]), "{self:#?}");
        let terminal = self
            .lines
            .iter()
            .filter(|line| line.event == "pass" || line.event == "fail")
            .count();
        if killed {
            assert_eq!(self.status.signal(), Some(9), "{self:#?}");
            assert_eq!(terminal, 0, "killed peer wrote a verdict: {self:#?}");
        } else {
            assert_eq!(terminal, 1, "{self:#?}");
            let last = self.lines.last().unwrap();
            assert!(last.event == "pass" || last.event == "fail", "{self:#?}");
            assert_eq!(self.code() == Some(0), last.event == "pass", "{self:#?}");
        }
    }
}

impl std::fmt::Debug for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "status: {}", self.status)?;
        for line in &self.lines {
            writeln!(f, "  {}", line.raw)?;
        }
        writeln!(
            f,
            "deliveries={} admission_rejections={} connected={} disconnected={}",
            self.deliveries.len(),
            self.admission_rejections.len(),
            self.connected,
            self.disconnected
        )?;
        write!(f, "stderr: {}", self.stderr)
    }
}

pub fn empty_run() -> Run {
    Run {
        status: ExitStatus::from_raw(0),
        lines: Vec::new(),
        stderr: String::new(),
        deliveries: Vec::new(),
        admission_rejections: Vec::new(),
        connected: 0,
        disconnected: 0,
    }
}

/// Fold a finished peer into its run. The first run failure, if any, leads
/// the panic; cleanup failures and all evidence follow it.
pub fn conclude(
    mut run: Run,
    finished: Finished,
    stage: Option<&str>,
    first_failure: Option<String>,
) -> Run {
    let mut errors = finished.errors;
    for raw in finished.lines {
        match raw
            .map_err(String::from)
            .and_then(|raw| Line::parse(raw, stage))
        {
            Ok(line) if run.lines.len() < MAX_LINES => run.lines.push(line),
            Ok(_) => errors.push("record count bound".into()),
            Err(error) => errors.push(error),
        }
    }
    run.stderr = finished.stderr;
    match finished.status {
        Some(status) => run.status = status,
        None => errors.push("peer status unavailable".into()),
    }
    if first_failure.is_some() || !errors.is_empty() {
        panic!(
            "{}\ncleanup: {errors:?}\n{run:#?}",
            first_failure.as_deref().unwrap_or("cleanup failed")
        );
    }
    run
}

/// Take every record the peer has written so far, in order.
pub fn take_lines(
    peer: &PeerProcess,
    stage: &str,
    run: &mut Run,
    mut each: impl FnMut(&Line) -> Result<(), String>,
) -> Result<bool, String> {
    let mut any = false;
    while let Some(raw) = peer.try_line() {
        any = true;
        let line = Line::parse(raw?, Some(stage))?;
        if run.lines.len() == MAX_LINES {
            return Err("record count bound".into());
        }
        run.lines.push(line);
        each(run.lines.last().unwrap())?;
    }
    Ok(any)
}

/// Run the peer without an endpoint: argv refusals happen before any
/// connection. Bounded like every other run.
pub fn run_offline(args: &[String]) -> Run {
    let peer = PeerProcess::spawn(args, None);
    let finished = peer.finish(Instant::now() + CLEANUP_BOUND);
    conclude(empty_run(), finished, None, None)
}

/// Spawn the peer against a real service publishing `initial`, feed every
/// service event and peer record to `owner`, and return after the peer
/// exits and its connection has been released.
pub fn run(
    label: &str,
    stage: &str,
    args: &[String],
    initial: OutputAuthoritySnapshot,
    mut owner: impl FnMut(&mut Owner<'_>, Input<'_>) -> Result<(), String>,
) -> Run {
    let directory = std::env::temp_dir().join(format!(
        "sophia-native-proof-{}-{label}",
        std::process::id()
    ));
    let mut transport = OutputFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        1,
        OutputFileLimits::default(),
    )
    .expect("bind output files endpoint");
    // The listener exists before the peer starts and nothing is accepted
    // until the service runs, which is after authorization. No gate needed.
    let mut peer = PeerProcess::spawn(args, Some(transport.socket_path()));
    transport
        .authorize_supervised_pid(peer.id())
        .expect("authorize proof peer");
    let service = OutputFileService::spawn(transport, initial).expect("spawn output service");
    let mut run = empty_run();
    let mut state = Owner {
        service: &service,
        peer: &mut peer,
        connection_epoch: None,
        killed: false,
    };
    let deadline = Instant::now() + RUN_BOUND;
    let mut released_at = None;
    let result = (|| -> Result<(), String> {
        loop {
            if Instant::now() >= deadline {
                return Err("scripted run exceeded its outer deadline".into());
            }
            let mut idle = true;
            while let Some(raw) = state.peer.try_line() {
                idle = false;
                let line = Line::parse(raw?, Some(stage))?;
                if run.lines.len() == MAX_LINES {
                    return Err("record count bound".into());
                }
                run.lines.push(line);
                owner(&mut state, Input::Line(run.lines.last().unwrap()))?;
            }
            if let Some(event) = service.try_event().map_err(|e| e.to_string())? {
                idle = false;
                match &event {
                    OutputFileServiceEvent::Connected { connection_epoch } => {
                        run.connected += 1;
                        state.connection_epoch = Some(*connection_epoch);
                    }
                    OutputFileServiceEvent::Disconnected { .. } => run.disconnected += 1,
                    OutputFileServiceEvent::Proposal {
                        proposal,
                        admission,
                    } => {
                        if *admission != OutputProposalAdmission::Active {
                            return Err(format!("non-active admission: {admission:?}"));
                        }
                        run.deliveries.push(proposal.clone());
                    }
                    OutputFileServiceEvent::ProposalRejected { transaction, .. } => {
                        run.admission_rejections.push(*transaction)
                    }
                    other => return Err(format!("unexpected service event: {other:?}")),
                }
                owner(&mut state, Input::Event(event))?;
            }
            // Done once the peer has exited, its records are read and the
            // service has released the connection it admitted.
            if state.peer.try_wait()?.is_some() && run.connected == run.disconnected {
                let since = *released_at.get_or_insert_with(Instant::now);
                // Let a late record or event arrive before closing the books.
                if since.elapsed() >= Duration::from_millis(50) {
                    return Ok(());
                }
            }
            if idle {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
    })();
    let killed = state.killed;
    if result.is_err() {
        let _ = peer.kill();
    }
    // Dropping the service closes the connection, which ends a peer that is
    // still waiting on it.
    drop(service);
    let finished = peer.finish(Instant::now() + CLEANUP_BOUND);
    let run = conclude(run, finished, Some(stage), result.err());
    assert!(
        !directory.exists(),
        "endpoint directory survived service drop"
    );
    run.check_records(killed);
    assert_eq!(run.connected, 1, "{run:#?}");
    run
}

#[test]
fn fixtures_are_valid_and_b_differs_verifiably() {
    let a = layout_a().snapshot(A_EPOCH);
    let b = layout_b().snapshot(A_EPOCH + 1);
    a.validate().unwrap();
    b.validate().unwrap();
    for (layout, snapshot) in [(layout_a(), &b), (layout_b(), &a)] {
        for intent in [
            OutputTopologyIntent::ValidateOnly,
            OutputTopologyIntent::Apply,
        ] {
            layout
                .candidate(snapshot.topology_epoch, intent)
                .validate_against(snapshot)
                .unwrap();
        }
    }
    assert_ne!(a.primary_output, b.primary_output);
    assert_ne!(a.heads[0].current_mode, b.heads[0].current_mode);
    assert_ne!(a.groups[0].logical, b.groups[0].logical);
    // The compact argv stays within the peer's nine arguments.
    assert_eq!(
        argv("validate", Some(1), A_EPOCH, Some(&layout_b())).len(),
        9
    );
}
