use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::sync::Arc;

use sophia_session::diagnostics::{Capture, Retention, Store, capture_line, reduced_record};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(
            std::env::temp_dir().join(format!("sophia-diagnostics-{}-{nonce}", std::process::id())),
        )
    }
    fn store(&self) -> Store {
        Store::open(&self.0, Retention::default()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn live_owner_selection_failure_and_preservation_survive_new_sessions() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let first = store
        .begin("hagia", std::process::id(), "release_commit=test\n")
        .unwrap();
    assert!(store.begin("hagia", std::process::id(), "").is_err());
    let marker = store.mark(None, "window stopped responding").unwrap();
    assert_eq!(marker.session, first.id);
    store.finish(&first.id, Some(17)).unwrap();
    assert!(store.mark(None, "must select the ended session").is_err());
    assert_eq!(store.select(Some("latest")).unwrap().status, "failed");
    let kept = store.keep(&first.id).unwrap();
    let second = store
        .begin("hagia", std::process::id(), "release_commit=next\n")
        .unwrap();
    assert_eq!(store.select(None).unwrap().id, second.id);
    let result = store.inspect(&first.id, Some(&marker.id)).unwrap();
    assert!(result.manifest.contains("release_commit=test"));
    assert!(result.markers.contains("window stopped responding"));
    assert!(
        fs::read_to_string(kept.join("snapshot"))
            .unwrap()
            .contains("complete=true")
    );
    assert!(kept.join("SHA256SUMS").exists());
}

#[test]
fn dead_owner_and_pid_reuse_are_never_reported_as_live() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let record = store.begin("test", child.id(), "").unwrap();
    rustix::process::kill_process(
        rustix::process::Pid::from_raw(child.id() as i32).unwrap(),
        rustix::process::Signal::STOP,
    )
    .unwrap();
    assert!(store.mark(None, "owner is stopped").is_ok());
    child.kill().unwrap();
    child.wait().unwrap();
    assert_eq!(
        store.select(Some(&record.id)).unwrap().status,
        "interrupted"
    );
    let marker = store.mark(Some("latest"), "reported after crash").unwrap();
    assert_eq!(marker.session, record.id);
    let manifest = record.path.join("manifest");
    let content = fs::read_to_string(&manifest).unwrap();
    let content = content
        .lines()
        .filter(|l| !l.starts_with("owner_pid=") && !l.starts_with("owner_start_ticks="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        manifest,
        format!(
            "{content}\nowner_pid={}\nowner_start_ticks=0\n",
            std::process::id()
        ),
    )
    .unwrap();
    assert_eq!(
        store.select(Some(&record.id)).unwrap().status,
        "interrupted"
    );
}

#[test]
fn retention_keeps_active_and_preserved_evidence_but_not_unbounded_daily_copies() {
    let fixture = Fixture::new();
    let store = Store::open(
        &fixture.0,
        Retention {
            finished_sessions: 1,
            bytes: 1024 * 1024,
        },
    )
    .unwrap();
    let first = store.begin("hagia", std::process::id(), "").unwrap();
    store.finish(&first.id, Some(0)).unwrap();
    let kept = store.keep(&first.id).unwrap();
    let second = store.begin("hagia", std::process::id(), "").unwrap();
    store.finish(&second.id, Some(0)).unwrap();
    assert!(!first.path.exists());
    assert!(kept.exists());
    let live = store.begin("hagia", std::process::id(), "").unwrap();
    let tiny = Store::open(
        &fixture.0,
        Retention {
            finished_sessions: 0,
            bytes: 1,
        },
    )
    .unwrap();
    tiny.prune().unwrap();
    assert!(live.path.exists());
    assert!(!second.path.exists());
    assert!(tiny.preserved_bytes().unwrap() > 0);
}

#[test]
fn labels_and_files_do_not_allow_injection_or_follow_links() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let record = store.begin("hagia", std::process::id(), "").unwrap();
    assert!(store.mark(None, "injected\nstatus=passed").is_err());
    assert!(store.mark(None, &"é".repeat(129)).is_err());
    assert!(store.inspect("../outside", None).is_err());
    let outside = fixture.0.join("outside");
    fs::write(&outside, "unchanged").unwrap();
    fs::remove_file(record.path.join("markers.log")).unwrap();
    symlink(&outside, record.path.join("markers.log")).unwrap();
    assert!(store.mark(None, "must fail").is_err());
    assert_eq!(fs::read_to_string(&outside).unwrap(), "unchanged");
    fs::remove_file(record.path.join("markers.log")).unwrap();
    fs::hard_link(&outside, record.path.join("markers.log")).unwrap();
    assert!(store.mark(None, "must also fail").is_err());
    fs::set_permissions(&fixture.0, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(Store::open(&fixture.0, Retention::default()).is_err());
}

#[test]
fn concurrent_markers_remain_distinct_and_ambiguous_sessions_need_selection() {
    let fixture = Fixture::new();
    let store = Arc::new(fixture.store());
    let first = store.begin("one", std::process::id(), "").unwrap();
    let second = store.begin("two", std::process::id(), "").unwrap();
    assert!(store.mark(None, "ambiguous").is_err());
    let threads = (0..8)
        .map(|index| {
            let store = store.clone();
            let id = first.id.clone();
            std::thread::spawn(move || store.mark(Some(&id), &format!("marker {index}")).unwrap())
        })
        .collect::<Vec<_>>();
    let ids = threads
        .into_iter()
        .map(|thread| thread.join().unwrap().id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 8);
    assert!(
        store
            .inspect(&second.id, Some(ids.first().unwrap()))
            .is_err()
    );
    assert_eq!(
        store
            .inspect(&first.id, None)
            .unwrap()
            .markers
            .lines()
            .count(),
        8
    );
}

#[test]
fn capture_is_bounded_redacted_and_survives_without_completion_records() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let record = store.begin("capture", std::process::id(), "").unwrap();
    // Simulate a full rolling history without allocating a huge test buffer.
    for segment in 0..4 {
        let path = record.path.join(format!("events.{segment}.log"));
        let file = fs::File::create(&path).unwrap();
        file.set_len(15 * 1024 * 1024).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let capture = Capture::start(&record.path).unwrap();
    for sequence in 0..1700 {
        capture_line(&format!(
            "sophia_live_resource_sample schema=1 seq={sequence} rss_kib=123"
        ));
        if sequence % 100 == 0 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    capture_line(
        "sophia_live_desktop_profile schema=1 status=loaded generation=4 digest=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa title=private namespace=42 peer_pid=99",
    );
    capture_line(
        "sophia_session_profile schema=1 status=activated role=wm generation=5 digest=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    );
    capture_line(
        "sophia_config_reload schema=2 status=applied generation=6 digest=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
    );
    // Oversized input is counted instead of being allowed to allocate an unbounded queue entry.
    capture_line(&"x".repeat(5000));
    drop(capture);
    let inspection = store.inspect(&record.id, None).unwrap();
    assert!(inspection.identities.contains("generation=4"));
    assert!(inspection.identities.contains("generation=5"));
    assert!(inspection.identities.contains("generation=6"));
    assert!(!inspection.identities.contains("private"));
    assert!(!inspection.identities.contains("peer_pid"));
    let resources = inspection
        .events
        .iter()
        .filter(|line| line.contains("sophia_live_resource_sample "))
        .count();
    let discarded = inspection
        .health
        .lines()
        .find_map(|line| line.strip_prefix("discarded="))
        .unwrap()
        .parse::<usize>()
        .unwrap();
    assert!(resources > 0);
    assert_eq!(resources + discarded, 1701); // resources plus the oversized record
    assert!(inspection.health.contains("recording=stopped"));
    assert!(inspection.health.contains("rotated_bytes=15728640"));
    assert!(inspection.record.bytes <= 64 * 1024 * 1024);
    assert!(!inspection.health.contains("discarded=0\n"));
    assert!(!record.path.join("outcome").exists());
}

#[test]
fn payload_values_are_not_copied_from_session_records() {
    let record = reduced_record("sophia_event schema=1 title=123 namespace_id=456 xid=789 error=secret uri=https://example.invalid text=hello count=9 status=failed").unwrap();
    assert_eq!(record, "sophia_event schema=1 count=9 status=failed");
    assert!(reduced_record("arbitrary client stdout").is_none());
}

#[test]
fn interaction_records_distinguish_delivery_rejection_and_chrome_without_payloads() {
    let records = [
        "sophia_live_session_input_routing schema=1 key_observed_count=3 key_routed_count=0 key_no_focus_count=0 key_stale_focus_count=3 wm_action_count=0 pointer_button_count=2 pointer_routed_count=0 chrome_event_count=0 lease_wait_count=0 lease_rejected_count=2",
        "sophia_live_session_pointer_batch schema=1 observed_count=2 routed_count=0 suppressed_no_target_count=2 suppressed_policy_count=0",
        "sophia_live_session_pointer schema=8 status=button_suppressed reason=no_target count=2",
        "sophia_live_session_pointer schema=2 status=button_routed count=2",
        "sophia_live_session_input_pipeline schema=2 status=key_suppressed reason=no_focus",
        "sophia_live_input_lease schema=1 confirmed=2 rejected=1 released=1 stale=0",
        "sophia_live_input_lease schema=1 status=quarantined reason=release_timeout",
        "sophia_live_input_lease schema=2 status=refused reason=outside_scope",
        "sophia_live_input_lease schema=2 status=refused reason=target_evidence",
        "sophia_live_input_lease schema=2 status=refused reason=binding_timeout",
        "sophia_live_input_lease schema=1 status=release_deferred reason=capacity",
        "sophia_live_explicit_pointer_grab schema=2 status=rejected reason=anchor_admission",
        "sophia_live_explicit_pointer_grab schema=1 deferred=2 cancelled=1",
        "sophia_live_explicit_pointer_grab schema=1 prepared=1 activated=1 released=0 aborted=0 rejected=2",
        "sophia_live_compositor_chrome_set schema=1 status=composed generation=42 eligible_surfaces=2 frames=2 focused_frames=1 unfocused_frames=1 focus_rings=1 primitives=3 clearance=2",
        "sophia_live_session_present_feedback schema=1 kind=complete transaction=42 routed=true ust=234569038838 msc=19564390",
        "sophia_live_session_present_feedback schema=1 kind=idle transaction=42 routed=true",
    ];
    for record in records {
        let raw = format!(
            "{record} admission=123 namespace=456 xid=789 title=secret pointer_x=10 pointer_y=20 keycode=38 text=secret"
        );
        assert_eq!(reduced_record(&raw).as_deref(), Some(record));
    }
}

#[test]
fn policy_configuration_rejection_retains_bounded_missing_slots() {
    let record = "sophia_live_wm_configuration schema=2 status=rejected reason=unavailable_session_slot catalog_generation=17 missing_slot_count=2 missing_slots=2,7";
    assert_eq!(reduced_record(record).as_deref(), Some(record));
    for invalid in ["", "0", "7,", "-1", "65536", "seven"] {
        let raw = format!(
            "sophia_live_wm_configuration schema=2 status=rejected reason=unavailable_session_slot catalog_generation=17 missing_slot_count=2 missing_slots={invalid}"
        );
        assert_eq!(
            reduced_record(&raw).as_deref(),
            Some(
                "sophia_live_wm_configuration schema=2 status=rejected reason=unavailable_session_slot catalog_generation=17 missing_slot_count=2"
            )
        );
    }
}

#[test]
fn interaction_vocabulary_is_scoped_and_rejects_unbounded_values() {
    for record in [
        "sophia_other status=button_suppressed reason=no_target confirmed=2 prepared=1 frames=2 ust=123 msc=456",
        "sophia_live_input_lease confirmed=secret rejected=-1 released=18446744073709551616 stale=1.5",
        "sophia_live_explicit_pointer_grab prepared=secret status=secret reason=secret",
        "sophia_live_session_pointer status=secret reason=secret",
        "sophia_live_compositor_chrome_set frames=18446744073709551616",
    ] {
        assert_eq!(
            reduced_record(record).as_deref(),
            record.split_whitespace().next()
        );
    }
}

#[test]
fn native_shell_gate_outcomes_survive_sanitization_without_client_payloads() {
    for record in [
        "sophia_live_shell_gpu schema=1 status=granted grant_epoch=7",
        "sophia_live_shell_gpu schema=1 status=revoked grant_epoch=7",
        "sophia_live_shell_content schema=1 status=outputs facts_generation=3 outputs=2",
        "sophia_live_shell_content schema=1 status=presented output=2 candidate_generation=9 presentation_epoch=11 resident_bytes=24576 backing_bytes=24576",
        "sophia_live_shell_content schema=2 status=transport_failed stage=runtime",
    ] {
        assert_eq!(
            reduced_record(&format!(
                "{record} title=secret xid=42 payload=private device_path=/dev/dri/renderD128"
            ))
            .as_deref(),
            Some(record)
        );
    }
    for record in [
        "sophia_other status=granted outputs=2",
        "sophia_live_shell_gpu status=private",
        "sophia_live_shell_content status=private outputs=secret",
    ] {
        assert_eq!(
            reduced_record(record).as_deref(),
            record.split_whitespace().next()
        );
    }
}

#[test]
fn actual_atomic_test_outcomes_survive_sanitization_without_payloads() {
    for (status, errno) in [
        ("Submitted", "none"),
        ("WouldBlock", "11"),
        ("Rejected", "22"),
    ] {
        let record = format!(
            "sophia_live_atomic_test schema=1 output=2 scene_generation=91 status={status} errno={errno} request_scope=PageFlip nonblocking=true allow_modeset=false"
        );
        assert_eq!(
            reduced_record(&format!("{record} xid=123 payload=secret error=private")),
            Some(record)
        );
    }
    for record in [
        "sophia_other status=Submitted errno=22 request_scope=PageFlip",
        "sophia_live_atomic_test status=secret errno=private request_scope=unknown",
        "sophia_live_atomic_test errno=-22",
        "sophia_live_atomic_test errno=0",
        "sophia_live_atomic_test errno=2147483648",
    ] {
        assert_eq!(
            reduced_record(record).as_deref(),
            record.split_whitespace().next()
        );
    }
}

#[test]
fn protocol_tally_retains_refusal_classification_without_resource_payloads() {
    for status in ["clean", "compatibility_refusals", "degraded"] {
        let safe = format!(
            "sophia_live_session_protocol_error_tally schema=3 status={status} major=144 minor=30 code=1 count=5 distinct=1 discarded=64 total=69"
        );
        assert_eq!(
            reduced_record(&format!(
                "{safe} xid=123 resource_id=456 namespace_id=789 title=private error=secret payload=hidden"
            ))
            .unwrap(),
            safe
        );
    }
    assert_eq!(
        reduced_record("sophia_other_event major=144 minor=30 code=1 distinct=1 discarded=64 total=69 status=compatibility_refusals").unwrap(),
        "sophia_other_event"
    );
}

#[test]
fn present_feedback_retains_only_known_completion_modes() {
    for mode in ["Copy", "Flip", "Skip", "SuboptimalCopy"] {
        let record = format!(
            "sophia_live_session_present_feedback schema=1 kind=complete transaction=71 routed=true mode={mode} ust=10000 msc=23"
        );
        assert_eq!(
            reduced_record(&format!("{record} window=42 payload=private")),
            Some(record)
        );
        assert_eq!(
            reduced_record(&format!("sophia_other_event mode={mode}")),
            Some("sophia_other_event".to_owned())
        );
    }
    assert_eq!(
        reduced_record("sophia_live_session_present_feedback mode=private"),
        Some("sophia_live_session_present_feedback".to_owned())
    );
}

#[test]
fn paired_layout_observations_survive_without_resource_payloads() {
    let record = "sophia_live_layout_probe schema=1 output=2 scene_generation=91 source_image=817 status=Tested original_status=Rejected alternative_status=Submitted original_errno=22 alternative_errno=none format=875713112 original_modifier=144115188077027331 alternative_modifier=0";
    assert_eq!(
        reduced_record(&format!("{record} xid=123 payload=secret error=private")),
        Some(record.to_owned())
    );
    for stage in ["Atomic", "Framebuffer"] {
        let staged = record.replace("schema=1", &format!("schema=3 original_stage={stage}"));
        assert_eq!(
            reduced_record(&format!("{staged} xid=123 payload=secret error=private")),
            Some(staged)
        );
        assert_eq!(
            reduced_record(&format!("sophia_other_event original_stage={stage}")),
            Some("sophia_other_event".to_owned())
        );
    }
    let refused = "sophia_live_layout_probe schema=3 status=FramebufferRejected original_stage=Framebuffer output=2 scene_generation=91 format=875713112 original_modifier=144115188077027331 original_errno=22";
    assert_eq!(reduced_record(refused), Some(refused.to_owned()));
    for status in ["FramebufferRejectionIneligible", "LayoutMismatch"] {
        let record = format!("sophia_live_layout_probe schema=3 status={status}");
        assert_eq!(reduced_record(&record), Some(record));
    }
    let retired = "sophia_live_layout_probe schema=2 status=RetiredCopy transaction=71 output=2 scene_generation=91 source_image=817 native_generation=9 format=875713112 original_modifier=144115188077027331 alternative_modifier=0";
    assert_eq!(
        reduced_record(&format!("{retired} xid=123 device_path=/private/card")),
        Some(retired.to_owned())
    );
    let matched = "sophia_live_layout_probe schema=2 status=PreferenceMatched transaction=71 output=2 native_generation=9 preference_generation=17 format=875713112 original_modifier=144115188077027331 alternative_modifier=0";
    assert_eq!(
        reduced_record(&format!("{matched} xid=123 device_path=/private/card")),
        Some(matched.to_owned())
    );
    for field in [
        "status=secret",
        "original_stage=secret",
        "original_stage=Unknown",
        "original_stage=Prime",
        "original_stage=framebuffer",
        "original_status=secret",
        "alternative_status=secret",
        "original_errno=-22",
        "alternative_errno=2147483648",
        "source_image=18446744073709551616",
        "source_image=secret",
        "native_generation=secret",
        "preference_generation=18446744073709551616",
        "original_modifier=18446744073709551616",
        "format=4294967296",
    ] {
        assert_eq!(
            reduced_record(&format!("sophia_live_layout_probe {field}")),
            Some("sophia_live_layout_probe".to_owned())
        );
    }
    assert_eq!(
        reduced_record("sophia_other_event source_image=817"),
        Some("sophia_other_event".to_owned())
    );
}

#[test]
fn protocol_tally_classification_rejects_out_of_range_and_nonnumeric_fields() {
    let prefix = "sophia_live_session_protocol_error_tally";
    let maximums = format!(
        "{prefix} major=255 minor=65535 code=255 distinct=64 discarded={} total={}",
        u64::MAX,
        u64::MAX,
    );
    assert_eq!(reduced_record(&maximums).unwrap(), maximums);
    for field in [
        "major=256",
        "minor=65536",
        "code=256",
        "distinct=65",
        "discarded=18446744073709551616",
        "total=18446744073709551616",
        "major=-1",
        "minor=+1",
        "code=0xff",
        "distinct=1.0",
        "discarded=true",
        "total=none",
        "major=",
        "minor=private",
    ] {
        assert_eq!(
            reduced_record(&format!("{prefix} {field}")).unwrap(),
            prefix,
            "rejected field: {field}"
        );
    }
}

#[test]
fn session_failure_records_preserve_safe_phase_and_cause_without_error_payloads() {
    use sophia_session::diagnostics::{SessionFailurePhase, session_failure_record};

    let error: Box<dyn std::error::Error> =
        "persistent session controls did not drain cleanly".into();
    let record = session_failure_record(SessionFailurePhase::ControlDrain, error.as_ref());
    assert_eq!(
        record,
        "sophia_session_failure schema=1 status=failed phase=control_drain failure_code=session_control_drain"
    );
    assert_eq!(reduced_record(&record).unwrap(), record);

    let private = std::io::Error::other("/private/application/document");
    let record = session_failure_record(SessionFailurePhase::Authority, &private);
    assert_eq!(
        record,
        "sophia_session_failure schema=1 status=failed phase=authority failure_code=unclassified"
    );
    assert_eq!(reduced_record(&record).unwrap(), record);
    assert_eq!(
        reduced_record("sophia_session_failure phase=private_document failure_code=private_document error=private_document").unwrap(),
        "sophia_session_failure"
    );
}

#[test]
fn vt_failure_records_retain_the_boundary_and_safe_cause() {
    use sophia_renderer_live::LiveRendererScanoutBufferExportDetail as Detail;
    use sophia_session::diagnostics::failure_code;

    let error = Detail::WorkerDisconnected;
    let record = format!(
        "sophia_live_renderer_handoff schema=1 status=failed phase=export_images failure_code={} retained_count=5 error=private",
        failure_code(&error),
    );
    assert_eq!(
        reduced_record(&record).unwrap(),
        "sophia_live_renderer_handoff schema=1 status=failed phase=export_images failure_code=renderer_worker_disconnected retained_count=5"
    );
    let missing: Box<dyn std::error::Error> =
        "retained scene refers to an unavailable promoted renderer image".into();
    assert_eq!(failure_code(missing.as_ref()), "handoff_missing_image");
    for (message, code) in [
        (
            "public WM projection has no reconciled content placement",
            "wm_missing_reconciled_content",
        ),
        (
            "public WM retained output has no committed content placement",
            "wm_missing_retained_content",
        ),
    ] {
        let error: Box<dyn std::error::Error> = message.into();
        assert_eq!(failure_code(error.as_ref()), code);
        assert_eq!(
            reduced_record(&format!("sophia_session_failure schema=1 status=failed phase=window_management failure_code={code} error=private_document")).unwrap(),
            format!("sophia_session_failure schema=1 status=failed phase=window_management failure_code={code}")
        );
    }
    let private = std::io::Error::other("/private/application/document");
    assert_eq!(failure_code(&private), "unclassified");
    assert_eq!(
        reduced_record("sophia_live_renderer_handoff failure_code=private_document").unwrap(),
        "sophia_live_renderer_handoff"
    );
    for record in [
        "sophia_live_session_quiescence schema=3 status=complete pending_control_count=0",
        "sophia_live_session_quiescence schema=3 status=timed_out pending_control_count=1",
        "sophia_live_session_vt schema=4 status=preparing",
        "sophia_live_input_epoch schema=1 reason=virtual_terminal epoch=3",
        "sophia_live_session_runtime_fatal schema=1 status=detected source=owner_loop action=bounded_cleanup",
        "sophia_live_session_vt schema=6 status=quiesced outcome=forced_detach_timeout",
    ] {
        assert_eq!(reduced_record(record).unwrap(), record);
    }
}

#[test]
fn marker_windows_follow_the_boot_clock_across_wall_clock_changes() {
    let fixture = Fixture::new();
    let store = fixture.store();
    let record = store.begin("clock", std::process::id(), "").unwrap();
    let boot = fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap();
    fs::write(
        record.path.join("markers.log"),
        format!("incident\t1\t200000\t{}\tclock changed\n", boot.trim()),
    )
    .unwrap();
    let event_path = record.path.join("events.0.log");
    fs::write(&event_path, "1\t999999999\t200001\tsophia_test schema=1 count=1\n2\t2\t300000\tsophia_test schema=1 count=2\n").unwrap();
    fs::set_permissions(event_path, fs::Permissions::from_mode(0o600)).unwrap();
    let inspection = store.inspect(&record.id, Some("incident")).unwrap();
    assert_eq!(inspection.events.len(), 1);
    assert!(inspection.events[0].contains("count=1"));
    assert_eq!(inspection.window_start_msec, Some(140000));
    store.finish(&record.id, Some(1)).unwrap();
    let marker = store.mark(Some(&record.id), "reported much later").unwrap();
    fs::write(
        record.path.join("markers.log"),
        format!(
            "{}\t1\t400000\t{}\treported later\n",
            marker.id,
            boot.trim()
        ),
    )
    .unwrap();
    let inspection = store.inspect(&record.id, Some(&marker.id)).unwrap();
    assert_eq!(inspection.events.len(), 1);
    assert!(inspection.events[0].contains("count=2"));
}

#[test]
fn visual_progress_preserves_stages_and_frame_identity_without_payloads() {
    for record in [
        "sophia_live_visual_progress schema=1 status=enabled",
        "sophia_live_visual_progress schema=1 status=content stage=offered transaction=41 surface_token=0123456789abcdef source=dma_present",
        "sophia_live_visual_progress schema=1 status=committed_snapshot surface_token=0123456789abcdef generation=2 source=cpu",
        "sophia_live_visual_progress schema=1 status=head_snapshot output=1 head=2 target_generation=1 enabled=true baseline=false pending=none rendering=cpu:3:none submitted=mixed_present:4:41 presented=retained_mixed:2:none submissions=4 retirements=2 submissions_delta=3 retirements_delta=2 missed_count=3",
        "sophia_live_visual_progress schema=1 status=feedback_ready transaction=41 kind=idle",
        "sophia_live_session_present schema=2 status=retired transaction=41 surface=3 unit_scale=true",
    ] {
        let raw = format!(
            "{record} namespace=1 xid=3 title=secret pixels=secret checksum=123 text=secret"
        );
        assert_eq!(reduced_record(&raw).as_deref(), Some(record));
    }
}

#[test]
fn visual_progress_rejects_unbounded_identity_and_arbitrary_vocabulary() {
    for record in [
        "sophia_live_visual_progress pending=cpu:18446744073709551616:none",
        "sophia_live_visual_progress submitted=mixed_present:4:41:secret",
        "sophia_live_visual_progress pending=secret:4:41 surface_token=secret",
        "sophia_live_visual_progress status=secret stage=secret source=secret kind=secret head=-1",
        "sophia_other pending=cpu:4:none surface_token=0123456789abcdef head=4",
    ] {
        assert_eq!(
            reduced_record(record).as_deref(),
            record.split_whitespace().next()
        );
    }
}

#[test]
fn shell_action_causal_fields_are_scoped_and_bounded() {
    for record in [
        "sophia_shell_action_receipt schema=1 status=issued connection_epoch=1 content_grant_epoch=2 event_id=3 output=4 candidate_generation=5 presentation_epoch=6 target_id=9 target_generation=7 action=8 monotonic_usec=12345 disposition=0",
        "sophia_shell_action_receipt schema=1 status=acknowledged connection_epoch=1 content_grant_epoch=2 event_id=3 output=4 candidate_generation=5 presentation_epoch=6 target_id=9 target_generation=7 action=8 monotonic_usec=12350 disposition=1",
        "sophia_shell_action_cause schema=1 connection_epoch=1 event_id=3 output=4 action=8 activation_serial=9 policy_connection_epoch=10 admission=Admitted",
        "sophia_shell_indicator_state schema=1 connection_epoch=1 indicator_generation=13 output=4 indicator=2 action=8 slot=1 state_bits=1 entries=4",
        "sophia_shell_action_policy schema=1 policy_connection_epoch=10 activation_serial=9 action=8 transaction=11 request_id=12 indicator_generation=13 outcome=Committed",
        "sophia_shell_native_binding schema=1 connection_epoch=1 content_grant_epoch=2 output=3 candidate_generation=4 native_owner=5 native_frame=6 head=7 target_generation=8 heads=2 mode_refresh_millihz=60000",
        "sophia_shell_native_completion schema=1 output=3 native_owner=5 native_frame=6 heads=2 monotonic_usec=12345 timestamp_source=kernel missing_kernel_timestamp=0",
        "sophia_shell_content_shutdown schema=1 status=retained connection_epoch=1 content_grant_epoch=2 monotonic_usec=200000 workers_joined=1 settled_candidates=1 active_epochs=0 retired_epochs=1 candidates=0 resources=1 resource_ids=1 transfers=0 allocations=0 permits=0 demands=0 staging_bytes=0 resident_bytes=8 retiring_bytes=0 backing_bytes=8 reserved_resident_bytes=0 reserved_bytes=8 reserved_backing_bytes=8 response_records=0 response_bytes=0 input_records=0 input_bytes=0",
        "sophia_shell_content_sample schema=1 status=active connection_epoch=1 content_grant_epoch=2 monotonic_usec=200000 workers_joined=0 settled_candidates=0 active_epochs=0 retired_epochs=1 candidates=0 resources=1 resource_ids=1 transfers=0 allocations=0 permits=0 demands=0 staging_bytes=0 resident_bytes=8 retiring_bytes=0 backing_bytes=8 reserved_resident_bytes=0 reserved_bytes=8 reserved_backing_bytes=8 response_records=0 response_bytes=0 input_records=0 input_bytes=0",
        "sophia_shell_content_budget schema=1 connection_epoch=1 content_grant_epoch=2 limits_generation=1 max_staging_bytes=8388608 max_resident_bytes=16777216 max_retiring_bytes=16777216 max_live_resources=64 max_resource_ids=128 max_open_transfers=4 max_allocations_total=16 max_open_candidates_total=8 max_pending_candidates_total=8 max_control_records=128 max_input_queue_bytes=131072 max_output_queue_bytes=262144",
    ] {
        assert_eq!(
            reduced_record(&format!("{record} title=secret x=123 payload=456")),
            Some(record.into())
        );
    }
    for record in [
        "sophia_shell_action_receipt status=private monotonic_usec=-1 disposition=3",
        "sophia_shell_action_receipt event_id=18446744073709551616 action=+2",
        "sophia_shell_action_cause status=issued admission=secret target_generation=1",
        "sophia_shell_action_policy outcome=Cancelled event_id=1",
        "sophia_shell_action_cause schema=2",
        "sophia_shell_indicator_state slot=-1 state_bits=18446744073709551616 label=secret",
        "sophia_shell_native_completion event_id=1 monotonic_usec=18446744073709551616",
        "sophia_shell_native_binding native_frame=-1 native_owner=secret",
        "sophia_shell_native_completion timestamp_source=secret missing_kernel_timestamp=2",
        "sophia_shell_content_shutdown status=pretend response_bytes=-1 input_records=18446744073709551616 payload=secret",
    ] {
        assert_eq!(
            reduced_record(record).as_deref(),
            record.split_whitespace().next()
        );
    }
}

#[test]
fn content_pointer_cancellation_identity_survives_capture() {
    let line = "sophia_shell_pointer_binding schema=1 status=cancelled reason=target_continuity_lost pressed=false observed_output=2 candidate=15 presentation=9 layout_generation=2 authority_current=true";
    let reduced = reduced_record(line).expect("known diagnostic");
    for field in [
        "status=cancelled",
        "reason=target_continuity_lost",
        "observed_output=2",
        "candidate=15",
        "presentation=9",
    ] {
        assert!(reduced.contains(field), "missing {field}: {reduced}");
    }
}

#[test]
fn allocation_shutdown_capture_preserves_bounded_disposition_and_reason() {
    for record in [
        "sophia_window_allocation_publisher schema=1 status=stopped pending_cancelled=true generation=2",
        "sophia_live_session_quiescence schema=3 status=started reason=logout_complete timeout_msec=2000",
        "sophia_live_session_quiescence schema=3 status=frontend_drained reason=runtime_deadline elapsed_msec=70",
    ] {
        assert_eq!(reduced_record(record).as_deref(), Some(record));
    }
    let reduced =
        reduced_record("sophia_live_session_quiescence schema=3 reason=private-user-text").unwrap();
    assert!(!reduced.contains("reason="));
}

#[test]
fn independent_component_evidence_is_bounded_and_payload_free() {
    for record in [
        "sophia_shell_component schema=1 status=negotiated slot=0 role=bar connection_epoch=1 content_grant_epoch=1 revision=6 gpu_mode=direct gpu_grant_epoch=1 device_major=226 device_minor=129",
        "sophia_shell_component schema=1 status=negotiated slot=1 role=application_launcher connection_epoch=2 content_grant_epoch=2 revision=7 gpu_mode=denied gpu_grant_epoch=0 device_major=0 device_minor=0",
        "sophia_shell_component schema=1 status=service_failed slot=1",
        "sophia_shell_component schema=1 status=process_retired slot=1 connection_epoch=2 content_grant_epoch=2 endpoint_released=true",
        "sophia_shell_component schema=1 status=process_failed slot=1 connection_epoch=2 content_grant_epoch=2",
        "sophia_shell_component_catalog schema=1 status=built generation=1 entries=12",
        "sophia_native_launcher schema=1 status=process_started transaction=123",
        "sophia_native_launcher schema=1 status=spawn_failed",
    ] {
        assert_eq!(
            reduced_record(&format!(
                "{record} text=secret reason=private event=private path=/secret"
            )),
            Some(record.into())
        );
    }
    for fields in [
        "status=private",
        "slot=2",
        "slot=-1",
        "revision=65536",
        "role=other",
        "gpu_mode=auto",
        "connection_epoch=18446744073709551616",
        "device_minor=4294967296",
        "device_major=0xE2",
        "entries=32",
    ] {
        assert_eq!(
            reduced_record(&format!("sophia_shell_component {fields}")),
            Some("sophia_shell_component".into())
        );
    }
    assert_eq!(
        reduced_record("sophia_other gpu_mode=direct role=bar slot=0"),
        Some("sophia_other".into())
    );
}
