//! Real child, supplied admission and completion. No native display claim.
use sophia_protocol::*;
use sophia_runtime::*;

fn request_profile(wire: &str, probe: bool, stage: &str) {
    #[cfg(feature = "comparator-request-attribution")]
    {
        use sophia_9p::request_attribution::{NAMES, snapshot};
        for (site, value) in NAMES.into_iter().zip(snapshot()) {
            println!(
                "supplied_owner_requests schema=1 wire={wire} probe={probe} stage={stage} site={site} calls={} thread_cpu_ns={} process_allocations={} process_reallocations={} process_bytes={} nested=true",
                value.calls,
                value.thread_cpu_ns,
                value.process_allocations,
                value.process_reallocations,
                value.process_bytes
            );
        }
    }
    #[cfg(not(feature = "comparator-request-attribution"))]
    let _ = (wire, probe, stage);
}
#[path = "support/shell_timing_image_authority.rs"]
mod image_authority;
#[path = "support/shell_timing_image_relay.rs"]
mod image_relay;
#[cfg(feature = "comparator-journal-trace")]
#[path = "support/shell_journal_trace.rs"]
mod journal_trace;
#[cfg(feature = "comparator-wire-trace")]
#[path = "support/shell_timing_trace.rs"]
mod ordered_trace;
use std::{
    process::Command,
    time::{Duration, Instant},
};

#[cfg(feature = "comparator-allocation-counting")]
#[global_allocator]
static ALLOCATOR: sophia_runtime::comparator_counters::Allocator =
    sophia_runtime::comparator_counters::ALLOCATOR;

#[test]
#[ignore = "requires explicit prebuilt SOPHIA_TIMING_PEER_BINARY; device-hidden"]
fn refuses_unarmed_ambiguous_and_authentication_bypass_launches() {
    let binary = std::env::var_os("SOPHIA_TIMING_PEER_BINARY").expect("explicit peer binary");
    assert!(std::path::Path::new(&binary).is_absolute());
    let root = std::env::temp_dir().join(format!("timing-peer-refusals-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let config = root.join("peer.json");
    for (wire, serve, auth, expected) in [
        ("files", false, false, "expected --serve"),
        ("files", true, false, "ambiguous shell wire"),
        (
            "ipc",
            true,
            true,
            "cannot bypass an authenticated file launch",
        ),
        ("invalid", true, false, "invalid wire"),
    ] {
        std::fs::write(
            &config,
            serde_json::json!({"wire":wire,"output":1,"frames":3,"height":24,"hold_msec":0})
                .to_string(),
        )
        .unwrap();
        let mut command = Command::new(&binary);
        command
            .env_remove("SOPHIA_LAUNCH_AUTH_FD")
            .env("SOPHIA_SHELL_CONFIG", &config)
            .env("SOPHIA_SHELL_SOCKET", root.join("absent-ipc"))
            .env("SOPHIA_SHELL_9P_SOCKET", root.join("absent-files"));
        if serve {
            command.arg("--serve");
        }
        if auth {
            command
                .env("SOPHIA_LAUNCH_AUTH_FD", "3")
                .env_remove("SOPHIA_SHELL_9P_SOCKET");
        }
        let result = command.output().unwrap();
        assert!(!result.status.success());
        let record: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(record["event"], "failed");
        assert!(
            record["error"].as_str().unwrap().contains(expected),
            "{record}"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires explicit prebuilt SOPHIA_TIMING_PEER_BINARY; device-hidden"]
fn refuses_sealed_source_without_its_file_launch_or_with_bytes() {
    let binary = std::env::var_os("SOPHIA_TIMING_PEER_BINARY").expect("explicit peer binary");
    assert!(std::path::Path::new(&binary).is_absolute());
    let root = std::env::temp_dir().join(format!(
        "timing-peer-sealed-refusals-{}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let config = root.join("peer.json");
    let image = serde_json::json!({"max_resource_bytes":4194304,"max_width":8192,
        "max_height":4096,"max_admitted_outputs":16,"transfer_slots":4,
        "transfer_timeout_ms":2000});
    for (wire, source, expected) in [
        ("ipc", "sealed", "sealed source requires the file wire"),
        (
            "files",
            "sealed",
            "sealed source requires an authenticated file launch",
        ),
        ("files", "bytes", "image bounds require sealed source"),
    ] {
        std::fs::write(
            &config,
            serde_json::json!({"wire":wire,"output":1,"frames":3,"height":24,"hold_msec":0,
            "source":source,"image":image})
            .to_string(),
        )
        .unwrap();
        // Absent endpoints: each refusal must precede any connection.
        let result = Command::new(&binary)
            .arg("--serve")
            .env_remove("SOPHIA_LAUNCH_AUTH_FD")
            .env_remove(if wire == "files" {
                "SOPHIA_SHELL_SOCKET"
            } else {
                "SOPHIA_SHELL_9P_SOCKET"
            })
            .env("SOPHIA_SHELL_CONFIG", &config)
            .env(
                if wire == "files" {
                    "SOPHIA_SHELL_9P_SOCKET"
                } else {
                    "SOPHIA_SHELL_SOCKET"
                },
                root.join("absent"),
            )
            .output()
            .unwrap();
        assert!(!result.status.success());
        let record: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(record["event"], "failed");
        assert!(
            record["error"].as_str().unwrap().contains(expected),
            "{record}"
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires explicit prebuilt SOPHIA_TIMING_PEER_BINARY; device-hidden"]
fn supplied_owner_records_every_offer_and_terminal_on_ipc_baseline() {
    const FRAMES: u64 = 64;
    const PANEL_WIDTH: u32 = 1920;
    #[cfg(feature = "comparator-counters")]
    let sampler = sophia_runtime::comparator_counters::Sampler::start().unwrap();
    let binary = std::env::var_os("SOPHIA_TIMING_PEER_BINARY").expect("explicit peer binary");
    assert!(std::path::Path::new(&binary).is_absolute());
    // Files use the authenticated ISB controls below. Byte-files captures stay
    // reproducible on the frozen historical branch, not this production merge.
    for (files, probe) in [(false, false), (false, true)] {
        request_profile("ipc", probe, "begin");
        #[cfg(feature = "comparator-wire-trace")]
        sophia_9p::wire_trace::begin();
        #[cfg(feature = "comparator-progress")]
        let progress_before = sophia_runtime::comparator_counters::progress::snapshot();
        let root = std::env::temp_dir().join(format!(
            "timing-peer-{}-{files}-{probe}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let config = root.join("peer.json");
        std::fs::write(
            &config,
            serde_json::json!({"wire":if files {"files"} else {"ipc"},
            "output":1,"frames":FRAMES,"height":24,"hold_msec":100,"probe_bytes":if probe {4194304} else {0}})
            .to_string(),
        )
        .unwrap();
        let mut host = ShellSessionTransport::bind_for_supervised_uid(
            root.join("export"),
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        let mut command = Command::new(&binary);
        command
            .arg("--serve")
            .env_remove("SOPHIA_LAUNCH_AUTH_FD")
            .env_remove("SOPHIA_SHELL_SOCKET")
            .env_remove("SOPHIA_SHELL_9P_SOCKET")
            .env("SOPHIA_SHELL_CONFIG", &config)
            .env(
                if files {
                    "SOPHIA_SHELL_9P_SOCKET"
                } else {
                    "SOPHIA_SHELL_SOCKET"
                },
                host.socket_path(),
            )
            .stdout(std::fs::File::create(root.join("stdout")).unwrap())
            .stderr(std::fs::File::create(root.join("stderr")).unwrap());
        let mut child = command.spawn().unwrap();
        host.authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: child.id(),
            peer_pid: child.id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
        let policy = ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        };
        if files {
            host.accept_files_with_content_policy(1, Duration::from_secs(5), policy)
        } else {
            host.accept_and_negotiate_with_content_policy(1, Duration::from_secs(5), policy)
        }
        .unwrap();
        let output = ContentOutputId {
            id: 1,
            generation: 1,
        };
        let allocation = ContentAllocationId {
            id: 1,
            generation: 1,
        };
        host.publish_content_output_facts(
            TransactionId::from_raw(1),
            1,
            vec![ContentOutputFactsEntry {
                output,
                local_width: PANEL_WIDTH,
                local_height: 1080,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
        let start = Instant::now();
        let mut completed = 0;
        let mut mismatched = 0;
        let mut held = None;
        while child.try_wait().unwrap().is_none() {
            if start.elapsed() > Duration::from_secs(30) {
                child.kill().unwrap();
                panic!("peer deadline");
            }
            let result = (|| -> Result<(), ShellTransportError> {
                host.service_owner_turn()?;
                let now = start.elapsed().as_millis() as u64;
                host.service_content_resources(now)?;
                host.service_content_allocation_requests(&[], now)?;
                while let Some((_, request)) = host.next_content_allocation_request() {
                    host.grant_content_allocation(
                        request.allocation_request_id,
                        ContentAllocationSnapshot {
                            native_opening: None,
                            output,
                            allocation,
                            scale_generation: 1,
                            scale_numerator: 1,
                            scale_denominator: 1,
                            role: 1,
                            edge: 1,
                            margins: ContentMargins::default(),
                            logical: ContentLogicalRect {
                                x: 0,
                                y: 0,
                                width: PANEL_WIDTH,
                                height: 24,
                            },
                            pixel: ContentPixelRect {
                                x: 0,
                                y: 0,
                                width: PANEL_WIDTH,
                                height: 24,
                            },
                            parent: ContentAllocationId::default(),
                            anchor_parent_rect: ContentPixelRect::default(),
                            allowed_reservation_extent: 24,
                        },
                        &[],
                    )?;
                }
                let allocations = host.content_allocation_snapshots();
                host.service_content_demands(&[output], &allocations)?;
                while let Some((transaction, demand)) = host.next_content_demand() {
                    host.grant_content_demand(transaction, output, demand.demand_id, now)?;
                }
                host.service_content_candidates(
                    &[ContentCandidateContext {
                        output,
                        facts_generation: 1,
                        interaction_generation: 1,
                        allocations: &allocations,
                    }],
                    now,
                )?;
                while let Some((output, generation)) = host.next_content_submission() {
                    let bundle = host.begin_content_submission(output, generation, now)?;
                    if !panel_canvas_matches(&bundle, PANEL_WIDTH) {
                        mismatched += 1;
                    }
                    held = Some(bundle);
                    let grant = host.content_grant().unwrap();
                    host.content_prepared(grant, output, generation, 1, 1, now)?;
                    host.content_presented(grant, output, generation, generation, 1, 1)?;
                    completed += 1;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {}
                Err(ShellTransportError::NotConnected) if completed == FRAMES => {}
                Err(error) => {
                    let _ = child.kill();
                    panic!(
                        "owner: {error:?}; stdout={}; stderr={}",
                        std::fs::read_to_string(root.join("stdout")).unwrap(),
                        std::fs::read_to_string(root.join("stderr")).unwrap()
                    );
                }
            }
            std::thread::yield_now();
        }
        let result = child.wait_with_output().unwrap();
        let stdout = std::fs::read(root.join("stdout")).unwrap();
        let stderr = std::fs::read(root.join("stderr")).unwrap();
        assert!(
            result.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        if let Some(directory) = std::env::var_os("SOPHIA_TIMING_PEER_JOURNALS") {
            use std::io::Write;
            let directory = std::path::Path::new(&directory);
            assert!(directory.is_absolute());
            let stem = format!(
                "{}{}",
                if files { "files" } else { "ipc" },
                if probe { "-4mib" } else { "" }
            );
            let name = format!("{stem}.ndjson");
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap()
                .write_all(&stdout)
                .unwrap();
            let name = format!("{stem}.counters");
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap()
                .write_all(&stderr)
                .unwrap();
        }
        let records: Vec<serde_json::Value> = String::from_utf8(stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(completed, FRAMES);
        assert_eq!(
            mismatched, 0,
            "presented panel bytes differ from the canvas"
        );
        #[cfg(feature = "comparator-progress")]
        {
            let progress = sophia_runtime::comparator_counters::progress::snapshot();
            let counts: serde_json::Map<String, serde_json::Value> =
                sophia_runtime::comparator_counters::progress::NAMES
                    .iter()
                    .zip(progress.iter().zip(progress_before))
                    .map(|(name, (after, before))| (name.to_string(), (after - before).into()))
                    .collect();
            if let Some(directory) = std::env::var_os("SOPHIA_TIMING_PEER_JOURNALS") {
                let stem = format!(
                    "{}{}",
                    if files { "files" } else { "ipc" },
                    if probe { "-4mib" } else { "" }
                );
                let path =
                    std::path::Path::new(&directory).join(format!("{stem}.server-progress.json"));
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .unwrap();
                serde_json::to_writer_pretty(&mut file, &serde_json::json!({
                    "frames": FRAMES, "includes_startup": true,
                    "scope": "supplied owner fixture; no Session owner loop or physical presentation",
                    "counts": counts,
                })).unwrap();
            }
        }
        #[cfg(feature = "comparator-wire-trace")]
        ordered_trace::finish(files, probe, FRAMES);
        let offers: Vec<_> = records.iter().filter(|r| r["event"] == "offer").collect();
        let terminals: Vec<_> = records
            .iter()
            .filter(|r| r["event"] == "terminal")
            .collect();
        assert_eq!(offers.len(), FRAMES as usize);
        assert_eq!(terminals.len(), FRAMES as usize);
        let uploads: Vec<_> = records
            .iter()
            .filter(|r| r["event"] == "upload_offer")
            .collect();
        assert_eq!(uploads.len(), FRAMES as usize * if probe { 2 } else { 1 });
        assert_eq!(
            records
                .iter()
                .filter(|r| r["event"] == "probe_released")
                .count(),
            if probe { FRAMES as usize } else { 0 }
        );
        if probe {
            for pair in uploads.chunks_exact(2) {
                assert_eq!(pair[0]["upload"]["total_bytes"], 4194304);
                assert_eq!(pair[0]["upload"]["resource_id"], 1);
                assert_eq!(pair[1]["upload"]["total_bytes"], PANEL_WIDTH * 24 * 4);
            }
        }
        if cfg!(feature = "comparator-counters") {
            let counters = String::from_utf8(stderr).unwrap();
            let lines: Vec<_> = counters.lines().collect();
            let stride = if cfg!(feature = "comparator-progress") {
                2
            } else {
                1
            };
            assert_eq!(lines.len(), stride * 2 * FRAMES as usize);
            for (index, pair) in lines.chunks_exact(stride).enumerate() {
                let line = pair[0];
                if stride == 2 {
                    assert!(pair[1].starts_with(
                        "sophia_comparator_progress schema=3 mode=progress process=peer "
                    ));
                    assert!(pair[1].contains(&format!("sequence={} ", index / 2 + 1)));
                }
                assert!(line.starts_with(if cfg!(feature = "comparator-progress") {
                    "sophia_comparator_counters schema=2 mode=progress process=peer "
                } else if cfg!(feature = "comparator-profiling") {
                    "sophia_comparator_counters schema=3 mode=profile process=peer "
                } else if cfg!(feature = "comparator-allocation-counting") {
                    "sophia_comparator_counters schema=3 mode=rust-global process=peer "
                } else {
                    "sophia_comparator_counters schema=3 mode=cpu-only process=peer "
                }));
                if !cfg!(feature = "comparator-progress") {
                    assert!(line.contains(" minor_faults="));
                    assert!(line.contains(" major_faults="));
                }
                assert!(line.contains(if index % 2 == 0 {
                    "stage=frame_start "
                } else {
                    "stage=frame_end "
                }));
                assert!(line.contains(&format!("sequence={} ", index / 2 + 1)));
            }
        }
        for (offer, terminal) in offers.iter().zip(&terminals) {
            for field in [
                "connection_epoch",
                "content_grant_epoch",
                "output",
                "candidate_generation",
            ] {
                assert_eq!(offer[field], terminal[field]);
            }
            assert_eq!(terminal["outcome"], "presented");
            assert!(
                offer["submitted_monotonic_usec"].as_u64().unwrap()
                    <= terminal["observed_monotonic_usec"].as_u64().unwrap()
            );
        }
        assert_eq!(records.last().unwrap()["event"], "end");
        host.disconnect().unwrap();
        drop(held);
        assert!(host.collect_content_accounting().quiescent());
        drop(host);
        std::fs::remove_dir_all(root).unwrap();
        request_profile("ipc", probe, "end");
    }
    #[cfg(feature = "comparator-counters")]
    sampler.finish().unwrap();
}

/// Supplied authority and relay verdicts drive the child's real launch
/// client; neither is Factotum or launch-domain proof. The sealed run keeps
/// the byte workload's dimensions and bytes. No native display claim.
#[test]
#[ignore = "requires explicit prebuilt SOPHIA_TIMING_PEER_BINARY; device-hidden; supplied authority"]
fn supplied_authority_sealed_source_keeps_the_paired_workload() {
    const FRAMES: u64 = 64;
    const PANEL_WIDTH: u32 = 1920;
    let binary = std::env::var_os("SOPHIA_TIMING_PEER_BINARY").expect("explicit peer binary");
    assert!(std::path::Path::new(&binary).is_absolute());
    for probe in [false, true] {
        request_profile("isb", probe, "begin");
        #[cfg(feature = "comparator-journal-trace")]
        let trace = journal_trace::begin(JOURNAL_TRACE_EVENTS);
        let root =
            std::env::temp_dir().join(format!("timing-peer-sealed-{}-{probe}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let grant = ContentGrant {
            connection_epoch: 1,
            content_grant_epoch: 1,
        };
        // The host's own content limits are the single source of both the
        // published image budgets and the peer's configured role bounds.
        let limits = ContentLimits::prototype(grant);
        let (discovery, image_limits, formats, bounds) = sealed_image_config(&limits);
        let config = root.join("peer.json");
        std::fs::write(
            &config,
            serde_json::json!({"wire":"files","output":1,"frames":FRAMES,"height":24,
            "hold_msec":100,"probe_bytes":if probe {4194304} else {0},"source":"sealed",
            "image":{"max_resource_bytes":bounds.max_resource_bytes,"max_width":bounds.max_width,
            "max_height":bounds.max_height,"max_admitted_outputs":bounds.max_admitted_outputs,
            "transfer_slots":bounds.transfer_slots,
            "transfer_timeout_ms":limits.transfer_timeout_ms}})
            .to_string(),
        )
        .unwrap();
        let mut host = ShellComponentTransport::bind_for_supervised_uid(
            root.join("export"),
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        let mut epochs = ContentEpochRegistry::new(64 << 20).unwrap();
        let (relay_host, relay_child) = std::os::unix::net::UnixStream::pair().unwrap();
        let relay_fd = std::os::fd::AsRawFd::as_raw_fd(&relay_child);
        let mut command = Command::new(&binary);
        command
            .arg("--serve")
            .env_remove("SOPHIA_SHELL_SOCKET")
            .env("SOPHIA_SHELL_CONFIG", &config)
            .env("SOPHIA_SHELL_9P_SOCKET", host.socket_path())
            .env("SOPHIA_LAUNCH_AUTH_FD", relay_fd.to_string())
            .stdout(std::fs::File::create(root.join("stdout")).unwrap())
            .stderr(std::fs::File::create(root.join("stderr")).unwrap());
        sophia_linux_peer::inherit_one_fd_on_exec(&mut command, relay_child.into()).unwrap();
        let mut child = command.spawn().unwrap();
        // The Command held the child's end; drop it so EOF follows child exit.
        drop(command);
        let challenge = image_authority::challenge();
        let relay = std::thread::spawn(move || {
            image_relay::serve(
                relay_host,
                challenge,
                Duration::from_secs(5),
                Duration::from_secs(60),
            )
        });
        let owned = (|| -> Result<_, ShellTransportError> {
            host.authorize_authenticated_peer(
                &ProtectionDomainEvidence {
                    backend: ProtectionBackendKind::Bubblewrap,
                    supervisor_pid: child.id(),
                    peer_pid: child.id(),
                    roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
                },
                Box::new(image_authority::Factory),
            )?;
            host.configure_image_channel(discovery, image_limits, formats, bounds)?;
            host.reserve_content(&mut epochs, limits.clone())?;
            host.begin_file_negotiation(
                &epochs,
                1,
                Duration::from_secs(5),
                ShellContentAdmissionPolicy::Granted {
                    discrete_input: true,
                },
            )?;
            let deadline = Instant::now() + Duration::from_secs(5);
            while host.poll_negotiation(&mut epochs, 65536)?.is_none() {
                if Instant::now() >= deadline {
                    return Err(ShellTransportError::Io("negotiation stalled".into()));
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            Ok(())
        })();
        let completed = match owned {
            Ok(()) => run_sealed_owner(&mut host, &mut epochs, &mut child, FRAMES, 24),
            Err(error) => Err(format!("{error:?}")),
        };
        if completed.is_err() {
            // A peer that closed its wire first gets a bounded moment to
            // journal why before it is killed.
            let grace = Instant::now() + Duration::from_secs(2);
            while child.try_wait().unwrap().is_none() && Instant::now() < grace {
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = child.kill();
        }
        let status = child.wait().unwrap();
        let relay = relay.join().unwrap();
        let stdout = std::fs::read(root.join("stdout")).unwrap();
        let stderr = std::fs::read(root.join("stderr")).unwrap();
        let context = format!(
            "stdout={} stderr={}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        let (completed, mut image_owner, held) =
            completed.unwrap_or_else(|error| panic!("owner: {error}; {context}"));
        assert!(status.success(), "{context}");
        assert_eq!(relay, Ok(()), "{context}");
        assert_eq!(completed, FRAMES);
        let records: Vec<serde_json::Value> = String::from_utf8(stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records[0]["event"], "start");
        assert_eq!(records[0]["source"], "sealed");
        let uploads: Vec<_> = records
            .iter()
            .filter(|r| r["event"] == "upload_offer")
            .collect();
        let accepted = records
            .iter()
            .filter(|r| r["event"] == "upload_accepted")
            .count();
        assert_eq!(uploads.len(), FRAMES as usize * if probe { 2 } else { 1 });
        assert_eq!(accepted, uploads.len());
        for chunk in uploads.chunks_exact(if probe { 2 } else { 1 }) {
            if probe {
                assert_eq!(chunk[0]["upload"]["total_bytes"], 4194304);
                assert_eq!(chunk[0]["upload"]["resource_id"], 1);
            }
            assert_eq!(
                chunk.last().unwrap()["upload"]["total_bytes"],
                PANEL_WIDTH * 24 * 4
            );
        }
        assert_eq!(
            records
                .iter()
                .filter(|r| r["event"] == "terminal" && r["outcome"] == "presented")
                .count(),
            FRAMES as usize
        );
        assert_eq!(records.last().unwrap()["event"], "end");
        drop(held);
        host.disconnect(&mut epochs).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while image_owner.pending() != 0 {
            assert!(Instant::now() < deadline, "image owner did not drain");
            image_owner
                .service(&mut host, &mut epochs, 0, Instant::now())
                .unwrap();
            std::thread::yield_now();
        }
        assert!(host.collect_content_accounting(&mut epochs).quiescent());
        drop(host);
        #[cfg(feature = "comparator-journal-trace")]
        journal_trace::finish(trace, if probe { "isb-4mib" } else { "isb" });
        std::fs::remove_dir_all(root).unwrap();
        request_profile("isb", probe, "end");
    }
}

/// A 1920x512 panel is actually placed at full height on the byte
/// wires: the host grants the requested geometry, and every placed
/// resource carries the whole generation pattern. Three frames is
/// the smallest count that exercises both alternating resources and
/// one reuse.
#[test]
#[ignore = "requires explicit prebuilt SOPHIA_TIMING_PEER_BINARY; device-hidden"]
fn supplied_owner_places_a_512_high_panel_on_ipc_baseline() {
    const FRAMES: u64 = 3;
    const PANEL_WIDTH: u32 = 1920;
    const PANEL_HEIGHT: u32 = 512;
    let binary = std::env::var_os("SOPHIA_TIMING_PEER_BINARY").expect("explicit peer binary");
    assert!(std::path::Path::new(&binary).is_absolute());
    for files in [false] {
        let root =
            std::env::temp_dir().join(format!("timing-peer-large-{}-{files}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let config = root.join("peer.json");
        std::fs::write(
            &config,
            serde_json::json!({"wire":if files {"files"} else {"ipc"},
            "output":1,"frames":FRAMES,"height":PANEL_HEIGHT,"hold_msec":100,"probe_bytes":0})
            .to_string(),
        )
        .unwrap();
        let mut host = ShellSessionTransport::bind_for_supervised_uid(
            root.join("export"),
            rustix::process::geteuid().as_raw(),
        )
        .unwrap();
        let mut command = Command::new(&binary);
        command
            .arg("--serve")
            .env_remove("SOPHIA_LAUNCH_AUTH_FD")
            .env_remove("SOPHIA_SHELL_SOCKET")
            .env_remove("SOPHIA_SHELL_9P_SOCKET")
            .env("SOPHIA_SHELL_CONFIG", &config)
            .env(
                if files {
                    "SOPHIA_SHELL_9P_SOCKET"
                } else {
                    "SOPHIA_SHELL_SOCKET"
                },
                host.socket_path(),
            )
            .stdout(std::fs::File::create(root.join("stdout")).unwrap())
            .stderr(std::fs::File::create(root.join("stderr")).unwrap());
        let mut child = command.spawn().unwrap();
        host.authorize_protected_peer(&ProtectionDomainEvidence {
            backend: ProtectionBackendKind::Bubblewrap,
            supervisor_pid: child.id(),
            peer_pid: child.id(),
            roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
        })
        .unwrap();
        let policy = ShellContentAdmissionPolicy::Granted {
            discrete_input: true,
        };
        if files {
            host.accept_files_with_content_policy(1, Duration::from_secs(5), policy)
        } else {
            host.accept_and_negotiate_with_content_policy(1, Duration::from_secs(5), policy)
        }
        .unwrap();
        let output = ContentOutputId {
            id: 1,
            generation: 1,
        };
        let allocation = ContentAllocationId {
            id: 1,
            generation: 1,
        };
        host.publish_content_output_facts(
            TransactionId::from_raw(1),
            1,
            vec![ContentOutputFactsEntry {
                output,
                local_width: PANEL_WIDTH,
                local_height: 1080,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
        let start = Instant::now();
        let mut completed = 0;
        let mut mismatched = 0;
        let mut held = None;
        while child.try_wait().unwrap().is_none() {
            if start.elapsed() > Duration::from_secs(30) {
                child.kill().unwrap();
                panic!("peer deadline");
            }
            let result = (|| -> Result<(), ShellTransportError> {
                host.service_owner_turn()?;
                let now = start.elapsed().as_millis() as u64;
                host.service_content_resources(now)?;
                host.service_content_allocation_requests(&[], now)?;
                while let Some((_, request)) = host.next_content_allocation_request() {
                    // Grant exactly the requested geometry.
                    assert_eq!(request.desired_width, PANEL_WIDTH);
                    assert_eq!(request.desired_height, PANEL_HEIGHT);
                    host.grant_content_allocation(
                        request.allocation_request_id,
                        ContentAllocationSnapshot {
                            native_opening: None,
                            output,
                            allocation,
                            scale_generation: 1,
                            scale_numerator: 1,
                            scale_denominator: 1,
                            role: 1,
                            edge: 1,
                            margins: ContentMargins::default(),
                            logical: ContentLogicalRect {
                                x: 0,
                                y: 0,
                                width: request.desired_width,
                                height: request.desired_height,
                            },
                            pixel: ContentPixelRect {
                                x: 0,
                                y: 0,
                                width: request.desired_width,
                                height: request.desired_height,
                            },
                            parent: ContentAllocationId::default(),
                            anchor_parent_rect: ContentPixelRect::default(),
                            allowed_reservation_extent: PANEL_HEIGHT,
                        },
                        &[],
                    )?;
                }
                let allocations = host.content_allocation_snapshots();
                host.service_content_demands(&[output], &allocations)?;
                while let Some((transaction, demand)) = host.next_content_demand() {
                    host.grant_content_demand(transaction, output, demand.demand_id, now)?;
                }
                host.service_content_candidates(
                    &[ContentCandidateContext {
                        output,
                        facts_generation: 1,
                        interaction_generation: 1,
                        allocations: &allocations,
                    }],
                    now,
                )?;
                while let Some((output, generation)) = host.next_content_submission() {
                    let bundle = host.begin_content_submission(output, generation, now)?;
                    if !placed_canvas_matches(&bundle, PANEL_WIDTH, PANEL_HEIGHT) {
                        mismatched += 1;
                    }
                    held = Some(bundle);
                    let grant = host.content_grant().unwrap();
                    host.content_prepared(grant, output, generation, 1, 1, now)?;
                    host.content_presented(grant, output, generation, generation, 1, 1)?;
                    completed += 1;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {}
                Err(ShellTransportError::NotConnected) if completed == FRAMES => {}
                Err(error) => {
                    let _ = child.kill();
                    panic!(
                        "owner: {error:?}; stdout={}; stderr={}",
                        std::fs::read_to_string(root.join("stdout")).unwrap(),
                        std::fs::read_to_string(root.join("stderr")).unwrap()
                    );
                }
            }
            std::thread::yield_now();
        }
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success(), "large panel peer failed");
        assert_eq!(completed, FRAMES);
        assert_eq!(mismatched, 0, "placed 512-high panel bytes differ");
        drop(held);
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// The sealed source places the same 1920x512 panel: one memfd per
/// frame, full pattern, under the unchanged 4 MiB resource bound.
#[test]
#[ignore = "requires explicit prebuilt SOPHIA_TIMING_PEER_BINARY; device-hidden; supplied authority"]
fn supplied_authority_sealed_source_places_a_512_high_panel() {
    const FRAMES: u64 = 3;
    const PANEL_WIDTH: u32 = 1920;
    const PANEL_HEIGHT: u32 = 512;
    let binary = std::env::var_os("SOPHIA_TIMING_PEER_BINARY").expect("explicit peer binary");
    assert!(std::path::Path::new(&binary).is_absolute());
    #[cfg(feature = "comparator-journal-trace")]
    let trace = journal_trace::begin(JOURNAL_TRACE_EVENTS);
    let root =
        std::env::temp_dir().join(format!("timing-peer-sealed-large-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let grant = ContentGrant {
        connection_epoch: 1,
        content_grant_epoch: 1,
    };
    let limits = ContentLimits::prototype(grant);
    let (discovery, image_limits, formats, bounds) = sealed_image_config(&limits);
    let config = root.join("peer.json");
    std::fs::write(
        &config,
        serde_json::json!({"wire":"files","output":1,"frames":FRAMES,"height":PANEL_HEIGHT,
        "hold_msec":100,"probe_bytes":0,"source":"sealed",
        "image":{"max_resource_bytes":bounds.max_resource_bytes,"max_width":bounds.max_width,
        "max_height":bounds.max_height,"max_admitted_outputs":bounds.max_admitted_outputs,
        "transfer_slots":bounds.transfer_slots,"transfer_timeout_ms":limits.transfer_timeout_ms}})
        .to_string(),
    )
    .unwrap();
    let mut host = ShellComponentTransport::bind_for_supervised_uid(
        root.join("export"),
        rustix::process::geteuid().as_raw(),
    )
    .unwrap();
    let mut epochs = ContentEpochRegistry::new(64 << 20).unwrap();
    let (relay_host, relay_child) = std::os::unix::net::UnixStream::pair().unwrap();
    let relay_fd = std::os::fd::AsRawFd::as_raw_fd(&relay_child);
    let mut command = Command::new(&binary);
    command
        .arg("--serve")
        .env_remove("SOPHIA_SHELL_SOCKET")
        .env("SOPHIA_SHELL_CONFIG", &config)
        .env("SOPHIA_SHELL_9P_SOCKET", host.socket_path())
        .env("SOPHIA_LAUNCH_AUTH_FD", relay_fd.to_string())
        .stdout(std::fs::File::create(root.join("stdout")).unwrap())
        .stderr(std::fs::File::create(root.join("stderr")).unwrap());
    sophia_linux_peer::inherit_one_fd_on_exec(&mut command, relay_child.into()).unwrap();
    let mut child = command.spawn().unwrap();
    drop(command);
    let challenge = image_authority::challenge();
    let relay = std::thread::spawn(move || {
        image_relay::serve(
            relay_host,
            challenge,
            Duration::from_secs(5),
            Duration::from_secs(60),
        )
    });
    let owned = (|| -> Result<_, ShellTransportError> {
        host.authorize_authenticated_peer(
            &ProtectionDomainEvidence {
                backend: ProtectionBackendKind::Bubblewrap,
                supervisor_pid: child.id(),
                peer_pid: child.id(),
                roles: [ProtectionDomainRole::MetadataShell].into_iter().collect(),
            },
            Box::new(image_authority::Factory),
        )?;
        host.configure_image_channel(discovery, image_limits, formats, bounds)?;
        host.reserve_content(&mut epochs, limits.clone())?;
        host.begin_file_negotiation(
            &epochs,
            1,
            Duration::from_secs(5),
            ShellContentAdmissionPolicy::Granted {
                discrete_input: true,
            },
        )?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while host.poll_negotiation(&mut epochs, 65536)?.is_none() {
            if Instant::now() >= deadline {
                return Err(ShellTransportError::Io("negotiation stalled".into()));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })();
    let completed = match owned {
        Ok(()) => run_sealed_owner(&mut host, &mut epochs, &mut child, FRAMES, PANEL_HEIGHT),
        Err(error) => Err(format!("{error:?}")),
    };
    if completed.is_err() {
        let grace = Instant::now() + Duration::from_secs(2);
        while child.try_wait().unwrap().is_none() && Instant::now() < grace {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    let relay = relay.join().unwrap();
    let stdout = std::fs::read(root.join("stdout")).unwrap();
    let stderr = std::fs::read(root.join("stderr")).unwrap();
    let context = format!(
        "stdout={} stderr={}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    let (completed, mut image_owner, held) =
        completed.unwrap_or_else(|error| panic!("owner: {error}; {context}"));
    assert!(status.success(), "{context}");
    assert_eq!(relay, Ok(()), "{context}");
    assert_eq!(completed, FRAMES);
    let records: Vec<serde_json::Value> = String::from_utf8(stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let uploads: Vec<_> = records
        .iter()
        .filter(|r| r["event"] == "upload_offer")
        .collect();
    assert_eq!(uploads.len(), FRAMES as usize);
    for upload in uploads {
        assert_eq!(
            upload["upload"]["total_bytes"],
            PANEL_WIDTH * PANEL_HEIGHT * 4
        );
        assert_eq!(upload["upload"]["width"], PANEL_WIDTH);
        assert_eq!(upload["upload"]["height"], PANEL_HEIGHT);
    }
    assert_eq!(
        records
            .iter()
            .filter(|r| r["event"] == "terminal" && r["outcome"] == "presented")
            .count(),
        FRAMES as usize
    );
    drop(held);
    host.disconnect(&mut epochs).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while image_owner.pending() != 0 {
        assert!(Instant::now() < deadline, "image owner did not drain");
        image_owner
            .service(&mut host, &mut epochs, 0, Instant::now())
            .unwrap();
        std::thread::yield_now();
    }
    assert!(host.collect_content_accounting(&mut epochs).quiescent());
    drop(host);
    #[cfg(feature = "comparator-journal-trace")]
    journal_trace::finish(trace, "isb-512");
    std::fs::remove_dir_all(root).unwrap();
}

/// The journal trace classifier, on synthetic traces whose answers are
/// known: same-turn and later-turn outcomes and deliveries, a continuation
/// and an explicit reread, a command without an answer key, and a keyed
/// command still unanswered.
#[cfg(feature = "comparator-journal-trace")]
#[test]
fn journal_trace_classifies_by_record_semantics() {
    use sophia_9p::journal_trace::{AckResult, Event, ReadResult, RoleKey, RolePhase, RoleRecord};
    const J: u64 = 3;
    let key = |class, id| RoleKey {
        class,
        id,
        generation: 0,
    };
    let role = |sequence, phase, kind, key, status, related| {
        Event::Role(RoleRecord {
            journal: J,
            sequence,
            phase,
            kind,
            transaction: 0,
            key,
            status,
            related,
        })
    };
    let commit = |sequence: u64| Event::Commit {
        journal: J,
        first: sequence,
        last: sequence,
        start: (sequence - 1) * 10,
        end: sequence * 10,
    };
    let read = |offset, delivered, first, last, starts_inside, ends_inside| Event::Read {
        journal: J,
        offset,
        requested: 64,
        delivered,
        first,
        last,
        starts_inside,
        ends_inside,
        result: ReadResult::Ready,
        parked: 1,
    };
    let ack = |requested, result| Event::Ack {
        journal: J,
        epoch: 1,
        requested,
        result,
        acknowledged: requested,
        floor: requested * 10,
        last_committed: 3,
    };
    let tx_role = |sequence, phase, kind, key, status, related, transaction| match role(
        sequence, phase, kind, key, status, related,
    ) {
        Event::Role(record) => Event::Role(RoleRecord {
            transaction,
            ..record
        }),
        _ => unreachable!(),
    };
    let allocation = |id| key("allocation_request", id);
    let generation = key("candidate_generation", 7);
    let resource = RoleKey {
        class: "resource",
        id: 4,
        generation: 1,
    };
    let events = vec![
        Event::Opened {
            journal: J,
            epoch: 1,
            next_sequence: 1,
            tail: 0,
        },
        Event::Mark { label: 1 },
        commit(1),
        role(1, RolePhase::Appended, 4, RoleKey::NONE, 0, 0),
        role(1, RolePhase::Submitted, 34, generation, 0, 11),
        commit(2),
        role(2, RolePhase::Outcome, 35, generation, 1, 0),
        read(0, 20, 1, 2, false, false),
        Event::Mark { label: 2 },
        ack(2, AckResult::Advanced),
        role(1, RolePhase::Released, 4, key("submission", 11), 0, 2),
        commit(3),
        role(3, RolePhase::Outcome, 35, generation, 2, 0),
        Event::Mark { label: 3 },
        // Record 3 is delivered a turn after its commit, in two reads.
        read(20, 4, 3, 3, false, true),
        read(24, 6, 3, 3, true, false),
        // Rereading record 1 delivers nothing new.
        read(0, 10, 1, 1, false, false),
        ack(2, AckResult::Repeat),
        commit(4),
        role(4, RolePhase::Submitted, 50, RoleKey::NONE, 0, 12),
        commit(5),
        tx_role(5, RolePhase::Submitted, 40, allocation(9), 0, 13, 21),
        // A host-initiated answer has no submission.
        commit(6),
        role(6, RolePhase::Outcome, 41, allocation(8), 0, 0),
        // A resubmission under the same key takes the answer by its
        // transaction; the first stays explicitly unanswered.
        commit(7),
        tx_role(7, RolePhase::Submitted, 40, allocation(9), 0, 14, 22),
        commit(8),
        tx_role(8, RolePhase::Outcome, 41, allocation(9), 0, 0, 22),
        // A late answer to an image offer joins the offer by its
        // transaction, though a cancel has taken the resource key since.
        commit(9),
        tx_role(9, RolePhase::Submitted, 60, resource, 0, 15, 30),
        commit(10),
        tx_role(10, RolePhase::Submitted, 61, resource, 0, 16, 31),
        commit(11),
        tx_role(11, RolePhase::Outcome, 62, resource, 1, 0, 30),
        // Neither holder carries this transaction, and without one two
        // holders cannot be told apart: both stay unmatched.
        commit(12),
        tx_role(12, RolePhase::Outcome, 62, resource, 2, 0, 32),
        commit(13),
        role(13, RolePhase::Outcome, 62, resource, 3, 0),
    ];
    let summary = journal_trace::analyze(&events, 0);
    assert_eq!(summary["violations"], serde_json::json!([]));
    let classes: Vec<_> = summary["submissions"][0]["outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["sequence"].clone(),
                o["status"].clone(),
                o["class"].clone(),
            )
        })
        .collect();
    assert_eq!(
        classes,
        [
            (2.into(), 1.into(), "same_turn".into()),
            (3.into(), 2.into(), "later_turn".into())
        ]
    );
    assert_eq!(
        summary["submissions"][0]["released"],
        serde_json::json!({"ack": 2, "ordinal": 10, "turn": 2})
    );
    assert_eq!(
        summary["submitted_by_kind"],
        serde_json::json!({
            "34": {"submitted": 1, "first_outcome_same_turn": 1, "outcomes": 2},
            "40": {"submitted": 2, "superseded_without_outcome": 1,
                "first_outcome_same_turn": 1, "outcomes": 1, "unreleased": 2},
            "50": {"submitted": 1, "no_outcome_key": 1, "outcomes": 0, "unreleased": 1},
            "60": {"submitted": 1, "first_outcome_same_turn": 1, "outcomes": 1, "unreleased": 1},
            "61": {"submitted": 1, "keyed_without_outcome": 1, "outcomes": 0, "unreleased": 1},
        })
    );
    assert_eq!(summary["submissions"][2]["superseded_by"], 7);
    assert_eq!(
        summary["submissions"][3]["outcomes"][0]["join"],
        "key_and_transaction"
    );
    assert_eq!(summary["submissions"][4]["outcomes"][0]["sequence"], 11);
    assert_eq!(summary["submissions"][0]["outcomes"][0]["join"], "key_only");
    let unmatched: Vec<_> = summary["unmatched_outcomes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| {
            (
                o["sequence"].clone(),
                o["reason"].clone(),
                o["candidates"].clone(),
            )
        })
        .collect();
    assert_eq!(
        unmatched,
        [
            (6.into(), "no_submission".into(), serde_json::json!([])),
            (
                12.into(),
                "transaction_mismatch".into(),
                serde_json::json!([9, 10])
            ),
            (
                13.into(),
                "ambiguous_key".into(),
                serde_json::json!([9, 10])
            ),
        ]
    );
    let journal = &summary["journals"][0];
    assert_eq!(
        journal["delivery"],
        serde_json::json!({"same_turn": 2, "later_turn": 1, "undelivered": 10})
    );
    assert_eq!(
        journal["reads"],
        serde_json::json!({"advance": 2, "continuation": 1, "reread": 1,
            "skip": 0, "stale": 0, "invalid": 0})
    );
    assert_eq!(journal["delivered_through"], 3);
    // Parked reads count toward the next record a read completes: one before
    // records 1 and 2, two before record 3 across its split, and the
    // reread's after the last delivery.
    assert_eq!(journal["parked_before_delivery"], 3);
    assert_eq!(journal["parked_after_last_delivery"], 1);
    assert_eq!(journal["acks"]["repeat"], 1);
    assert_eq!(journal["kinds"]["4"], 1);

    // An acknowledgement past the delivered records, a release without its
    // acknowledgement, and overflow are each violations.
    let broken = vec![
        events[0],
        commit(1),
        read(0, 10, 1, 1, false, false),
        commit(2),
        ack(2, AckResult::Advanced),
        role(1, RolePhase::Released, 4, RoleKey::NONE, 0, 1),
    ];
    let summary = journal_trace::analyze(&broken, 1);
    // A read that skips record 2 does not deliver it, so acknowledging
    // record 3 is not covered.
    let skipped = vec![
        events[0],
        commit(1),
        commit(2),
        commit(3),
        read(0, 10, 1, 1, false, false),
        read(20, 10, 3, 3, false, false),
        ack(3, AckResult::Advanced),
    ];
    let skipped = journal_trace::analyze(&skipped, 0);
    assert_eq!(
        skipped["violations"],
        serde_json::json!([
            "journal 3 read at 20 skips from 10",
            "journal 3 ack 3 passes delivered 1",
        ])
    );
    assert_eq!(skipped["journals"][0]["reads"]["skip"], 1);
    assert_eq!(skipped["journals"][0]["delivered_through"], 1);
    assert_eq!(
        summary["violations"],
        serde_json::json!([
            "journal 3 ack 2 passes delivered 1",
            "journal 3 release of 1 follows no ack 1",
            "journal 3 released unknown receipt 1",
            "1 trace events did not fit",
        ])
    );
}

/// Journal trace storage per traced run, reserved up front. Turn marks
/// coalesce, so the busy owner loop does not consume it.
#[cfg(feature = "comparator-journal-trace")]
const JOURNAL_TRACE_EVENTS: usize = 1 << 18;

fn sealed_image_config(
    limits: &ContentLimits,
) -> (
    shell_files::ShellImageDiscovery,
    shell_files::ShellImageLimits,
    shell_files::ShellImageFormats,
    shell_files::ShellImageRoleBounds,
) {
    let offers = limits.max_open_transfers.min(4);
    let (validation_timeout_ms, chunk) = (2000, 65536);
    (
        shell_files::ShellImageDiscovery {
            connection_epoch: limits.grant.connection_epoch,
            channel_generation: 1,
            endpoint: "image.sock".into(),
            source_kinds: 1,
            max_offers: offers,
            max_packets_per_turn: 1,
            hello_timeout_ms: 1000,
            validation_timeout_ms,
            validation_chunk_bytes: chunk,
            capture_interest_timeout_ms: 500,
            formats_generation: 1,
            limits_generation: 1,
        },
        shell_files::ShellImageLimits {
            channel_generation: 1,
            limits_generation: 1,
            max_resource_bytes: limits.max_resource_bytes,
            max_sealed_backing_bytes: limits.max_resident_bytes,
            max_dma_source_bytes: 0,
            max_renderer_allocation_bytes: 0,
            max_renderer_entries: 0,
            max_offers: offers,
            max_validation_jobs: offers,
            max_capture_jobs: 0,
            max_admitted_outputs: limits.max_outputs,
            max_format_rows_per_output: 1,
            max_width: limits.max_width_px,
            max_height: limits.max_height_px,
            validation_timeout_ms,
            capture_interest_timeout_ms: 500,
            validation_chunk_bytes: chunk,
            max_packets_per_turn: 1,
        },
        shell_files::ShellImageFormats {
            channel_generation: 1,
            formats_generation: 1,
            limits_generation: 1,
            rows: vec![],
        },
        shell_files::ShellImageRoleBounds {
            max_resource_bytes: limits.max_resource_bytes,
            max_width: limits.max_width_px,
            max_height: limits.max_height_px,
            max_admitted_outputs: limits.max_outputs,
            transfer_slots: limits.max_open_transfers,
        },
    )
}

/// The supplied owner loop of the byte test, on a component transport with
/// its sealed image owner serviced every turn.
fn run_sealed_owner(
    host: &mut ShellComponentTransport,
    epochs: &mut ContentEpochRegistry,
    child: &mut std::process::Child,
    frames: u64,
    panel_height: u32,
) -> Result<(u64, ShellIsbImageOwner, Option<ContentRenderBundle>), String> {
    const PANEL_WIDTH: u32 = 1920;
    let output = ContentOutputId {
        id: 1,
        generation: 1,
    };
    let allocation = ContentAllocationId {
        id: 1,
        generation: 1,
    };
    let limits = host.content_limits().cloned().ok_or("no content limits")?;
    let mut image_owner = ShellIsbImageOwner::new(limits.max_resource_bytes as usize, 65536)
        .map_err(|e| e.to_string())?;
    host.publish_content_output_facts(
        epochs,
        TransactionId::from_raw(1),
        1,
        vec![ContentOutputFactsEntry {
            output,
            local_width: PANEL_WIDTH,
            local_height: 1080,
            scale_numerator: 1,
            scale_denominator: 1,
            scale_generation: 1,
        }],
    )
    .map_err(|e| format!("{e:?}"))?;
    let start = Instant::now();
    let mut completed = 0;
    let mut mismatched = 0;
    let mut held = None;
    let mut stage = "start";
    #[cfg(feature = "comparator-journal-trace")]
    let mut turn = 0;
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if start.elapsed() > Duration::from_secs(30) {
            return Err("peer deadline".into());
        }
        #[cfg(feature = "comparator-journal-trace")]
        {
            turn += 1;
            sophia_9p::journal_trace::mark(turn);
        }
        let result = (|| -> Result<(), ShellTransportError> {
            stage = "owner turn";
            host.service_owner_turn(epochs)?;
            let now = start.elapsed().as_millis() as u64;
            stage = "resources";
            host.service_content_resources(epochs, now)?;
            stage = "image owner";
            image_owner.service(host, epochs, now, Instant::now())?;
            stage = "allocations";
            host.service_content_allocation_requests(epochs, &[], now)?;
            while let Some((_, request)) = host.next_content_allocation_request(epochs) {
                let rect = ContentPixelRect {
                    x: 0,
                    y: 0,
                    width: PANEL_WIDTH,
                    height: panel_height,
                };
                host.grant_content_allocation(
                    epochs,
                    request.allocation_request_id,
                    ContentAllocationSnapshot {
                        native_opening: None,
                        output,
                        allocation,
                        scale_generation: 1,
                        scale_numerator: 1,
                        scale_denominator: 1,
                        role: 1,
                        edge: 1,
                        margins: ContentMargins::default(),
                        logical: ContentLogicalRect {
                            x: 0,
                            y: 0,
                            width: PANEL_WIDTH,
                            height: panel_height,
                        },
                        pixel: rect,
                        parent: ContentAllocationId::default(),
                        anchor_parent_rect: ContentPixelRect::default(),
                        allowed_reservation_extent: panel_height,
                    },
                    &[],
                )?;
            }
            let allocations = host.content_allocation_snapshots(epochs);
            stage = "demands";
            host.service_content_demands(epochs, &[output], &allocations)?;
            while let Some((transaction, demand)) = host.next_content_demand(epochs) {
                host.grant_content_demand(epochs, transaction, output, demand.demand_id, now)?;
            }
            stage = "candidates";
            host.service_content_candidates(
                epochs,
                &[ContentCandidateContext {
                    output,
                    facts_generation: 1,
                    interaction_generation: 1,
                    allocations: &allocations,
                }],
                now,
            )?;
            while let Some((output, generation)) = host.next_content_submission(epochs) {
                let bundle = host.begin_content_submission(epochs, output, generation, now)?;
                if !placed_canvas_matches(&bundle, PANEL_WIDTH, panel_height) {
                    mismatched += 1;
                }
                held = Some(bundle);
                let grant = host.content_grant().unwrap();
                host.content_prepared(epochs, grant, output, generation, 1, 1, now)?;
                host.content_presented(epochs, grant, output, generation, generation, 1, 1)?;
                completed += 1;
            }
            Ok(())
        })();
        match result {
            Ok(()) | Err(ShellTransportError::NotConnected) if completed == frames => {}
            Ok(()) => {}
            Err(error) => return Err(format!("{stage}: {error:?}")),
        }
        std::thread::yield_now();
    }
    if mismatched != 0 {
        return Err(format!(
            "{mismatched} presented panels differ from the canvas"
        ));
    }
    Ok((completed, image_owner, held))
}

/// The one placed panel carries exactly its candidate's canvas bytes, in
/// order, whichever wire or source delivered it.
fn panel_canvas_matches(bundle: &ContentRenderBundle, width: u32) -> bool {
    placed_canvas_matches(bundle, width, 24)
}

/// The placed resource is exactly `width` x `height` of this
/// generation's canvas pattern: every byte checked, none sampled.
fn placed_canvas_matches(bundle: &ContentRenderBundle, width: u32, height: u32) -> bool {
    let shade = (bundle.candidate_generation % 128) as u8;
    let [placement] = bundle.placements.as_slice() else {
        return false;
    };
    let Some(lease) = bundle.resource(placement.resource) else {
        return false;
    };
    let bytes = lease.bytes();
    bytes.len() == width as usize * height as usize * 4
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| *byte == if index % 4 == 3 { 255 } else { shade })
}
