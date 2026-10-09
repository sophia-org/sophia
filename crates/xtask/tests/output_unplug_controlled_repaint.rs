//! The controlled-repaint verdict (t307 part 2) on logs built from the
//! emitters' own formats: the retained run, and each declared way a run is
//! LOST, UNREADY, INSUFFICIENT or INVALID instead.

#[path = "../src"]
mod source {
    pub mod output_unplug;
}

use source::output_unplug::{Mode, probe_frame_checksum, verify};

const HEADS: [u32; 2] = [1, 2];

/// A per-(head, shift) override of what the run shows.
type Override<T> = Box<dyn Fn(u32, usize) -> Option<T>>;

/// One run, as the guest, the host, Session, the WM and the renderer write
/// it. Every field is one declared fact a control may change.
struct Run {
    ready: bool,
    presents: usize,
    /// Committed hold-shift actions, one per host press.
    shifts: usize,
    /// Seconds from the Present's retirement to the first shift's frames.
    first_shift_s: f64,
    /// The pixels of (head, shift) when not the client's frame.
    region: Override<(u64, u64)>,
    /// The x offset head shows for shift, when not the committed one.
    offset: Override<u32>,
    /// Shifts after which a head presents nothing.
    stops_after: Option<(u32, usize)>,
    renderer: Box<dyn Fn(u32) -> u32>,
    owner: Box<dyn Fn(u32) -> u32>,
    import_heads: Vec<u32>,
    outcome: &'static str,
    /// A late presented region, seconds after the retirement, on head 2.
    late: Option<(f64, u64)>,
    hotplug: u32,
    extra: String,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            ready: true,
            presents: 1,
            shifts: 20,
            first_shift_s: 1.2,
            region: Box::new(|_, _| None),
            offset: Box::new(|_, _| None),
            stops_after: None,
            renderer: Box::new(|head| 10 + head),
            owner: Box::new(|_| 1),
            import_heads: HEADS.to_vec(),
            outcome: "Committed",
            late: None,
            hotplug: 0,
            extra: String::new(),
        }
    }
}

fn controlled_repaint() -> Mode {
    Mode::parse("controlled-repaint").unwrap()
}

fn checksum() -> u64 {
    probe_frame_checksum(400, 300)
}

/// A tracing line stamped `seconds` after 12:00:01 guest time.
fn traced(seconds: f64, record: &str) -> String {
    let micros = (1_000_000.0 + seconds * 1_000_000.0).round() as i64;
    format!(
        "2026-10-09T12:{:02}:{:02}.{:06}Z  INFO sophia_test: {record}\n",
        micros / 60_000_000,
        micros / 1_000_000 % 60,
        micros % 1_000_000
    )
}

impl Run {
    fn frame(
        &self,
        log: &mut String,
        seconds: f64,
        head: u32,
        frame: usize,
        x: u32,
        pixels: (u64, u64),
    ) {
        log.push_str(&traced(seconds, &format!(
            "sophia_live_head_composition_queue schema=1 status=queued output=1 head={head} frame={frame} scene_generation={frame} target_generation=1"
        )));
        log.push_str(&traced(seconds + 0.001, &format!(
            "sophia_native_composition_region_frame schema=1 status=read output=1 head={head} scene_generation={frame} layer=0 source_stage=renderer_image target=400x300_{x}_0 region_pixels=120000 nonzero_rgb_pixels={} checksum={}",
            pixels.1, pixels.0
        )));
        log.push_str(&traced(seconds + 0.010, &format!(
            "sophia_live_native_head_page_flip schema=2 status=retired output=1 head={head} submission={frame} frame={frame}"
        )));
    }

    fn imports(&self, log: &mut String, head: u32, frame: usize, imports: u32) {
        if self.import_heads.contains(&head) {
            log.push_str(&format!(
                "sophia_live_head_renderer_imports schema=1 status=observed reason=initial renderer={} owner={} output=1 head={head} frame={frame} target_generation=1 scene_generation={frame} imports={imports} evictions=0 live_entries={imports} hits=0 descriptor_mismatches=0 capacity_rejections=0 snapshot_captures=1 snapshot_promotions=1 snapshot_live_entries=1 records=1\n",
                (self.renderer)(head),
                (self.owner)(head),
            ));
        }
    }

    fn build(&self) -> String {
        let mut log = String::from("\
sophia_qemu_unplug schema=1 status=starting isolation=headless control=none host_drm=none host_vt=none gpu=virtio-gpu mode=controlled-repaint single_card=1
sophia_qemu_guest schema=1 status=booting gpu=virtio-gpu scenario=output-unplug
sophia_qemu_topology schema=1 status=observed requested_heads=2 connectors=2 connected=2
sophia_qemu_unplug schema=1 status=running mode=controlled-repaint wm=true client=dri3
sophia_live_native_owner schema=1 status=opened epoch=1 reason=startup
");
        let good = (checksum(), 120_000);
        for head in HEADS {
            self.frame(&mut log, -0.05, head, 1, 0, good);
            self.imports(&mut log, head, 1, 1);
        }
        for present in 0..self.presents {
            log.push_str(&traced(present as f64 * 0.5, &format!(
                "sophia_live_session_present schema=2 status=retired transaction={} surface=2097153 source=400x300 target=400x300_0_0 clip=400x300_0_0 unit_scale=true ust=1 msc=1",
                26 + present
            )));
        }
        if !self.ready {
            // The 153 shape: never ready, a first region of the frame and a
            // later one that is not.
            self.frame(&mut log, 1.0, 1, 2, 0, (7, 0));
            log.push_str(&self.extra);
            return log;
        }
        log.push_str(&traced(0.2, "sophia_live_session_startup schema=2 status=ready elapsed_msec=900 surface=true visual_detail=true presented=true outputs_ready=1/1 recovery_attempts=0"));
        log.push_str(
            "\
dri3_layout stage=holding hold_ms=35000
sophia_qemu_unplug schema=1 status=static_barrier present=retired client=holding
sophia_qemu_unplug schema=1 status=cadence_sending key=f9 count=20 period_ms=1000
",
        );
        for press in 0..20 {
            log.push_str(&format!(
                "sophia_qemu_unplug schema=1 status=key_cadence index={press} key=f9 down_ms={} up_ms={} late_ms=0\n",
                press * 1000,
                press * 1000 + 80
            ));
            if press >= self.shifts {
                continue;
            }
            let (transaction, request, serial) = (100 + press, 50 + press, 1 + press);
            let x = 8 * (press as u32 + 1);
            log.push_str(&format!(
                "sophia_qemu_wm_hold schema=1 status=proposed transaction={transaction} request_id={request} cause=1 activation_serial={serial} shift=1 offset_x={x} placements=1\n\
sophia_shell_action_policy schema=1 policy_connection_epoch=1 activation_serial={serial} action=1 transaction={transaction} request_id={request} indicator_generation=0 outcome={} target_output=0 target_generation=0\n\
sophia_live_wm_chrome schema=2 status=settled transaction={transaction} request_id={request} scene_generation={} outcome={}\n\
sophia_qemu_wm_hold schema=1 status=outcome transaction={transaction} request_id={request} scene_generation={} outcome=1\n",
                self.outcome,
                press + 2,
                self.outcome,
                press + 2,
            ));
            for head in HEADS {
                if self
                    .stops_after
                    .is_some_and(|(h, after)| h == head && press >= after)
                {
                    continue;
                }
                let seconds = self.first_shift_s + press as f64;
                let pixels = (self.region)(head, press).unwrap_or(good);
                let x = (self.offset)(head, press).unwrap_or(x);
                self.frame(&mut log, seconds, head, press + 2, x, pixels);
            }
        }
        if let Some((seconds, pixels)) = self.late {
            self.frame(&mut log, seconds, 2, 40, 0, (pixels, 0));
        }
        log.push_str(&self.extra);
        log.push_str(&format!(
            "\
sophia_qemu_unplug schema=1 status=cadence_sent result=completed
sophia_qemu_unplug schema=1 status=uevents drm_hotplug={} input_remove=0 input_add=0
sophia_live_session schema=7 status=bounded_complete display=:181 elapsed_msec=40000
sophia_qemu_guest schema=1 status=complete scenario=output-unplug
sophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0
",
            self.hotplug
        ));
        log
    }
}

fn verdict(run: Run) -> Result<Vec<String>, String> {
    verify(&run.build(), controlled_repaint())
}

fn refused(run: Run, status: &str, reason: &str) {
    match verdict(run) {
        Err(line) => {
            assert!(
                line.starts_with(&format!(
                    "sophia_qemu_controlled_repaint_verdict schema=1 status={status}"
                )) && line.contains(reason),
                "expected {status} naming {reason:?}, got {line}"
            );
        }
        Ok(lines) => panic!("expected {status} naming {reason:?}, got a pass: {lines:?}"),
    }
}

#[test]
fn a_retained_image_on_two_importers_through_every_committed_shift_passes() {
    let lines = verdict(Run::default()).unwrap();
    assert_eq!(
        lines,
        [format!(
            "sophia_qemu_controlled_repaint_verdict schema=1 status=RETAINED size=400x300 checksum={} shifts=20 samples=1:20,2:20 renderers=1:11,2:12 recoveries=0",
            checksum()
        )]
    );
}

#[test]
fn a_changed_region_on_either_head_is_lost() {
    refused(
        Run {
            region: Box::new(|head, shift| (head == 2 && shift == 7).then_some((1, 0))),
            ..Run::default()
        },
        "LOST",
        "head=2 frame=9 offset_x=64 checksum=1 nonzero_rgb_pixels=0",
    );
    refused(
        Run {
            region: Box::new(|head, shift| (head == 1 && shift == 0).then_some((5, 120_000))),
            ..Run::default()
        },
        "LOST",
        "head=1 frame=2 offset_x=8 checksum=5",
    );
}

#[test]
fn a_mismatch_after_the_coverage_window_is_still_lost() {
    refused(
        Run {
            late: Some((30.0, 9)),
            ..Run::default()
        },
        "LOST",
        "head=2 frame=40",
    );
}

#[test]
fn a_mismatch_is_lost_even_when_coverage_is_insufficient() {
    refused(
        Run {
            shifts: 3,
            region: Box::new(|head, shift| (head == 1 && shift == 2).then_some((3, 0))),
            ..Run::default()
        },
        "LOST",
        "head=1",
    );
}

#[test]
fn never_ready_is_unready_and_reproduces_only_in_the_153_shape() {
    refused(
        Run {
            ready: false,
            ..Run::default()
        },
        "UNREADY",
        "reproduced=yes",
    );
    let mut run = Run {
        ready: false,
        ..Run::default()
    };
    run.presents = 1;
    let log = run
        .build()
        .replace("checksum=7", &format!("checksum={}", checksum()));
    let line = verify(&log, controlled_repaint()).unwrap_err();
    assert!(line.contains("status=UNREADY reproduced=no"), "{line}");
}

#[test]
fn omitted_coverage_is_insufficient_never_retained() {
    refused(
        Run {
            stops_after: Some((2, 12)),
            ..Run::default()
        },
        "INSUFFICIENT",
        "head 2 presented 12 of 20 committed shifts",
    );
    refused(
        Run {
            shifts: 10,
            ..Run::default()
        },
        "INSUFFICIENT",
        "samples=11",
    );
    refused(
        Run {
            first_shift_s: 3.0,
            ..Run::default()
        },
        "INSUFFICIENT",
        "max_gap_us=3050000",
    );
}

#[test]
fn a_wrong_offset_or_crop_is_refused() {
    // An earlier shift's crop after a later one.
    refused(
        Run {
            offset: Box::new(|head, shift| (head == 2 && shift == 5).then_some(16)),
            ..Run::default()
        },
        "INVALID",
        "head 2: crop runs",
    );
    // A crop no committed shift names.
    refused(
        Run {
            offset: Box::new(|head, shift| (head == 2 && shift == 3).then_some(33)),
            ..Run::default()
        },
        "INVALID",
        "head 2: crop runs",
    );
    // A stale frame at the previous crop where shift 6 was committed: the
    // head never showed that shift, which is missing coverage, not retention.
    refused(
        Run {
            offset: Box::new(|head, shift| (head == 1 && shift == 5).then_some(40)),
            ..Run::default()
        },
        "INSUFFICIENT",
        "head 1 presented 19 of 20 committed shifts, missing 6",
    );
}

#[test]
fn the_importers_must_be_two_renderers_of_one_owner() {
    refused(
        Run {
            renderer: Box::new(|_| 11),
            ..Run::default()
        },
        "INVALID",
        "not one importer per head",
    );
    refused(
        Run {
            owner: Box::new(|head| head),
            ..Run::default()
        },
        "INVALID",
        "head 2: import records name another renderer, owner or output",
    );
    refused(
        Run {
            import_heads: vec![1],
            ..Run::default()
        },
        "INVALID",
        "head 2: no per-head import record",
    );
    let log = Run::default()
        .build()
        .replacen("imports=1 ", "imports=0 ", 1);
    let line = verify(&log, controlled_repaint()).unwrap_err();
    assert!(
        line.contains("head 1: its renderer never imported the image"),
        "{line}"
    );
}

#[test]
fn a_second_client_present_is_refused() {
    refused(
        Run {
            presents: 2,
            ..Run::default()
        },
        "INVALID",
        "exactly one retired client Present",
    );
}

#[test]
fn the_window_must_be_presented_on_both_heads_of_one_output() {
    let log = Run::default()
        .build()
        .lines()
        .filter(|line| !line.contains(" head=2 "))
        .map(|line| format!("{line}\n"))
        .collect::<String>();
    let line = verify(&log, controlled_repaint()).unwrap_err();
    assert!(line.contains("presented on 1 heads"), "{line}");
}

#[test]
fn every_shift_is_bound_to_session_acceptance() {
    refused(
        Run {
            outcome: "Rejected",
            ..Run::default()
        },
        "INVALID",
        "was not committed",
    );
    // A WM shift Session never accepted as an action.
    let log = Run::default().build().replacen(
        "sophia_shell_action_policy schema=1 policy_connection_epoch=1 activation_serial=20 ",
        "sophia_shell_policy_note schema=1 policy_connection_epoch=1 activation_serial=20 ",
        1,
    );
    let line = verify(&log, controlled_repaint()).unwrap_err();
    assert!(line.contains("20 WM shifts against 19 committed"), "{line}");
    // A settlement of another transaction.
    let log = Run::default().build().replacen(
        "sophia_live_wm_chrome schema=2 status=settled transaction=105 ",
        "sophia_live_wm_chrome schema=2 status=settled transaction=905 ",
        1,
    );
    let line = verify(&log, controlled_repaint()).unwrap_err();
    assert!(
        line.contains("transaction 105: no single committed settlement"),
        "{line}"
    );
    // A proposal that names another activation.
    let log = Run::default().build().replacen(
        "cause=1 activation_serial=4 shift=1",
        "cause=1 activation_serial=9 shift=1",
        1,
    );
    let line = verify(&log, controlled_repaint()).unwrap_err();
    assert!(
        line.contains("transaction 103: the WM proposal is not shift 4"),
        "{line}"
    );
}

#[test]
fn the_fixture_must_be_this_mode_unchanged_and_fully_pressed() {
    refused(
        Run {
            hotplug: 1,
            ..Run::default()
        },
        "INVALID",
        "drm_hotplug=1",
    );
    let log = Run::default()
        .build()
        .replace("index=13 key=f9", "index=14 key=f9");
    let line = verify(&log, controlled_repaint()).unwrap_err();
    assert!(
        line.contains("cadence records do not follow the barrier"),
        "{line}"
    );
    let log = Run::default()
        .build()
        .replace("wm=true client=dri3", "wm=false client=dri3");
    let line = verify(&log, controlled_repaint()).unwrap_err();
    assert!(line.contains("status=INVALID"), "{line}");
    refused(
        Run {
            extra: "sophia_qemu_unplug schema=1 status=failed reason=cadence\n".to_owned(),
            ..Run::default()
        },
        "INVALID",
        "reason=cadence",
    );
}

#[test]
fn a_changed_owner_or_unclean_end_is_refused() {
    refused(
        Run { extra: "sophia_live_native_owner schema=1 status=closed epoch=1 reason=recovery settled=true submissions=1 retirements=1 submit_failures=0 retire_failures=0 in_flight=0 cleanup_pending=0 settlement_failures=0\n".to_owned(), ..Run::default() },
        "INVALID",
        "native owner records",
    );
    let log = Run::default().build().replace("qemu_exit=0", "qemu_exit=1");
    let line = verify(&log, controlled_repaint()).unwrap_err();
    assert!(line.contains("guest end"), "{line}");
}

#[test]
fn recoveries_are_context_and_never_turn_a_mismatch_into_retained() {
    let recovery =
        "sophia_renderer_worker schema=3 status=stall_recovered output=1 request=4 age_ms=1200\n";
    refused(
        Run {
            extra: recovery.to_owned(),
            region: Box::new(|head, shift| (head == 1 && shift == 3).then_some((4, 0))),
            ..Run::default()
        },
        "LOST",
        "recoveries=1",
    );
    let lines = verdict(Run {
        extra: recovery.to_owned(),
        ..Run::default()
    })
    .unwrap();
    assert!(lines[0].ends_with("recoveries=1"), "{lines:?}");
}
