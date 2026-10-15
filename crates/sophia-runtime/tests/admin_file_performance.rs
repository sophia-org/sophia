//! Opt-in diagnostic; no timing workload runs in the ordinary test gate.
#[path = "support/output_file_performance/cpu.rs"]
mod cpu;
use serde_json::json;
use sophia_protocol::{ControlCatalog, ControlCommand, ControlOutcome, ControlOwner};
use sophia_runtime::{AdminClient, ControlClient, ControlService};
use std::io::Write;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

enum Client {
    Legacy(ControlClient),
    Files(Box<AdminClient>),
}
impl Client {
    fn connect(path: &Path, files: bool) -> Self {
        if files {
            Self::Files(Box::new(AdminClient::connect(path).unwrap()))
        } else {
            Self::Legacy(ControlClient::connect(path).unwrap())
        }
    }
    fn catalog(&mut self) -> ControlCatalog {
        match self {
            Self::Legacy(c) => c.commands().unwrap(),
            Self::Files(c) => c.commands().unwrap(),
        }
    }
    fn invoke(&mut self, command: ControlCommand) {
        let result = match self {
            Self::Legacy(c) => c.invoke(command),
            Self::Files(c) => c.invoke(command),
        }
        .unwrap();
        assert_eq!(result, (7, ControlOutcome::Committed, String::new()));
    }
}

struct Owner {
    stop: Arc<AtomicBool>,
    wake: sophia_wake::Notifier,
    thread: Option<std::thread::JoinHandle<usize>>,
}
impl Owner {
    fn join(&mut self) -> usize {
        self.stop.store(true, Ordering::Release);
        self.wake.notify();
        self.thread.take().unwrap().join().unwrap()
    }
}
impl Drop for Owner {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.join();
        }
    }
}
fn owner(service: ControlService, command: ControlCommand, delay: Arc<AtomicU64>) -> Owner {
    let stop = Arc::new(AtomicBool::new(false));
    let done = stop.clone();
    let wake = sophia_wake::Wake::new().unwrap();
    let notifier = wake.notifier();
    service.set_owner_wake(notifier.clone());
    let thread = std::thread::spawn(move || {
        let mut pending = None;
        let mut count = 0;
        let mut identities = std::collections::BTreeSet::new();
        while !done.load(Ordering::Acquire) {
            wake.clear().unwrap();
            if pending.is_none() {
                pending = service.try_request();
            }
            if let Some(ticket) = &pending {
                assert_eq!(ticket.command, command);
                assert_eq!(ticket.generation, 7);
                assert!(!ticket.cancelled());
                if ticket.claim() {
                    assert!(identities.insert((ticket.connection, ticket.request)));
                    let ms = delay.load(Ordering::Acquire);
                    if ms != 0 {
                        std::thread::sleep(Duration::from_millis(ms));
                    }
                    ticket.finish(ControlOutcome::Committed);
                    count += 1;
                    pending = None;
                    continue;
                }
            }
            wake.wait(Some(Instant::now() + Duration::from_secs(1)))
                .unwrap();
        }
        assert!(pending.is_none());
        assert!(service.try_request().is_none());
        count
    });
    Owner {
        stop,
        wake: notifier,
        thread: Some(thread),
    }
}
fn summary(samples: &[u64]) -> serde_json::Value {
    assert!(!samples.is_empty());
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = |p: usize| sorted[(sorted.len() * p).div_ceil(100) - 1];
    json!({"samples_ns":samples,"p50_ns":rank(50),"p95_ns":rank(95),"p99_ns":rank(99),"max_ns":sorted.last().unwrap()})
}

#[test]
fn nearest_rank_keeps_outliers() {
    let value = summary(&[9, 1, 2, 3, 100]);
    assert_eq!(value["p50_ns"], 3);
    assert_eq!(value["p99_ns"], 100);
    assert_eq!(value["samples_ns"], json!([9, 1, 2, 3, 100]));
}

fn catalog(maximum: bool) -> ControlCatalog {
    let mut commands = (0..if maximum { 256 } else { 1 })
        .map(|i| ControlCommand {
            owner: ControlOwner::Policy,
            name: if maximum {
                format!("{i:0128}")
            } else {
                "focus next".into()
            },
        })
        .collect::<Vec<_>>();
    if maximum {
        commands.extend(
            sophia_protocol::CONTROL_SESSION_OPERATIONS
                .iter()
                .map(|name| ControlCommand {
                    owner: ControlOwner::Session,
                    name: (*name).into(),
                }),
        );
    }
    ControlCatalog {
        generation: 7,
        commands,
    }
}

#[test]
fn timing_catalogs_are_valid_before_any_measurement() {
    for maximum in [false, true] {
        let catalog = catalog(maximum);
        sophia_protocol::validate_control_catalog(&catalog).unwrap();
        assert_eq!(catalog.commands.len(), if maximum { 259 } else { 1 });
        sophia_protocol::admin_files::encode_admin_catalog(&catalog).unwrap();
    }
}

#[test]
#[ignore = "explicit release administrative transport diagnostic"]
fn admin_file_performance() {
    if cfg!(debug_assertions) {
        panic!("requires release profile");
    }
    let path = std::env::var_os("SOPHIA_ADMIN_PERF_EVIDENCE").expect("evidence path");
    let mut evidence = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    let mut record = |value: serde_json::Value| {
        serde_json::to_writer(&mut evidence, &value).unwrap();
        writeln!(evidence).unwrap();
        evidence.flush().unwrap();
    };
    record(
        json!({"kind":"header","schema":1,"qualification":false,"rounds":2,"discovery_samples":100,
        "warmups_per_delay":20,"invocation_samples_per_delay":200,"owner_delay_ms":[0,2],"idle_seconds":2,
        "order":"legacy,files; files,legacy per catalog", "percentiles":"nearest rank", "catalogs":[1,259],
        "limits":"diagnostic, no approved administrative numeric budget; synthetic owner, no desktop"}),
    );
    for maximum in [false, true] {
        let catalog = catalog(maximum);
        sophia_protocol::validate_control_catalog(&catalog).unwrap();
        for round in 0..2 {
            for files in if round == 0 {
                [false, true]
            } else {
                [true, false]
            } {
                let root = std::env::temp_dir().join(format!("admin-perf-{}", std::process::id()));
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new()
                    .mode(0o700)
                    .create(&root)
                    .unwrap();
                let before_threads = cpu::snapshot(&[std::process::id()]).unwrap().len();
                let service = if files {
                    ControlService::bind_files(&root)
                } else {
                    ControlService::bind(&root)
                }
                .unwrap();
                let socket = service.socket_path().to_owned();
                let catalog = Arc::new(catalog.clone());
                let publish_deadline = Instant::now() + Duration::from_secs(2);
                while !service.publish(catalog.clone(), &[]) {
                    assert!(Instant::now() < publish_deadline);
                    std::thread::yield_now();
                }
                let delay = Arc::new(AtomicU64::new(0));
                let mut owner = owner(service, catalog.commands[0].clone(), delay.clone());
                let mut discovery = Vec::new();
                for sample in 0..100 {
                    let start = Instant::now();
                    let mut client = Client::connect(&socket, files);
                    assert_eq!(client.catalog(), *catalog);
                    discovery.push(start.elapsed().as_nanos() as u64);
                    record(
                        json!({"kind":"sample","operation":"discovery","files":files,"round":round,
                        "commands":catalog.commands.len(),"sample":sample,"ns":discovery.last().unwrap()}),
                    );
                    drop(client);
                    // Outside timing: let the worker observe disconnect before reuse.
                    std::thread::sleep(Duration::from_millis(2));
                }
                let mut client = Client::connect(&socket, files);
                assert_eq!(client.catalog(), *catalog);
                let mut commands = Vec::new();
                for ms in [0, 2] {
                    delay.store(ms, Ordering::Release);
                    for _ in 0..20 {
                        client.invoke(catalog.commands[0].clone());
                    }
                    let mut samples = Vec::new();
                    for sample in 0..200 {
                        let start = Instant::now();
                        client.invoke(catalog.commands[0].clone());
                        samples.push(start.elapsed().as_nanos() as u64);
                        record(
                            json!({"kind":"sample","operation":"invoke","files":files,"round":round,
                            "commands":catalog.commands.len(),"owner_delay_ms":ms,"sample":sample,"ns":samples.last().unwrap()}),
                        );
                    }
                    commands.push(json!({"owner_delay_ms":ms,"timing":summary(&samples)}));
                }
                let before = cpu::snapshot(&[std::process::id()]).unwrap();
                let start = Instant::now();
                std::thread::sleep(Duration::from_secs(2));
                let elapsed = start.elapsed();
                let after = cpu::snapshot(&[std::process::id()]).unwrap();
                let mut idle = cpu::interval(&before, &after, elapsed).unwrap();
                // The shared accounting helper's output-role threshold is not an admin budget.
                idle.as_object_mut().unwrap().remove("passed");
                drop(client);
                assert_eq!(owner.join(), 440);
                assert_eq!(
                    cpu::snapshot(&[std::process::id()]).unwrap().len(),
                    before_threads
                );
                std::fs::remove_dir_all(root).unwrap();
                record(
                    json!({"kind":"block","files":files,"round":round,"commands":catalog.commands.len(),
                    "discovery":summary(&discovery),"invocations":commands,"idle":idle,"owner_deliveries":440,
                    "threads_before_and_after":before_threads}),
                );
            }
        }
    }
    record(json!({"kind":"result","completed":true,"qualification":false}));
}
