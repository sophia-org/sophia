//! The two-probe output-unplug proof (t306 qualification) on logs built from
//! the emitters' own formats: each passing run, and each way a returned head,
//! its owner map, its repaint or its routed key must be refused.

#[path = "../src"]
mod source {
    pub mod output_unplug;
}

use source::output_unplug::{Mode, probe_frame_checksum, verify};

const PROBES: [(&str, u32, u32, &str); 2] =
    [("1", 400, 300, "Virtual-1"), ("2", 320, 200, "Virtual-2")];

fn checksum(index: usize) -> u64 {
    probe_frame_checksum(PROBES[index].1, PROBES[index].2)
}

/// One presented region of probe `index` on (output, head), queued as frame
/// `frame` at scene generation `generation`, at crop x `x`, with `pixels`.
fn region(
    output: u32,
    head: u32,
    frame: u32,
    generation: u32,
    index: usize,
    x: u32,
    pixels: u64,
) -> String {
    let (_, w, h, _) = PROBES[index];
    format!(
        "sophia_live_head_composition_queue schema=1 status=queued output={output} head={head} frame={frame} scene_generation={generation} target_generation=1\n\
sophia_native_composition_region_frame schema=1 status=read output={output} head={head} scene_generation={generation} layer=0 source_stage=renderer_image target={w}x{h}_{x}_0 region_pixels={} nonzero_rgb_pixels={} checksum={pixels}\n\
sophia_live_native_head_page_flip schema=2 status=retired output={output} head={head} submission={frame} frame={frame}\n",
        w * h,
        w * h,
    )
}

fn owner(epoch: u32, reason: &str, heads: &[(u32, u32, &str)]) -> String {
    let mut text =
        format!("sophia_live_native_owner schema=1 status=opened epoch={epoch} reason={reason}\n");
    for (output, head, connector) in heads {
        text.push_str(&format!(
            "sophia_live_native_owner_head schema=1 status=mapped epoch={epoch} output={output} head={head} connector={connector} connector_id={}\n",
            30 + head
        ));
    }
    text
}

fn closed(epoch: u32) -> String {
    format!(
        "sophia_live_native_owner schema=1 status=closed epoch={epoch} reason=topology_rebuild settled=true\n"
    )
}

/// The fixture as one declared run of `mode`; `edit` may rewrite any part.
struct Run {
    mode: &'static str,
    after_return: Box<dyn Fn(u32, u32) -> String>,
    baseline_extra: String,
    returned_owner: Vec<(u32, u32, &'static str)>,
    key: String,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            mode: "one-return",
            after_return: Box::new(|output, head| {
                let index = usize::from(output != 1);
                region(output, head, 20 + head, 60, index, 8, checksum(index))
            }),
            baseline_extra: String::new(),
            returned_owner: vec![(1, 11, "Virtual-1"), (2, 12, "Virtual-2")],
            key: "dri3_layout probe=2 stage=key keycode=56 synthetic=0\n".to_owned(),
        }
    }
}

impl Run {
    fn build(&self) -> String {
        let all = self.mode == "all-return";
        let mut log = format!(
            "sophia_qemu_unplug schema=1 status=starting isolation=headless control=none host_drm=none host_vt=none gpu=virtio-gpu mode={0} single_card=1\n\
sophia_qemu_guest schema=1 status=booting gpu=virtio-gpu scenario=output-unplug\n\
sophia_qemu_topology schema=1 status=observed requested_heads=2 connectors=2 connected=2\n\
sophia_qemu_unplug schema=1 status=running mode={0} wm=true client=dri3 probes=2\n",
            self.mode
        );
        log.push_str(&owner(
            1,
            "startup",
            &[(1, 1, "Virtual-1"), (2, 2, "Virtual-2")],
        ));
        log.push_str("sophia_live_session_input_device schema=1 status=added device=257 keyboard=true pointer=false touch=false virtual=true source=udev\n");
        log.push_str("sophia_live_session_startup schema=2 status=ready elapsed_msec=900 surface=true visual_detail=true presented=true outputs_ready=2/2 recovery_attempts=0\n");
        for (id, w, h, connector) in PROBES {
            log.push_str(&format!("sophia_qemu_probe schema=1 status=declared probe={id} geometry={w}x{h} connector={connector}\n"));
        }
        for (index, transaction) in [(0, 26), (1, 27)] {
            let (_, w, h, _) = PROBES[index];
            log.push_str(&format!(
                "sophia_live_session_present schema=2 status=retired transaction={transaction} surface={} source={w}x{h} target={w}x{h}_0_0 clip={w}x{h}_0_0 unit_scale=true ust=1 msc=1\n",
                2097153 + index
            ));
        }
        log.push_str(&region(1, 1, 3, 26, 0, 0, checksum(0)));
        log.push_str(&region(2, 2, 3, 27, 1, 0, checksum(1)));
        log.push_str(&self.baseline_extra);
        log.push_str(
            "dri3_layout probe=1 stage=focus state=in source=event mode=0 detail=3 synthetic=0\n",
        );
        log.push_str(
            "dri3_layout probe=1 stage=focus state=out source=event mode=0 detail=3 synthetic=0\n",
        );
        log.push_str(
            "dri3_layout probe=2 stage=focus state=in source=event mode=0 detail=3 synthetic=0\n",
        );
        log.push_str(
            "sophia_qemu_unplug schema=1 status=static_barrier present=retired client=holding\n",
        );
        let consoles: &[u32] = if all { &[0, 1] } else { &[1] };
        for console in consoles {
            log.push_str(&format!("sophia_qemu_unplug schema=1 status=sending action=off target=Console_{console}\nsophia_qemu_unplug schema=1 status=sent action=off target=Console_{console}\n"));
        }
        log.push_str(&closed(1));
        if all {
            log.push_str("sophia_qemu_unplug schema=1 status=connectors connectors=2 connected=0 sample=20\n");
            log.push_str("sophia_live_output_topology schema=1 status=unavailable transition=1 retry_msec=250 error=none\n");
        } else {
            log.push_str(&owner(2, "topology_rebuild", &[(1, 1, "Virtual-1")]));
            log.push_str("sophia_live_output_topology schema=1 status=quiesced transition=1 outcome=drained abandoned_scanouts=0\n");
            log.push_str("sophia_live_output_topology schema=1 status=published transition=1 topology_epoch=2 generation=2 outputs=1 changed=true restored_images=2 policy_required=false input=quarantined\n");
            log.push_str("sophia_live_output_topology schema=1 status=settled transition=1 retirements=1 input=enabled\n");
            log.push_str(&closed(2));
        }
        for console in consoles {
            log.push_str(&format!("sophia_qemu_unplug schema=1 status=sending action=on target=Console_{console}\nsophia_qemu_unplug schema=1 status=sent action=on target=Console_{console}\n"));
        }
        log.push_str(&owner(3, "topology_rebuild", &self.returned_owner));
        log.push_str("sophia_live_output_topology schema=1 status=quiesced transition=2 outcome=drained abandoned_scanouts=0\n");
        log.push_str("sophia_live_output_topology schema=1 status=published transition=2 topology_epoch=3 generation=3 outputs=2 changed=true restored_images=2 policy_required=false input=quarantined\n");
        log.push_str("sophia_live_output_topology schema=1 status=settled transition=2 retirements=2 input=enabled\n");
        log.push_str("sophia_qemu_unplug schema=1 status=repaint_sending key=f9\n");
        log.push_str("sophia_qemu_wm_hold schema=1 status=proposed transaction=100 request_id=50 cause=1 activation_serial=1 shift=1 offset_x=8 placements=2\n");
        log.push_str("sophia_shell_action_policy schema=1 policy_connection_epoch=1 activation_serial=1 action=1 transaction=100 request_id=50 indicator_generation=0 outcome=Committed target_output=0 target_generation=0\n");
        log.push_str("sophia_live_wm_chrome schema=2 status=settled transaction=100 request_id=50 scene_generation=60 outcome=Committed\n");
        log.push_str("sophia_qemu_unplug schema=1 status=repaint_sent result=completed\n");
        for (output, head, _) in &self.returned_owner {
            log.push_str(&(self.after_return)(*output, *head));
        }
        log.push_str("sophia_qemu_unplug schema=1 status=key_sending phase=returned key=b\n");
        log.push_str("sophia_live_session_input_device schema=1 status=key_observed device=257\n");
        log.push_str(&self.key);
        log.push_str(
            "sophia_qemu_unplug schema=1 status=key_sent phase=returned result=completed\n",
        );
        log.push_str(&format!(
            "sophia_qemu_unplug schema=1 status=uevents drm_hotplug={} input_remove=0 input_add=0\n",
            if all { 4 } else { 2 }
        ));
        log.push_str("sophia_live_session schema=7 status=bounded_complete display=:181 elapsed_msec=40000\nsophia_qemu_guest schema=1 status=complete scenario=output-unplug\nsophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0\n");
        log
    }

    fn mode(&self) -> Mode {
        Mode::parse(self.mode).unwrap()
    }
}

fn refused(run: &Run, log: String, reason: &str) {
    match verify(&log, run.mode()) {
        Err(error) => assert!(error.contains(reason), "expected {reason:?}, got {error}"),
        Ok(lines) => panic!("expected {reason:?}, got a pass: {lines:?}"),
    }
}

#[test]
fn each_returned_head_presents_its_own_probe_after_the_repaint_and_the_key_routes() {
    for mode in ["one-return", "all-return"] {
        let run = Run {
            mode,
            ..Run::default()
        };
        let lines = verify(&run.build(), run.mode()).unwrap();
        let text = lines.join("\n");
        assert!(
            text.contains("status=probe_baselines probes=1@Virtual-1,2@Virtual-2"),
            "{mode}: {text}"
        );
        assert!(
            text.contains(
                "status=returned_heads_presented owner=3 probes=1@Virtual-1=11,2@Virtual-2=12"
            ),
            "{mode}: {text}"
        );
        assert!(
            text.contains("status=returned_input_routed device=257 probe=2 keycode=56"),
            "{mode}: {text}"
        );
    }
}

#[test]
fn a_surviving_or_pre_loss_head_never_stands_for_a_returned_one() {
    let run = Run::default();
    // The returned owner maps Virtual-2 to head 12; a region on the old head 2 (same output) proves nothing.
    let log = run.build().replace(
        &region(2, 12, 32, 60, 1, 8, checksum(1)),
        &region(2, 2, 32, 60, 1, 8, checksum(1)),
    );
    refused(&run, log, "probe 2: not presented");
    // The region on the surviving head of probe 1, not its own.
    let surviving = Run {
        after_return: Box::new(|output, _head| {
            if output == 1 {
                region(1, 11, 31, 60, 0, 8, checksum(0)) + &region(1, 11, 33, 60, 1, 8, checksum(1))
            } else {
                String::new()
            }
        }),
        ..Run::default()
    };
    refused(&surviving, surviving.build(), "probe 2: not presented");
    // The returned owner reuses the startup owner's output and head numbers. Shifted regions
    // under those numbers before the return (inside an older owner) never stand for it.
    let reused = Run {
        returned_owner: vec![(1, 1, "Virtual-1"), (2, 2, "Virtual-2")],
        baseline_extra: region(1, 1, 31, 60, 0, 8, checksum(0))
            + &region(2, 2, 32, 60, 1, 8, checksum(1)),
        after_return: Box::new(|_, _| String::new()),
        ..Run::default()
    };
    refused(
        &reused,
        reused.build(),
        "probe 1: not presented at 400x300_8_0 on returned head 1 of owner 3",
    );
    // With the same reused numbers, regions inside the returned owner pass.
    let fresh = Run {
        returned_owner: vec![(1, 1, "Virtual-1"), (2, 2, "Virtual-2")],
        ..Run::default()
    };
    verify(&fresh.build(), fresh.mode()).unwrap();
}

#[test]
fn every_returned_head_needs_its_own_region_and_frame() {
    let missing = Run {
        after_return: Box::new(|output, head| {
            if output == 1 {
                region(1, head, 31, 60, 0, 8, checksum(0))
            } else {
                String::new()
            }
        }),
        ..Run::default()
    };
    refused(
        &missing,
        missing.build(),
        "probe 2: not presented at 320x200_8_0 on returned head 12",
    );
    let wrong = Run {
        after_return: Box::new(|output, head| {
            let index = usize::from(output != 1);
            region(
                output,
                head,
                20 + head,
                60,
                index,
                8,
                if index == 1 { 7 } else { checksum(0) },
            )
        }),
        ..Run::default()
    };
    refused(
        &wrong,
        wrong.build(),
        "probe 2: returned head 12 shows checksum=7",
    );
    let unshifted = Run {
        after_return: Box::new(|output, head| {
            let index = usize::from(output != 1);
            region(output, head, 20 + head, 60, index, 0, checksum(index))
        }),
        ..Run::default()
    };
    refused(
        &unshifted,
        unshifted.build(),
        "not presented at 400x300_8_0",
    );
    let stale = Run {
        after_return: Box::new(|output, head| {
            let index = usize::from(output != 1);
            region(output, head, 20 + head, 59, index, 8, checksum(index))
        }),
        ..Run::default()
    };
    refused(&stale, stale.build(), "not presented at 400x300_8_0");
    // A region never retired is not presented.
    let run = Run::default();
    refused(
        &run,
        run.build()
            .replace("submission=32 frame=32", "submission=32 frame=99"),
        "probe 2: not presented",
    );
}

#[test]
fn an_incomplete_ambiguous_or_foreign_owner_map_is_refused() {
    let run = Run::default();
    let unavailable = run.build().replace(
        "sophia_live_native_owner_head schema=1 status=mapped epoch=3 output=2 head=12 connector=Virtual-2 connector_id=42\n",
        "sophia_live_native_owner_head schema=1 status=mapped epoch=3 output=2 head=12 connector=Virtual-2 connector_id=42\nsophia_live_native_owner_head schema=1 status=unavailable epoch=3\n",
    );
    refused(&run, unavailable, "owner 3 has no complete head map");
    let duplicate = Run {
        returned_owner: vec![(1, 11, "Virtual-1"), (2, 12, "Virtual-1")],
        ..Run::default()
    };
    refused(
        &duplicate,
        duplicate.build(),
        "maps a head or connector twice",
    );
    let missing = Run {
        returned_owner: vec![(1, 11, "Virtual-1")],
        ..Run::default()
    };
    refused(
        &missing,
        missing.build(),
        "maps no head for connector Virtual-2",
    );
    let foreign = run.build().replace(
        "status=mapped epoch=3 output=2",
        "status=mapped epoch=2 output=2",
    );
    refused(&run, foreign, "names no open owner");
    let none = run.build().replace(
        "sophia_live_native_owner_head schema=1 status=mapped epoch=3 output=1 head=11 connector=Virtual-1 connector_id=41\nsophia_live_native_owner_head schema=1 status=mapped epoch=3 output=2 head=12 connector=Virtual-2 connector_id=42\n",
        "",
    );
    refused(&run, none, "owner 3 has no complete head map");
}

#[test]
fn the_return_owner_must_hold_its_publication_settlement_and_repaint() {
    let run = Run::default();
    let early_close = run.build().replace(
        "sophia_qemu_unplug schema=1 status=repaint_sending",
        &format!(
            "{}sophia_qemu_unplug schema=1 status=repaint_sending",
            closed(3)
        ),
    );
    refused(
        &run,
        early_close,
        "the repaint settled outside the accepted owner",
    );
    let no_repaint = run.build().replace(
        "sophia_qemu_unplug schema=1 status=repaint_sending key=f9\n",
        "",
    );
    refused(&run, no_repaint, "no fixture repaint after the return");
    let rejected = run.build().replace(
        "transaction=100 request_id=50 indicator_generation=0 outcome=Committed",
        "transaction=100 request_id=50 indicator_generation=0 outcome=Rejected",
    );
    refused(
        &run,
        rejected,
        "expected exactly one committed hold-shift action, found 0",
    );
    let second_present = run.build().replace(
        "sophia_qemu_unplug schema=1 status=repaint_sent",
        "sophia_live_session_present schema=2 status=retired transaction=40 surface=2097153 source=400x300 target=400x300_8_0 clip=x unit_scale=true ust=2 msc=2\nsophia_qemu_unplug schema=1 status=repaint_sent",
    );
    refused(&run, second_present, "exactly two client Presents");
}

#[test]
fn each_probe_baseline_is_judged_alone() {
    // Probe 1's earlier region disagrees; probe 2's good pixels do not hide it.
    let run = Run {
        baseline_extra: region(1, 1, 4, 26, 0, 0, 9),
        ..Run::default()
    };
    refused(&run, run.build(), "probe 1: unstable_baseline");
    let wrong = Run::default();
    refused(
        &wrong,
        wrong
            .build()
            .replacen(&format!("checksum={}\n", checksum(1)), "checksum=5\n", 1),
        "probe 2: its first presented region is not its frame",
    );
}

#[test]
fn the_returned_key_must_reach_the_focused_probe_from_an_admitted_keyboard() {
    let run = Run::default();
    refused(
        &run,
        run.build().replace(
            "sophia_qemu_unplug schema=1 status=key_sending phase=returned key=b\n",
            "",
        ),
        "expected one returned-phase key",
    );
    let other = Run {
        key: "dri3_layout probe=1 stage=key keycode=56 synthetic=0\n".to_owned(),
        ..Run::default()
    };
    refused(
        &other,
        other.build(),
        "probe 1 received a key; probe 2 holds the focus",
    );
    let none = Run {
        key: String::new(),
        ..Run::default()
    };
    refused(
        &none,
        none.build(),
        "probe 2 reported no key after the return",
    );
    let wrong = Run {
        key: "dri3_layout probe=2 stage=key keycode=38 synthetic=0\n".to_owned(),
        ..Run::default()
    };
    refused(&wrong, wrong.build(), "expected one keycode 56");
    refused(
        &run,
        run.build().replace(
            "status=key_sent phase=returned result=completed",
            "status=key_sent phase=returned result=failed",
        ),
        "was not sent",
    );
    refused(
        &run,
        run.build().replace(
            "status=key_observed device=257",
            "status=key_observed device=999",
        ),
        "not an admitted keyboard",
    );
    // A key sent before the repaint does not count as returned input.
    let early = run.build().replace("sophia_qemu_unplug schema=1 status=key_sending phase=returned key=b\n", "")
        .replace("sophia_qemu_unplug schema=1 status=repaint_sending", "sophia_qemu_unplug schema=1 status=key_sending phase=returned key=b\nsophia_qemu_unplug schema=1 status=repaint_sending");
    refused(
        &run,
        early,
        "expected one returned-phase key after the return",
    );
}

/// The keyboard-only run with two probes: input-return's ordered chain at the
/// focused probe (probe 2), then a fixture repaint inside the one owner and each
/// probe presented at the shifted crop on its own head.
fn input_return(after: &str, chain_probe: &str, owners_extra: &str) -> String {
    let chain = "\
dri3_layout probe=P stage=focus state=in source=event mode=0 detail=3 synthetic=0
dri3_layout probe=P stage=holding hold_ms=35000
dri3_layout probe=P stage=focus state=in source=query
sophia_qemu_unplug schema=1 status=input_baseline_ready device=257
sophia_qemu_unplug schema=1 status=key_sending phase=baseline key=a
sophia_live_session_input_device schema=1 status=key_observed device=257
dri3_layout probe=P stage=key keycode=38 synthetic=0
sophia_qemu_unplug schema=1 status=input_baseline device=257 routed=yes
sophia_qemu_unplug schema=1 status=key_sent phase=baseline result=completed
sophia_qemu_unplug schema=1 status=sending action=off target=virtio2
sophia_qemu_unplug schema=1 status=sent action=off target=virtio2
sophia_live_session_input_device schema=1 status=removed device=257 released=0
sophia_qemu_unplug schema=1 status=input_removed device=257
sophia_qemu_unplug schema=1 status=sending action=on target=virtio2
sophia_qemu_unplug schema=1 status=sent action=on target=virtio2
sophia_live_session_input_device schema=1 status=added device=262 keyboard=true pointer=false touch=false virtual=true source=udev
sophia_qemu_unplug schema=1 status=input_return_ready device=262
sophia_qemu_unplug schema=1 status=key_sending phase=return key=b
dri3_layout probe=P stage=key keycode=56 synthetic=0
sophia_live_session_input_device schema=1 status=key_observed device=262
sophia_qemu_unplug schema=1 status=key_sent phase=return result=completed
sophia_qemu_unplug schema=1 status=input_return_routed device=262
"
    .replace("probe=P", &format!("probe={chain_probe}"));
    let mut log = String::from("\
sophia_qemu_unplug schema=1 status=starting isolation=headless control=none host_drm=none host_vt=none gpu=virtio-gpu mode=input-return single_card=1
sophia_qemu_guest schema=1 status=booting gpu=virtio-gpu scenario=output-unplug
sophia_qemu_topology schema=1 status=observed requested_heads=2 connectors=2 connected=2
sophia_qemu_unplug schema=1 status=running mode=input-return wm=true client=dri3 probes=2
");
    log.push_str(&owner(
        1,
        "startup",
        &[(1, 1, "Virtual-1"), (2, 2, "Virtual-2")],
    ));
    log.push_str("sophia_live_session_input_device schema=1 status=added device=256 keyboard=true pointer=false touch=false virtual=false source=udev\n");
    log.push_str("sophia_live_session_input_device schema=1 status=added device=257 keyboard=true pointer=false touch=false virtual=true source=udev\n");
    log.push_str("sophia_live_session_input_device schema=1 status=added device=258 keyboard=false pointer=true touch=false virtual=true source=udev\n");
    log.push_str("sophia_live_session_startup schema=2 status=ready elapsed_msec=900 surface=true visual_detail=true presented=true outputs_ready=2/2 recovery_attempts=0\n");
    for (id, w, h, connector) in PROBES {
        log.push_str(&format!("sophia_qemu_probe schema=1 status=declared probe={id} geometry={w}x{h} connector={connector}\n"));
    }
    for (index, transaction) in [(0, 26), (1, 27)] {
        let (_, w, h, _) = PROBES[index];
        log.push_str(&format!(
            "sophia_live_session_present schema=2 status=retired transaction={transaction} surface={} source={w}x{h} target={w}x{h}_0_0 clip={w}x{h}_0_0 unit_scale=true ust=1 msc=1\n",
            2097153 + index
        ));
    }
    log.push_str(&region(1, 1, 3, 26, 0, 0, checksum(0)));
    log.push_str(&region(2, 2, 3, 27, 1, 0, checksum(1)));
    log.push_str(
        "sophia_qemu_unplug schema=1 status=static_barrier present=retired client=holding\n",
    );
    log.push_str(&chain);
    log.push_str(owners_extra);
    log.push_str("sophia_qemu_unplug schema=1 status=repaint_sending key=f9\n");
    log.push_str("sophia_qemu_wm_hold schema=1 status=proposed transaction=100 request_id=50 cause=1 activation_serial=1 shift=1 offset_x=8 placements=2\n");
    log.push_str("sophia_shell_action_policy schema=1 policy_connection_epoch=1 activation_serial=1 action=1 transaction=100 request_id=50 indicator_generation=0 outcome=Committed target_output=0 target_generation=0\n");
    log.push_str("sophia_live_wm_chrome schema=2 status=settled transaction=100 request_id=50 scene_generation=60 outcome=Committed\n");
    log.push_str("sophia_qemu_unplug schema=1 status=repaint_sent result=completed\n");
    log.push_str(after);
    log.push_str(
        "sophia_qemu_unplug schema=1 status=uevents drm_hotplug=0 input_remove=2 input_add=2\n",
    );
    log.push_str("sophia_live_session schema=7 status=bounded_complete display=:181 elapsed_msec=40000\nsophia_qemu_guest schema=1 status=complete scenario=output-unplug\nsophia_qemu_unplug schema=1 status=guest_exited qemu_exit=0\n");
    log
}

fn input_after() -> String {
    region(1, 1, 31, 60, 0, 8, checksum(0)) + &region(2, 2, 32, 60, 1, 8, checksum(1))
}

#[test]
fn the_keyboard_only_run_routes_at_the_focused_probe_and_repaints_both_heads() {
    let mode = Mode::parse("input-return").unwrap();
    let lines = verify(&input_return(&input_after(), "2", ""), mode)
        .unwrap()
        .join("\n");
    assert!(
        lines.contains("status=input_routed baseline_device=257 return_device=262"),
        "{lines}"
    );
    assert!(
        lines
            .contains("status=returned_heads_presented owner=1 probes=1@Virtual-1=1,2@Virtual-2=2"),
        "{lines}"
    );
}

#[test]
fn the_keyboard_only_run_needs_post_return_pixels_one_owner_and_one_recipient() {
    let mode = Mode::parse("input-return").unwrap();
    let expect = |log: String, reason: &str| match verify(&log, mode) {
        Err(error) => assert!(error.contains(reason), "expected {reason:?}, got {error}"),
        Ok(lines) => panic!("expected {reason:?}, got a pass: {lines:?}"),
    };
    // Unchanged outputs are not pixel evidence: no region after the repaint.
    expect(
        input_return("", "2", ""),
        "probe 1: not presented at 400x300_8_0",
    );
    // Only one head repainted.
    expect(
        input_return(&region(1, 1, 31, 60, 0, 8, checksum(0)), "2", ""),
        "probe 2: not presented",
    );
    // An owner change in a run whose outputs were to stay.
    expect(
        input_return(
            &input_after(),
            "2",
            &(closed(1)
                + &owner(
                    2,
                    "topology_rebuild",
                    &[(1, 1, "Virtual-1"), (2, 2, "Virtual-2")],
                )),
        ),
        "2 owners opened",
    );
    // The other probe received a key.
    let log = input_return(&input_after(), "2", "").replace(
        "sophia_qemu_unplug schema=1 status=key_sent phase=return result=completed",
        "dri3_layout probe=1 stage=key keycode=56 synthetic=0\nsophia_qemu_unplug schema=1 status=key_sent phase=return result=completed",
    );
    expect(log, "probe 1 received a key; probe 2 holds the focus");
}

fn all_return() -> String {
    Run {
        mode: "all-return",
        ..Run::default()
    }
    .build()
}

fn insert_after(log: &str, line: &str, insert: &str) -> String {
    let line = format!("{line}\n");
    assert_eq!(log.matches(&line).count(), 1, "{line}");
    log.replacen(&line, &format!("{line}{insert}"), 1)
}

fn insert_before(log: &str, line: &str, insert: &str) -> String {
    assert_eq!(log.matches(line).count(), 1, "{line}");
    log.replacen(line, &format!("{insert}{line}"), 1)
}

/// The combined run: the keyboard leaves before the heads and returns after them.
fn combined() -> String {
    let log = all_return().replace("mode=all-return", "mode=combined-return");
    let log = insert_before(
        &log,
        "sophia_qemu_unplug schema=1 status=sending action=off target=Console_0",
        "\
sophia_qemu_unplug schema=1 status=keyboard_sending action=off target=virtio2
sophia_qemu_unplug schema=1 status=keyboard_sent action=off target=virtio2
sophia_live_session_input_device schema=1 status=removed device=257 released=0
",
    );
    let log = insert_after(&log, "sophia_qemu_unplug schema=1 status=sent action=on target=Console_1", "\
sophia_qemu_unplug schema=1 status=keyboard_sending action=on target=virtio2
sophia_qemu_unplug schema=1 status=keyboard_sent action=on target=virtio2
sophia_live_session_input_device schema=1 status=added device=262 keyboard=true pointer=false touch=false virtual=true source=udev
");
    log.replace(
        "status=key_observed device=257",
        "status=key_observed device=262",
    )
    .replace("input_remove=0 input_add=0", "input_remove=1 input_add=1")
}

const SETTLED_RETURN: &str =
    "sophia_live_output_topology schema=1 status=settled transition=2 retirements=2 input=enabled";

/// The locked run: locked after the barrier; after the return settles the cover
/// names the lock and the returned topology, a locked-phase key is held by the
/// lock, then the right secret unlocks it.
fn locked(between: &str) -> String {
    let log = all_return().replace("mode=all-return", "mode=locked-return");
    let log = insert_after(
        &log,
        "sophia_qemu_unplug schema=1 status=static_barrier present=retired client=holding",
        "sophia_live_session_lock schema=1 status=locked epoch=1\n",
    );
    insert_after(
        &log,
        SETTLED_RETURN,
        &format!("{between}sophia_live_session_lock schema=1 status=unlocked epoch=1\n"),
    )
}

const LOCKED_PHASE: &str = "\
sophia_live_session_lock schema=1 status=covered epoch=1 topology_epoch=3 outputs=2 heads=2
sophia_qemu_unplug schema=1 status=key_sending phase=locked key=b
sophia_live_session_lock schema=1 status=key_held epoch=1 device=257
sophia_qemu_unplug schema=1 status=key_sent phase=locked result=completed
sophia_qemu_lock_input schema=1 status=sent source=qmp secret=right
";

#[test]
fn the_combined_run_routes_from_the_returned_keyboard() {
    let mode = Mode::parse("combined-return").unwrap();
    let lines = verify(&combined(), mode).unwrap().join("\n");
    assert!(
        lines.contains("status=returned_input_routed device=262 probe=2 keycode=56"),
        "{lines}"
    );
    assert!(
        lines.contains("status=returned_heads_presented owner=3"),
        "{lines}"
    );
}

#[test]
fn the_combined_run_refuses_an_old_identity_or_a_keyboard_out_of_order() {
    let mode = Mode::parse("combined-return").unwrap();
    let expect = |log: String, reason: &str| match verify(&log, mode) {
        Err(error) => assert!(error.contains(reason), "expected {reason:?}, got {error}"),
        Ok(lines) => panic!("expected {reason:?}, got a pass: {lines:?}"),
    };
    expect(
        combined().replace(
            "status=key_observed device=262",
            "status=key_observed device=257",
        ),
        "not an admitted keyboard",
    );
    expect(
        combined().replace(
            "sophia_live_session_input_device schema=1 status=removed device=257 released=0\n",
            "",
        ),
        "keyboard 257 was not removed",
    );
    expect(
        combined().replace(
            "status=keyboard_sending action=off target=virtio2\n",
            "status=keyboard_sending action=of target=virtio2\n",
        ),
        "expected exactly one keyboard_sending action=off",
    );
    expect(
        combined()
            .replace(
                "added device=262 keyboard=true pointer=false touch=false virtual=true",
                "added device=257 keyboard=true pointer=false touch=false virtual=true",
            )
            .replace(
                "status=key_observed device=262",
                "status=key_observed device=257",
            ),
        "kept the removed identity",
    );
    expect(
        combined().replace(
            "mode=combined-return wm=true client=dri3 probes=2",
            "mode=combined-return wm=true client=dri3",
        ),
        "runs only with the two-probe fixture",
    );
}

#[test]
fn the_locked_run_covers_every_returned_head_and_holds_its_key_before_the_unlock() {
    let mode = Mode::parse("locked-return").unwrap();
    let lines = verify(&locked(LOCKED_PHASE), mode).unwrap().join("\n");
    assert!(
        lines.contains("status=lock_covered epoch=1 topology_epoch=3 heads=2 held_device=257"),
        "{lines}"
    );
    assert!(
        lines.contains("status=returned_input_routed device=257 probe=2"),
        "{lines}"
    );
}

#[test]
fn the_locked_run_refuses_a_missing_wrong_or_late_cover_and_any_leaked_key() {
    let mode = Mode::parse("locked-return").unwrap();
    let expect = |log: String, reason: &str| match verify(&log, mode) {
        Err(error) => assert!(error.contains(reason), "expected {reason:?}, got {error}"),
        Ok(lines) => panic!("expected {reason:?}, got a pass: {lines:?}"),
    };
    let cover = "sophia_live_session_lock schema=1 status=covered epoch=1 topology_epoch=3 outputs=2 heads=2\n";
    expect(
        locked(&LOCKED_PHASE.replace(cover, "")),
        "no cover for lock 1 over the returned topology 3",
    );
    expect(
        locked(&LOCKED_PHASE.replace("topology_epoch=3", "topology_epoch=2")),
        "no cover for lock 1 over the returned topology 3",
    );
    expect(
        locked(&LOCKED_PHASE.replace("status=covered epoch=1", "status=covered epoch=2")),
        "no cover for lock 1",
    );
    expect(
        locked(&LOCKED_PHASE.replace("outputs=2 heads=2", "outputs=1 heads=1")),
        "with 2 heads",
    );
    // A cover only after the unlock is no cover.
    let late = locked(&LOCKED_PHASE.replace(cover, "")).replace(
        "sophia_live_session_lock schema=1 status=unlocked epoch=1\n",
        &format!("sophia_live_session_lock schema=1 status=unlocked epoch=1\n{cover}"),
    );
    expect(late, "no cover for lock 1");
    expect(
        locked(&LOCKED_PHASE.replace(
            "sophia_live_session_lock schema=1 status=key_held epoch=1 device=257\n",
            "",
        )),
        "the lock held no key",
    );
    // An earlier held observation for the same lock and device never stands for the post-return key.
    let earlier = locked(LOCKED_PHASE).replace(
        "sophia_live_session_lock schema=1 status=locked epoch=1\n",
        "sophia_live_session_lock schema=1 status=locked epoch=1\nsophia_live_session_lock schema=1 status=key_held epoch=1 device=257\n",
    );
    expect(earlier, "device 257 was already held by lock 1");
    expect(locked(&LOCKED_PHASE.replace("sophia_qemu_lock_input schema=1 status=sent source=qmp secret=right\n", "dri3_layout probe=2 stage=key keycode=56 synthetic=0\nsophia_qemu_lock_input schema=1 status=sent source=qmp secret=right\n")), "a probe received a key while locked");
    expect(locked(&LOCKED_PHASE.replace("sophia_qemu_lock_input schema=1 status=sent source=qmp secret=right\n", "dri3_layout probe=1 stage=key keycode=48 synthetic=0\nsophia_qemu_lock_input schema=1 status=sent source=qmp secret=right\n")), "a probe received a key while locked");
    expect(
        locked(&LOCKED_PHASE.replace(
            "sophia_qemu_lock_input schema=1 status=sent source=qmp secret=right\n",
            "",
        )),
        "no right secret",
    );
    expect(
        locked(&LOCKED_PHASE.replace(
            "status=key_sent phase=locked result=completed",
            "status=key_sent phase=locked result=failed",
        )),
        "was not sent before the unlock",
    );
    // No unlock at all.
    expect(
        locked(LOCKED_PHASE).replace(
            "sophia_live_session_lock schema=1 status=unlocked epoch=1\n",
            "",
        ),
        "was not unlocked",
    );
    // Locked only after the removal began.
    let late_lock = locked(LOCKED_PHASE).replace("sophia_live_session_lock schema=1 status=locked epoch=1\n", "").replace(
        "sophia_qemu_unplug schema=1 status=sent action=off target=Console_0\n",
        "sophia_qemu_unplug schema=1 status=sent action=off target=Console_0\nsophia_live_session_lock schema=1 status=locked epoch=1\n",
    );
    expect(
        late_lock,
        "did not lock after the barrier and before the removal",
    );
}
