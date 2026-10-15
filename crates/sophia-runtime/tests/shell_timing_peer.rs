//! Real child, supplied admission and completion. No native display claim.
use sophia_protocol::*;
use sophia_runtime::*;
use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

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
fn supplied_owner_records_every_offer_and_terminal_on_both_wires() {
    let binary = std::env::var_os("SOPHIA_TIMING_PEER_BINARY").expect("explicit peer binary");
    assert!(std::path::Path::new(&binary).is_absolute());
    for files in [true, false] {
        let root = std::env::temp_dir().join(format!("timing-peer-{}-{files}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let config = root.join("peer.json");
        std::fs::write(
            &config,
            serde_json::json!({"wire":if files {"files"} else {"ipc"},
            "output":1,"frames":3,"height":24,"hold_msec":100})
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
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
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
                local_width: 64,
                local_height: 64,
                scale_numerator: 1,
                scale_denominator: 1,
                scale_generation: 1,
            }],
        )
        .unwrap();
        let start = Instant::now();
        let mut completed = 0;
        let mut held = None;
        while child.try_wait().unwrap().is_none() {
            if start.elapsed() > Duration::from_secs(10) {
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
                                width: 64,
                                height: 24,
                            },
                            pixel: ContentPixelRect {
                                x: 0,
                                y: 0,
                                width: 64,
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
                    held = Some(host.begin_content_submission(output, generation, now)?);
                    let grant = host.content_grant().unwrap();
                    host.content_prepared(grant, output, generation, 1, 1, now)?;
                    host.content_presented(grant, output, generation, generation, 1, 1)?;
                    completed += 1;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {}
                Err(ShellTransportError::NotConnected) if completed == 3 => {}
                Err(error) => {
                    let _ = child.kill();
                    panic!("owner: {error:?}");
                }
            }
            std::thread::yield_now();
        }
        let result = child.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "stdout={} stderr={}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        if let Some(directory) = std::env::var_os("SOPHIA_TIMING_PEER_JOURNALS") {
            use std::io::Write;
            let directory = std::path::Path::new(&directory);
            assert!(directory.is_absolute());
            let name = if files { "files.ndjson" } else { "ipc.ndjson" };
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(name))
                .unwrap()
                .write_all(&result.stdout)
                .unwrap();
        }
        let records: Vec<serde_json::Value> = String::from_utf8(result.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(completed, 3);
        let offers: Vec<_> = records.iter().filter(|r| r["event"] == "offer").collect();
        let terminals: Vec<_> = records
            .iter()
            .filter(|r| r["event"] == "terminal")
            .collect();
        assert_eq!(offers.len(), 3);
        assert_eq!(terminals.len(), 3);
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
}
