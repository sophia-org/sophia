#![cfg(target_os = "linux")]
//! Opt-in external lock renderer against the real file service. No desktop,
//! input device, authentication, secret or installation is involved. The
//! fixture config must use the ordinary indigo/blue/green input palette.
use sophia_protocol::lock_files::*;
use sophia_runtime::lock_files::*;
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Provider(Child);
impl Drop for Provider {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn ticks(pid: u32) -> u64 {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let fields: Vec<_> = stat[stat.rfind(')').unwrap() + 2..]
        .split_whitespace()
        .collect();
    fields[11].parse::<u64>().unwrap() + fields[12].parse::<u64>().unwrap()
}

#[test]
#[ignore = "explicit external provider binary and synthetic config required"]
fn external_provider_renders_both_outputs_and_prioritizes_feedback() {
    let binary = std::env::var("SOPHIA_TEST_LOCK_PROVIDER").unwrap();
    let config = std::env::var("SOPHIA_TEST_LOCK_PROVIDER_CONFIG").unwrap();
    assert!(std::path::Path::new(&binary).is_absolute());
    let sizes = if std::env::var_os("SOPHIA_TEST_LOCK_NATIVE_SIZES").is_some() {
        [(2560, 1440), (1920, 1080)]
    } else {
        [(640, 480), (320, 240)]
    };
    let seconds: u64 = std::env::var("SOPHIA_TEST_LOCK_SECONDS")
        .map_or(Ok(2), |s| s.parse())
        .unwrap();
    let dir = std::env::temp_dir().join(format!("lock-provider-workload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lock = LockObject {
        lock_epoch: 3,
        topology_generation: 1,
        phase: LockPhase::Locked,
        allocations: sizes
            .iter()
            .enumerate()
            .map(|(i, &(w, h))| LockAllocation {
                output_id: i as u64 + 1,
                output_generation: 1,
                allocation_id: i as u64 + 1,
                allocation_generation: 1,
                pixel_width: w,
                pixel_height: h,
                scale_numerator: 1,
                scale_denominator: 1,
            })
            .collect(),
    };
    let mut transport = LockFileTransport::bind_for_supervised_uid(
        dir.join("endpoint"),
        rustix::process::geteuid().as_raw(),
        5,
        LockFileLimits {
            max_outputs: 2,
            upload_slots: 2,
            max_chords: 8,
            max_width_px: sizes[0].0,
            max_height_px: sizes[0].1,
            max_resource_bytes: u64::from(sizes[0].0) * u64::from(sizes[0].1) * 4,
            max_live_resources: 4,
            journal_records: 128,
            journal_bytes: 32768,
            assembly_timeout_ms: 2000,
            ack_progress_timeout_ms: 2000,
        },
    )
    .unwrap();
    let mut launch = Command::new("/bin/sh");
    launch.args(["-c", "read start; exec \"$@\"", "provider", &binary]);
    launch
        .env_clear()
        .env("SOPHIA_LOCK_9P_SOCKET", transport.socket_path())
        .env("SOPHIA_LOCK_CONFIG", config)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    // An isolated render-node test may pass exactly its explicit grant.
    for key in [
        "SOPHIA_SHELL_GPU_MODE",
        "SOPHIA_SHELL_GPU_RENDER_NODE",
        "SOPHIA_SHELL_GPU_DEVICE_MAJOR",
        "SOPHIA_SHELL_GPU_DEVICE_MINOR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            launch.env(key, value);
        }
    }
    let mut provider = Provider(launch.spawn().unwrap());
    transport.authorize_supervised_pid(provider.0.id()).unwrap();
    let service = LockFileService::spawn(transport, lock, vec![]).unwrap();
    provider
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(b"start\n")
        .unwrap();
    let start = Instant::now();
    let mut warm = [0_u64; 2];
    let mut frames = [0_u64; 2];
    let mut gaps: [Vec<f64>; 2] = [vec![], vec![]];
    let mut last = [None; 2];
    let mut measured = None;
    let mut cpu_before = (0, 0);
    let mut sample = None;
    let mut feedback_at = None;
    let mut feedback = [None; 2];
    let mut feedback_samples: [Vec<f64>; 2] = [vec![], vec![]];
    let palette = [0xff4b0082_u32, 0xff003366, 0xff006400];
    let mut expected_color = palette[0];
    let mut resources = BTreeMap::new();
    while feedback_samples[0].len() < 20 {
        assert!(
            start.elapsed() < Duration::from_secs(seconds + 15),
            "provider stalled: warm={warm:?}, frames={frames:?}, feedback={feedback:?}"
        );
        assert!(provider.0.try_wait().unwrap().is_none(), "provider exited");
        if measured.is_some_and(|t: Instant| t.elapsed() >= Duration::from_secs(seconds))
            && feedback_at.is_none()
        {
            if sample.is_none() {
                let elapsed = measured.unwrap().elapsed().as_secs_f64();
                sample = Some((
                    elapsed,
                    ticks(provider.0.id()) - cpu_before.0,
                    ticks(std::process::id()) - cpu_before.1,
                ));
            }
            expected_color = palette[feedback_samples[0].len() % palette.len()];
            service
                .command(LockFileServiceCommand::Entry(LockEntry {
                    lock_epoch: 3,
                    entry: LockEntryKind::Insert,
                    empty_after: false,
                }))
                .unwrap();
            feedback_at = Some(Instant::now());
        }
        let event = match service.event_timeout(Duration::from_millis(10)) {
            Ok(e) => e,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(e) => panic!("service ended: {e:?}"),
        };
        match event {
            LockFileServiceEvent::Connected { .. } => {}
            LockFileServiceEvent::Inbound { inbound, .. } => match inbound {
                LockInbound::ResourceReady {
                    resource,
                    width_px,
                    height_px,
                    pixels,
                } => {
                    assert!(sizes.contains(&(width_px, height_px)));
                    assert_eq!(pixels.len(), width_px as usize * height_px as usize * 4);
                    // Pixel inspection is outside the CPU interval. The image
                    // must be an opaque whole color, not just one changed pixel.
                    let color = u32::from_le_bytes(pixels[..4].try_into().unwrap());
                    let solid = feedback_at.is_some()
                        && pixels.chunks_exact(4).all(|p| p == color.to_le_bytes());
                    resources.insert(resource, solid.then_some(color));
                    assert!(resources.len() <= 4, "resource custody grew");
                }
                LockInbound::ResourceRetired(resource) => {
                    resources.remove(&resource);
                }
                LockInbound::Demand(d) => service
                    .command(LockFileServiceCommand::Permit {
                        allocation_id: d.allocation_id,
                        demand_id: d.demand_id,
                        expires_after: Duration::from_millis(100),
                    })
                    .unwrap(),
                LockInbound::Candidate { candidate, .. } => {
                    let i = candidate.output_id as usize - 1;
                    assert!(i < 2);
                    assert_eq!(candidate.allocation_id, candidate.output_id);
                    assert_eq!(candidate.lock_epoch, 3);
                    assert!(resources.contains_key(&candidate.resource));
                    service
                        .command(LockFileServiceCommand::Outcome(LockCandidateOutcome {
                            transaction: candidate.transaction,
                            lock_epoch: candidate.lock_epoch,
                            output_id: candidate.output_id,
                            allocation_id: candidate.allocation_id,
                            candidate_generation: candidate.candidate_generation,
                            status: LockCandidateStatus::Presented,
                            reason: 0,
                        }))
                        .unwrap();
                    if let Some(at) = feedback_at {
                        if resources[&candidate.resource] == Some(expected_color)
                            && feedback[i].is_none()
                        {
                            feedback[i] = Some(at.elapsed().as_secs_f64() * 1000.0);
                        }
                    } else if measured.is_some() {
                        frames[i] += 1;
                        if let Some(previous) = last[i] {
                            gaps[i].push(
                                Instant::now().duration_since(previous).as_secs_f64() * 1000.0,
                            );
                        }
                        last[i] = Some(Instant::now());
                    } else {
                        warm[i] += 1;
                        if warm.iter().all(|&n| n >= 3) {
                            cpu_before = (ticks(provider.0.id()), ticks(std::process::id()));
                            measured = Some(Instant::now());
                        }
                    }
                }
                LockInbound::Negotiated { .. } => {}
            },
            other => panic!("provider failure: {other:?}"),
        }
        if feedback.iter().all(Option::is_some) {
            for i in 0..2 {
                feedback_samples[i].push(feedback[i].take().unwrap());
            }
            feedback_at = None;
        }
    }
    let (elapsed, provider_ticks, host_ticks) = sample.unwrap();
    for g in &mut gaps {
        g.sort_by(f64::total_cmp);
    }
    for samples in &mut feedback_samples {
        samples.sort_by(f64::total_cmp);
    }
    let p95 = |g: &Vec<f64>| g[(g.len() - 1) * 95 / 100];
    assert!(frames.iter().all(|&n| n > 1));
    println!(
        "lock_provider_workload elapsed={elapsed:.3} frames={frames:?} fps={:?} \
        gap_p95_ms={:?} feedback_p95_ms={:?} feedback_samples=20 \
        provider_ticks={provider_ticks} \
        harness_ticks={host_ticks} service={:?}",
        frames.map(|n| n as f64 / elapsed),
        gaps.each_ref().map(p95),
        feedback_samples.each_ref().map(p95),
        service.stats()
    );
    drop(provider);
    drop(service);
    std::fs::remove_dir_all(dir).unwrap();
}
