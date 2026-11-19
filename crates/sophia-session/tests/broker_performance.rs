//! Opt-in cost diagnostic. Never a performance qualification by itself.
#[path = "../../sophia-runtime/tests/support/output_file_performance/cpu.rs"]
mod cpu;

use sophia_protocol::{
    DisplayLabel, MetadataDisclosure, NamespaceProfile, ReducedMetadataCandidate, SurfaceId,
    broker_files::{BrokerFileLimits, BrokerFileRequest},
};
use sophia_runtime::{
    ProcessLaunchSpec, ProcessSupervisor, ProtectionDomainRole, ProtectionDomainSpec,
    ProtectionPath, SupervisedProcessKind, SupervisorCommand,
    broker_files::{BrokerFileListener, SOPHIA_BROKER_9P_SOCKET_ENV},
};
use sophia_session::{
    broker_file_driver::BrokerFileDriver,
    broker_file_owner::{BrokerCompletion, BrokerFileOwner},
};
use std::{
    io::Write,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Peer {
    driver: BrokerFileDriver<u64>,
    supervisor: ProcessSupervisor,
    pid: u32,
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.supervisor.terminate();
        let _ = self.driver.close();
    }
}
impl Peer {
    fn start(executable: &Path, bwrap: &Path) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "sophia-broker-cost-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let uid = rustix::process::geteuid().as_raw();
        let listener = BrokerFileListener::bind(&directory, uid).unwrap();
        let socket = listener.socket_path();
        let domain = ProtectionDomainSpec::bubblewrap([ProtectionDomainRole::MetadataBroker])
            .unwrap()
            .bubblewrap_path(bwrap)
            .path(ProtectionPath::read_only(socket.parent().unwrap()))
            .unwrap();
        let spec = ProcessLaunchSpec::new(executable)
            .arg("metadata-broker-files-serve")
            .env(SOPHIA_BROKER_9P_SOCKET_ENV, socket)
            .env("SOPHIA_BROKER_DEFAULT_DISCLOSURE", "class-only")
            .process_group()
            .protection_domain(domain);
        let mut supervisor = ProcessSupervisor::new(SupervisedProcessKind::MetadataBroker, spec);
        supervisor
            .apply(SupervisorCommand::StartProcess {
                process: SupervisedProcessKind::MetadataBroker,
                delay: Duration::ZERO,
            })
            .unwrap();
        let evidence = supervisor.protection_evidence().unwrap().clone();
        let service = listener
            .accept(
                BrokerFileOwner::new(1).unwrap(),
                &evidence,
                1,
                1,
                0,
                BrokerFileLimits::default(),
                Duration::from_secs(5),
            )
            .unwrap();
        let mut driver = BrokerFileDriver::new(service);
        driver.negotiate(Duration::from_secs(5)).unwrap();
        Self {
            driver,
            supervisor,
            pid: evidence.peer_pid,
        }
    }
    fn admit(&mut self, surface: SurfaceId) {
        assert!(matches!(
            self.driver
                .request(
                    BrokerFileRequest::SurfaceAdmitted {
                        surface,
                        profile: NamespaceProfile::Confined
                    },
                    1,
                    Duration::from_secs(5)
                )
                .unwrap(),
            BrokerCompletion::Applied { rule: Some(_), .. }
        ));
    }
    fn candidate(&mut self, candidate: &ReducedMetadataCandidate) {
        assert!(matches!(
            self.driver
                .request(
                    BrokerFileRequest::CandidateReduced {
                        surface: candidate.surface,
                        disclosure: candidate.disclosure,
                        label: candidate.label.as_ref().map(|v| v.text.as_str()),
                        redacted: false,
                        generation: candidate.generation,
                    },
                    1,
                    Duration::from_secs(5)
                )
                .unwrap(),
            BrokerCompletion::Applied { .. }
        ));
    }
    fn check(&self, candidate: &ReducedMetadataCandidate) {
        let owner = self.driver.owner();
        let descriptor = owner.descriptors().get(candidate.surface).unwrap();
        assert_eq!(descriptor.generation, candidate.generation);
        assert_eq!(descriptor.label, candidate.label);
        assert_eq!(
            owner.grants()[&candidate.surface].target_generation,
            candidate.generation
        );
    }
    fn settle(&mut self) {
        // Wait for the final receipt acknowledgment before ending CPU accounting.
        self.driver.negotiate(Duration::from_secs(5)).unwrap();
    }
    fn finish(&mut self) {
        // Let blocked socket calls return before supervisor cleanup. Killing a
        // peer inside recv makes a byte trace incomplete, even after owner work
        // has settled. Teardown remains outside the measured workload interval.
        self.driver.close().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.supervisor.poll().unwrap().is_none() {
            assert!(
                Instant::now() < deadline,
                "broker did not exit after transport closure"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
#[ignore = "requires explicit executable, bubblewrap and new evidence path; opt-in cost diagnostic"]
fn broker_descriptor_cost() {
    let executable = std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_EXECUTABLE").unwrap());
    let bwrap = std::path::PathBuf::from(std::env::var_os("SOPHIA_TEST_BUBBLEWRAP").unwrap());
    let evidence =
        std::path::PathBuf::from(std::env::var_os("SOPHIA_BROKER_COST_EVIDENCE").unwrap());
    assert!(executable.is_absolute() && bwrap.is_absolute() && evidence.is_absolute());
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(evidence)
        .unwrap();
    let samples: usize = std::env::var("SOPHIA_BROKER_COST_SAMPLES")
        .unwrap_or_else(|_| "1000".into())
        .parse()
        .unwrap();
    assert!((1..=100_000).contains(&samples));
    // Two blocks, with independent process lifetime per block. Admission and
    // warmup are outside the interval; candidates and sample storage are prebuilt.
    // The legacy-envelope baseline this once paired against is recorded in
    // docs/notes/plans/jsschoen (t314 result).
    for block in 0..2 {
        for label_bytes in [0usize, 16, 120] {
            for latency in [true, false] {
                let mut peer = Peer::start(&executable, &bwrap);
                let surface = SurfaceId::new(1, 1);
                peer.admit(surface);
                let candidate = |generation| ReducedMetadataCandidate {
                    surface,
                    disclosure: MetadataDisclosure::ClassOnly,
                    label: (label_bytes != 0).then(|| DisplayLabel {
                        text: "x".repeat(label_bytes),
                        redacted: false,
                    }),
                    generation,
                };
                peer.candidate(&candidate(1));
                peer.settle();
                let candidates: Vec<_> = (2..samples as u64 + 2).map(candidate).collect();
                let mut timings = vec![0u64; samples];
                // Characterize counter-reading overhead without issuing work.
                // Keep raw observations; subtraction would disguise noise as a
                // precise correction. These probes never run in the latency pass.
                let observer = (!latency).then(|| {
                    (0..5)
                        .map(|_| {
                            let pids = [std::process::id(), peer.pid];
                            let start = Instant::now();
                            let before = cpu::snapshot(&pids).unwrap();
                            let after = cpu::snapshot(&pids).unwrap();
                            let elapsed = start.elapsed();
                            let cost = cpu::interval(&before, &after, elapsed).unwrap();
                            serde_json::json!({"wall_ns":elapsed.as_nanos().to_string(),
                            "cpu_ns":cost["combined_cpu_ns"],"tasks":cost["tasks"]})
                        })
                        .collect::<Vec<_>>()
                });
                let before =
                    (!latency).then(|| cpu::snapshot(&[std::process::id(), peer.pid]).unwrap());
                let started = Instant::now();
                for (candidate, timing) in candidates.iter().zip(&mut timings) {
                    if latency {
                        let start = Instant::now();
                        peer.candidate(candidate);
                        *timing = start.elapsed().as_nanos().try_into().unwrap();
                    } else {
                        peer.candidate(candidate);
                    }
                }
                peer.settle();
                let elapsed = started.elapsed();
                let cost = before.map(|before| {
                    cpu::interval(
                        &before,
                        &cpu::snapshot(&[std::process::id(), peer.pid]).unwrap(),
                        elapsed,
                    )
                    .unwrap()
                });
                peer.check(candidates.last().unwrap());
                peer.finish();
                writeln!(output, "{}", serde_json::json!({"kind":"sample","block":block,"files":true,"label_bytes":label_bytes,"operations":samples,
                    "peer_pid":peer.pid,"owner_pid":std::process::id(),"owner_tid":rustix::thread::gettid().as_raw_pid(),
                    "pass":if latency {"latency"} else {"cpu"},"elapsed_ns":elapsed.as_nanos().to_string(),
                    "latency_ns":if latency {Some(timings)} else {None},
                    "cpu_ns":cost.as_ref().map(|c| &c["combined_cpu_ns"]),"cpu_tasks":cost.as_ref().map(|c| &c["tasks"]),
                    "empty_work_observations":observer,
                    "copied_bytes":null,"qualification":false})).unwrap();
            }
        }
    }
    writeln!(output, "{}", serde_json::json!({"kind":"result","completed":true,"qualification":false,"copy_accounting":"pending"})).unwrap();
}
