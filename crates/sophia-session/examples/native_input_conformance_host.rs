//! Contained entry for the production Session private input owner.
//!
//! The runner supplies topology, credentials and one control pipe. Entry does
//! not discover a display, seat, device, bus or authorization service.
use sophia_conformance::private_instance::validate_entry;
use sophia_input_authority::{InstanceId, SeatBinding};
use sophia_protocol::{
    NamespaceId, OutputId, OutputTopologyEntry, OutputTopologySnapshot, Rect, SeatId, Size,
};
use sophia_session::private_input::{
    PrivateInputConfig, PrivateInputGrantPolicy, PrivateInputInstanceCookie, PrivateInputReadiness,
    PrivateInputService, PrivateInputThreadJoin,
};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct Options {
    activation: i32,
    control: i32,
    socket: PathBuf,
    cookie: PathBuf,
    instance: u64,
    namespace: u64,
    session_generation: u64,
    width: i32,
    height: i32,
    grants: PrivateInputGrantPolicy,
    lifetime: Duration,
}

impl Options {
    fn read() -> Result<Self, String> {
        let arguments = std::env::args().skip(1).collect::<Vec<_>>();
        let mut values = BTreeMap::new();
        for pair in arguments.chunks(2) {
            let [name, value] = pair else {
                return Err("every host option requires a value".into());
            };
            if values.insert(name.as_str(), value.as_str()).is_some() {
                return Err(format!("duplicate option: {name}"));
            }
        }
        let expected = [
            "--activation-fd",
            "--control-fd",
            "--socket",
            "--cookie-file",
            "--instance",
            "--namespace",
            "--session-generation",
            "--width",
            "--height",
            "--grants",
            "--lifetime-ms",
        ];
        if values.len() != expected.len() || expected.iter().any(|key| !values.contains_key(key)) {
            return Err(
                "host requires explicit private options; ambient options are unsupported".into(),
            );
        }
        let number = |key: &str| -> Result<u64, String> {
            values[key]
                .parse::<u64>()
                .map_err(|_| format!("invalid {key}"))
        };
        let positive = |key: &str| -> Result<i32, String> {
            i32::try_from(number(key)?)
                .ok()
                .filter(|value| *value > 0)
                .ok_or_else(|| format!("invalid {key}"))
        };
        let lifetime = number("--lifetime-ms")?;
        if !(1..=90_000).contains(&lifetime) {
            return Err("host lifetime must be in 1..=90000 milliseconds".into());
        }
        let instance = number("--instance")?;
        let namespace = number("--namespace")?;
        let session_generation = number("--session-generation")?;
        if instance == 0 || namespace == 0 || session_generation == 0 {
            return Err("instance, namespace and session generation must be nonzero".into());
        }
        let options = Self {
            activation: positive("--activation-fd")?,
            control: positive("--control-fd")?,
            socket: values["--socket"].into(),
            cookie: values["--cookie-file"].into(),
            instance,
            namespace,
            session_generation,
            width: positive("--width")?,
            height: positive("--height")?,
            grants: match values["--grants"] {
                "disabled" => PrivateInputGrantPolicy::Disabled,
                "verified" => PrivateInputGrantPolicy::EnabledWithVerifiedEvidence,
                _ => return Err("grants must be disabled or verified".into()),
            },
            lifetime: Duration::from_millis(lifetime),
        };
        for path in [&options.socket, &options.cookie] {
            if !path.starts_with("/work/")
                || path
                    .components()
                    .any(|part| part == std::path::Component::ParentDir)
            {
                return Err("host paths must stay inside the explicit /work artifacts".into());
            }
        }
        Ok(options)
    }

    fn config(&self) -> Result<PrivateInputConfig, String> {
        let cookie: [u8; 32] = std::fs::read(&self.cookie)
            .map_err(|e| e.to_string())?
            .try_into()
            .map_err(|_| "cookie file must contain exactly 32 bytes")?;
        let output = OutputId::from_raw(1);
        let size = Size {
            width: self.width,
            height: self.height,
        };
        let topology = OutputTopologySnapshot {
            generation: 1,
            primary: output,
            outputs: vec![OutputTopologyEntry {
                output,
                logical: Rect {
                    x: 0,
                    y: 0,
                    width: self.width,
                    height: self.height,
                },
                pixel_size: size,
                scale: 1,
                refresh_millihz: 60_000,
                timing: None,
            }],
        };
        topology
            .validate()
            .map_err(|error| format!("invalid topology: {error:?}"))?;
        let instance = InstanceId::new(self.instance);
        Ok(PrivateInputConfig {
            socket_path: self.socket.clone(),
            namespace: NamespaceId::from_raw(self.namespace),
            session_generation: self.session_generation,
            profile: sophia_protocol::NamespaceProfile::Confined,
            capabilities: sophia_protocol::NamespaceCapabilities::NONE,
            binding: SeatBinding::new(instance, SeatId::from_raw(1)),
            cookie: PrivateInputInstanceCookie { instance, cookie },
            grants: self.grants,
            max_concurrent_clients: NonZeroUsize::new(4).unwrap(),
            input_capacity: NonZeroUsize::new(8).unwrap(),
            advertised_buttons: 9,
            output_topology: topology,
        })
    }
}

fn run() -> Result<(), String> {
    let options = Options::read()?;
    let mut activation = validate_entry(options.activation)?;
    if activation.delegated().len() != 1 || !activation.delegated().contains(&options.control) {
        return Err("host requires exactly its delegated control pipe".into());
    }
    let mut control = activation.take_pipe(options.control)?;
    let flags = rustix::fs::fcntl_getfl(&control).map_err(|e| e.to_string())?;
    rustix::fs::fcntl_setfl(&control, flags | rustix::fs::OFlags::NONBLOCK)
        .map_err(|e| e.to_string())?;
    let service =
        PrivateInputService::start(options.config()?).map_err(|e| format!("start: {e:?}"))?;
    let ready = service
        .await_ready(Duration::from_secs(3))
        .map_err(|e| e.to_string())?;
    if ready != PrivateInputReadiness::Ready {
        return Err(format!("private service did not become ready: {ready:?}"));
    }
    if service.socket_path() != Path::new(&options.socket) {
        return Err("bound socket differs from the requested private socket".into());
    }
    println!("sophia_m4_host ready");
    std::io::stdout().flush().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + options.lifetime;
    let mut command = Vec::new();
    let mut commits = 0;
    let result = loop {
        let mut bytes = [0; 16];
        match control.read(&mut bytes) {
            Ok(0) => break Err("control owner ended without a stop command".into()),
            Ok(read) => {
                command.extend_from_slice(&bytes[..read]);
                if command == b"stop\n" {
                    break Ok(());
                }
                if !b"stop\n".starts_with(&command) {
                    break Err("unknown control command".into());
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => break Err(error.to_string()),
        }
        match service.apply_committed(Duration::from_millis(1)) {
            Ok(report) => {
                commits += report.committed;
                if !report.refused.is_empty() {
                    break Err(format!("committed effects refused: {:?}", report.refused));
                }
            }
            Err(error) => break Err(error.to_string()),
        }
        if Instant::now() >= deadline {
            break Err("private host deadline elapsed".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    };
    let outcome = service.stop();
    let service_joined = outcome.service_thread == PrivateInputThreadJoin::Joined;
    let workers = outcome
        .workers
        .as_ref()
        .ok_or("invocation returned no collection report")?;
    let workers_joined = workers.iter().filter(|worker| worker.joined).count();
    println!(
        "sophia_m4_host stopped service_joined={service_joined} workers={} workers_joined={workers_joined} committed={commits} interrupted={} settlement_readable={}",
        workers.len(),
        outcome.interrupted,
        outcome.settlement.readable,
    );
    if !service_joined || workers_joined != workers.len() || outcome.failure.is_some() {
        return Err(format!("private service collection failed: {outcome:?}"));
    }
    result
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("native-input-conformance-host: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
