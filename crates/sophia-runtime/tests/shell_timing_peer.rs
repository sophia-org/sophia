//! Real child, supplied admission and completion. No native display claim.
use sophia_protocol::*;
use sophia_runtime::*;
#[path = "support/shell_timing_image_authority.rs"]
mod image_authority;
#[path = "support/shell_timing_image_relay.rs"]
mod image_relay;
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
fn supplied_owner_records_every_offer_and_terminal_on_both_wires() {
    const FRAMES: u64 = 64;
    const PANEL_WIDTH: u32 = 1920;
    #[cfg(feature = "comparator-counters")]
    let sampler = sophia_runtime::comparator_counters::Sampler::start().unwrap();
    let binary = std::env::var_os("SOPHIA_TIMING_PEER_BINARY").expect("explicit peer binary");
    assert!(std::path::Path::new(&binary).is_absolute());
    for (files, probe) in [(true, false), (false, false), (true, true), (false, true)] {
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
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// A 1920x512 panel is actually placed at full height on the byte
/// wires: the host grants the requested geometry, and every placed
/// resource carries the whole generation pattern. Three frames is
/// the smallest count that exercises both alternating resources and
/// one reuse.
#[test]
#[ignore = "requires explicit prebuilt SOPHIA_TIMING_PEER_BINARY; device-hidden"]
fn supplied_owner_places_a_512_high_panel_on_both_wires() {
    const FRAMES: u64 = 3;
    const PANEL_WIDTH: u32 = 1920;
    const PANEL_HEIGHT: u32 = 512;
    let binary = std::env::var_os("SOPHIA_TIMING_PEER_BINARY").expect("explicit peer binary");
    assert!(std::path::Path::new(&binary).is_absolute());
    for files in [true, false] {
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
    std::fs::remove_dir_all(root).unwrap();
}

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
) -> Result<(u64, ShellSealedImageOwner, Option<ContentRenderBundle>), String> {
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
    let mut image_owner = ShellSealedImageOwner::new(limits.max_resource_bytes as usize, 65536)
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
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if start.elapsed() > Duration::from_secs(30) {
            return Err("peer deadline".into());
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
