mod cpu;
mod fixture;
mod process;
mod records;

use records::Record;
use serde_json::json;
use sophia_protocol::output_files::OutputFileLimits;
use sophia_protocol::*;
use sophia_runtime::*;
use std::collections::BTreeSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

struct Evidence(File, Vec<serde_json::Value>, usize);
impl Evidence {
    fn record(&mut self, value: serde_json::Value) -> Result<(), String> {
        if self.1.len() >= 16_384 {
            let terminal = matches!(
                value["kind"].as_str(),
                Some("workload_result" | "cleanup_result" | "peer_stderr" | "threads" | "result")
            );
            if !terminal {
                self.2 += 1;
                return Err("workload evidence entry bound exceeded".into());
            }
            // A bound failure must still reach teardown and flush. Reserve
            // space for its terminal records; never recurse into that failure.
            if self.1.len() >= 16_416 {
                self.2 += 1;
                return Ok(());
            }
        }
        self.1.push(value);
        Ok(())
    }

    fn flush(&mut self) -> Result<(), String> {
        if self.2 != 0 {
            self.1
                .push(json!({"kind":"evidence_overflow", "entries_dropped":self.2}));
            self.2 = 0;
        }
        for value in self.1.drain(..) {
            serde_json::to_writer(&mut self.0, &value).map_err(|e| e.to_string())?;
            writeln!(self.0).map_err(|e| e.to_string())?;
        }
        self.0.flush().map_err(|e| e.to_string())
    }
}

pub fn run() -> Result<(), String> {
    let path = std::env::var_os("SOPHIA_OUTPUT_PERF_EVIDENCE").ok_or("missing evidence path")?;
    let peer = std::env::var_os("SOPHIA_OUTPUT_PERF_PEER").ok_or("missing C peer path")?;
    let mut evidence = Evidence(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?,
        Vec::new(),
        0,
    );
    evidence.record(json!({"kind":"header", "profile":"release", "connect_samples":100,
        "proposal_warmups":20, "proposal_samples":1000, "idle_intervals":3, "idle_seconds":10,
        "connect_p99_ms":100, "connect_max_ms":500, "proposal_p99_ms":50, "proposal_max_ms":250,
        "idle_percent_one_core":2, "connection_teardown":"excluded; next sample waits for Disconnected",
        "idle_worker":"periodic 1ms sleep; context switches reported; no wakeup threshold"}))?;
    let result = (|| {
        for maximum in [false, true] {
            for mode in ["connect", "proposals", "idle"] {
                workload(Path::new(&peer), maximum, mode, &mut evidence)?;
            }
        }
        Ok(())
    })();
    evidence
        .record(json!({"kind":"result", "passed":result.is_ok(), "error":result.as_ref().err()}))?;
    evidence.flush()?;
    result
}

struct Owner {
    epoch: Option<u64>,
    connected: u64,
    disconnected: u64,
    deliveries: BTreeSet<(u64, u64)>,
    retries: u64,
}
impl Owner {
    fn event(
        &mut self,
        event: OutputFileServiceEvent,
        service: &OutputFileService,
        snapshot: &OutputAuthoritySnapshot,
        mode: &str,
        evidence: &mut Evidence,
    ) -> Result<(), String> {
        match event {
            OutputFileServiceEvent::Connected { connection_epoch } => {
                if self.epoch.is_some() || connection_epoch != self.connected + 1 {
                    return Err("unexpected connection epoch".into());
                }
                self.epoch = Some(connection_epoch);
                self.connected += 1;
                evidence.record(json!({"kind":"connected", "epoch":connection_epoch}))?;
            }
            OutputFileServiceEvent::Disconnected { connection_epoch } => {
                if self.epoch != Some(connection_epoch) {
                    return Err("unexpected disconnect epoch".into());
                }
                self.epoch = None;
                self.disconnected += 1;
                evidence.record(json!({"kind":"disconnected", "epoch":connection_epoch}))?;
            }
            OutputFileServiceEvent::Proposal {
                proposal,
                admission: OutputProposalAdmission::Active,
            } => {
                let epoch = proposal.message.connection_epoch;
                if mode != "proposals"
                    || self.epoch != Some(epoch)
                    || proposal.message.candidate != fixture::candidate(snapshot)
                {
                    return Err("unexpected owner candidate".into());
                }
                if !self.deliveries.insert((epoch, proposal.transaction.raw())) {
                    return Err("duplicate owner delivery".into());
                }
                let mut command = OutputFileServiceCommand::Settle {
                    transaction: proposal.transaction,
                    outcome: OutputV1Outcome {
                        connection_epoch: epoch,
                        topology_epoch: snapshot.topology_epoch,
                        kind: OutputV1OutcomeKind::Validated,
                        reason: 0,
                    },
                };
                let deadline = Instant::now() + Duration::from_secs(2);
                loop {
                    match service.command(command) {
                        Ok(()) => break,
                        Err(returned) => command = returned,
                    }
                    self.retries += 1;
                    if Instant::now() >= deadline {
                        return Err("owner settlement handoff timeout".into());
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                evidence.record(json!({"kind":"owner_settled", "epoch":epoch,
                    "transaction":proposal.transaction.raw()}))?;
            }
            other => return Err(format!("unexpected owner event: {other:?}")),
        }
        Ok(())
    }
}

fn workload(
    binary: &Path,
    maximum: bool,
    mode: &str,
    evidence: &mut Evidence,
) -> Result<(), String> {
    let label = if maximum { "max" } else { "small" };
    let snapshot = fixture::snapshot(maximum);
    snapshot.validate().map_err(|e| format!("fixture: {e:?}"))?;
    evidence.record(json!({"kind":"workload", "fixture":label, "mode":mode, "identity":fixture::identity(&snapshot)}))?;
    let baseline = cpu::snapshot(&[std::process::id()])?;
    let directory =
        std::env::temp_dir().join(format!("sophia-perf-{}-{label}-{mode}", std::process::id()));
    let mut transport = OutputFileTransport::bind_for_supervised_uid(
        &directory,
        rustix::process::geteuid().as_raw(),
        1,
        OutputFileLimits::default(),
    )
    .map_err(|e| format!("bind: {e:?}"))?;
    let mut peer = process::Peer::spawn(binary, transport.socket_path(), mode, label)?;
    transport
        .authorize_supervised_pid(peer.id())
        .map_err(|e| format!("authorize: {e:?}"))?;
    let service =
        OutputFileService::spawn(transport, snapshot.clone()).map_err(|e| e.to_string())?;
    let mut owner = Owner {
        epoch: None,
        connected: 0,
        disconnected: 0,
        deliveries: BTreeSet::new(),
        retries: 0,
    };
    let mut outcomes = BTreeSet::new();
    let mut samples = Vec::new();
    let mut warmups = 0;
    let mut ready = false;
    let mut done = false;
    let mut next_connect = 1;
    let mut idle_passed = true;
    let mut during = None;
    peer.command(b'G')?;
    if mode == "connect" {
        peer.command(b'N')?;
    }
    let started = Instant::now();
    let result = (|| {
        while !done {
            if started.elapsed() > Duration::from_secs(600) {
                return Err("workload timeout".into());
            }
            match service.event_timeout(Duration::from_millis(1)) {
                Ok(event) => owner.event(event, &service, &snapshot, mode, evidence)?,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(e) => return Err(format!("owner channel: {e}")),
            }
            peer.drain()?;
            while let Some(line) = peer.lines.pop_front() {
                evidence.record(json!({"kind":"peer_record", "line":line}))?;
                match records::parse(&line)? {
                    Record::Sample {
                        workload,
                        index,
                        start,
                        end,
                        epoch,
                        transaction,
                    } => {
                        if mode == "connect" {
                            if workload != "connect"
                                || index != samples.len() as u64
                                || epoch != index + 1
                                || transaction != 0
                            {
                                return Err("invalid connect sample identity".into());
                            }
                            samples.push(end - start);
                        } else if mode == "proposals" {
                            if epoch != 1
                                || transaction == 0
                                || !outcomes.insert((epoch, transaction))
                            {
                                return Err("duplicate or invalid outcome".into());
                            }
                            if workload == "warmup"
                                && warmups < 20
                                && samples.is_empty()
                                && index == warmups
                            {
                                warmups += 1;
                            } else if workload == "proposals"
                                && warmups == 20
                                && index == samples.len() as u64
                            {
                                samples.push(end - start);
                            } else {
                                return Err("invalid proposal sample order".into());
                            }
                        } else {
                            return Err("sample in idle workload".into());
                        }
                    }
                    Record::Ready { workload, epoch } => {
                        if ready || workload != mode || epoch != 1 || mode == "connect" {
                            return Err("unexpected READY".into());
                        }
                        ready = true;
                        during = Some(cpu::snapshot(&[std::process::id()])?);
                        if mode == "proposals" {
                            peer.command(b'N')?;
                        } else {
                            for index in 0..3 {
                                let before = cpu::snapshot(&[std::process::id(), peer.id()])?;
                                let start = Instant::now();
                                std::thread::sleep(Duration::from_secs(10));
                                let after = cpu::snapshot(&[std::process::id(), peer.id()])?;
                                let value = cpu::interval(&before, &after, start.elapsed())?;
                                idle_passed &= value["passed"] == true;
                                evidence.record(json!({"kind":"idle_interval", "index":index, "measurement":value}))?;
                                peer.drain()?;
                                if let Some(event) =
                                    service.try_event().map_err(|e| e.to_string())?
                                {
                                    return Err(format!("idle owner event: {event:?}"));
                                }
                            }
                            peer.command(b'X')?;
                        }
                    }
                    Record::Done { workload, count } => {
                        let expected = match mode {
                            "connect" => 100,
                            "proposals" => 1020,
                            _ => 0,
                        };
                        if workload != mode || count != expected {
                            return Err("wrong DONE count".into());
                        }
                        done = true;
                    }
                    Record::Failure { .. } => return Err(format!("C peer failure: {line}")),
                }
            }
            if during.is_none() && owner.epoch.is_some() {
                during = Some(cpu::snapshot(&[std::process::id()])?);
            }
            if mode == "connect"
                && next_connect < 100
                && owner.disconnected == next_connect
                && samples.len() as u64 == next_connect
            {
                peer.command(b'N')?;
                next_connect += 1;
            }
            if peer.stdout_closed && !done {
                return Err("peer exited before DONE".into());
            }
        }
        if mode != "idle" {
            peer.command(b'X')?;
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            peer.drain()?;
            if !peer.lines.is_empty() {
                return Err("extra records after DONE".into());
            }
            if let Some(event) = service.try_event().map_err(|e| e.to_string())? {
                owner.event(event, &service, &snapshot, mode, evidence)?;
            }
            if let Some(status) = peer.exited()? {
                if !status.success() {
                    return Err(format!("peer exit: {status}"));
                }
                if owner.epoch.is_none() && peer.stdout_closed {
                    break;
                }
            }
            if Instant::now() >= deadline {
                return Err("peer teardown timeout".into());
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let expected_connections = if mode == "connect" { 100 } else { 1 };
        if owner.connected != expected_connections || owner.disconnected != expected_connections {
            return Err("connection count mismatch".into());
        }
        records::reconcile(&owner.deliveries, &outcomes)?;
        let passed = if mode == "idle" {
            ready && idle_passed
        } else {
            let (count, p99, max) = if mode == "connect" {
                (100, 100_000_000, 500_000_000)
            } else {
                (1000, 50_000_000, 250_000_000)
            };
            if samples.len() != count {
                return Err("sample count mismatch".into());
            }
            let tail = records::percentile(&samples, 99);
            let worst = *samples.iter().max().unwrap();
            evidence.record(
                json!({"kind":"latency_summary", "p99_ns":tail,"max_ns":worst,"count":count}),
            )?;
            tail <= p99 && worst <= max
        };
        evidence.record(json!({"kind":"workload_summary", "passed":passed,"command_retries":owner.retries,
            "wall_ns":started.elapsed().as_nanos().to_string(), "deliveries":owner.deliveries.len()}))?;
        if !passed {
            return Err("declared performance bound failed".into());
        }
        Ok(())
    })();
    evidence.record(
        json!({"kind":"workload_result", "fixture":label, "mode":mode,
        "passed":result.is_ok(), "error":result.as_ref().err()}),
    )?;
    // EOF releases command waits, while dropping the service releases protocol
    // waits. Give the peer longer than its sample deadline to flush samples
    // already collected before resorting to the child guard's kill.
    peer.close_commands();
    drop(service);
    let cleanup = (|| {
        drain_peer(&mut peer, evidence)?;
        let after = cpu::snapshot(&[std::process::id()])?;
        evidence.record(json!({"kind":"threads", "before":baseline.keys().collect::<Vec<_>>(),
            "during":during.as_ref().map(|t|t.keys().collect::<Vec<_>>()), "after":after.keys().collect::<Vec<_>>()}))?;
        if !baseline.keys().eq(after.keys()) {
            return Err("task set not restored after worker joined".into());
        }
        if directory.exists() {
            return Err("endpoint directory survived service drop".into());
        }
        Ok(())
    })();
    evidence.record(json!({"kind":"cleanup_result", "error":cleanup.as_ref().err()}))?;
    evidence
        .record(json!({"kind":"peer_stderr", "text":String::from_utf8_lossy(&peer.diagnostics)}))?;
    drop(peer);
    evidence.flush()?;
    // Keep the triggering workload error when cleanup also failed.
    result.and(cleanup)
}

fn drain_peer(peer: &mut process::Peer, evidence: &mut Evidence) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        // Even when parsing failed earlier, preserve every remaining raw
        // line. Recovery records are evidence, never successful samples.
        while let Some(line) = peer.lines.pop_front() {
            evidence.record(json!({"kind":"peer_recovery_record", "line":line}))?;
        }
        peer.drain()?;
        if peer.stdout_closed && peer.lines.is_empty() && peer.exited()?.is_some() {
            break;
        }
        if Instant::now() >= deadline {
            return Err("peer evidence drain timeout; remaining samples unavailable".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

#[test]
fn failure_drain_keeps_queued_lines_and_samples_flushed_on_eof() {
    let root = std::env::temp_dir().join(format!("sophia-perf-drain-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let script = root.join("peer.sh");
    std::fs::write(
        &script,
        "cat >/dev/null\nprintf 'S proposals 0 10 20 1 1\\nF proposals 1 bad-command\\n'\nexit 1\n",
    )
    .unwrap();
    let mut peer =
        process::Peer::spawn(Path::new("/bin/sh"), &script, "proposals", "small").unwrap();
    peer.lines
        .push_back("already queued after parse error".into());
    let path = root.join("evidence.jsonl");
    let mut evidence = Evidence(File::create(&path).unwrap(), Vec::new(), 0);
    peer.close_commands();
    drain_peer(&mut peer, &mut evidence).unwrap();
    evidence.flush().unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    let records: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(records.len(), 3);
    assert_eq!(records[0]["line"], "already queued after parse error");
    assert_eq!(records[1]["line"], "S proposals 0 10 20 1 1");
    assert_eq!(records[2]["line"], "F proposals 1 bad-command");
    assert!(!peer.exited().unwrap().unwrap().success());
    drop(peer);
    drop(evidence);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn evidence_bound_preserves_terminal_error_and_flushes() {
    let path = std::env::temp_dir().join(format!("sophia-perf-overflow-{}", std::process::id()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    let mut evidence = Evidence(file, vec![json!({"kind":"sample"}); 16_384], 0);
    assert!(evidence.record(json!({"kind":"sample"})).is_err());
    evidence
        .record(json!({"kind":"workload_result","passed":false,"error":"entry bound"}))
        .unwrap();
    evidence
        .record(json!({"kind":"cleanup_result","error":null}))
        .unwrap();
    evidence.flush().unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("entry bound"));
    assert!(text.contains("evidence_overflow"));
    drop(evidence);
    std::fs::remove_file(path).unwrap();
}
