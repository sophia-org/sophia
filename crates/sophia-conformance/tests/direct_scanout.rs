use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sophia_conformance::{direct_scanout, direct_scanout_archive, direct_scanout_gate};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sophia-conformance-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn passing_log() -> String {
    [
        "sophia_live_session schema=18 status=bounded_complete display=:77 runtime_surfaces=0 runtime_max_surfaces=2 wm_policy=disabled wm_restarts=0",
        "sophia_live_session_present schema=2 status=retired transaction=242 surface=2097166 source=2560x1440 target=2560x1440_0_0 clip=2560x1440_0_0 unit_scale=true",
        "sophia_live_native_resources schema=12 status=complete direct_scanout_attempts=30 direct_scanout_flips=30 direct_scanout_tests=1 direct_scanout_test_rejections=0 direct_scanout_refusals=0 direct_scanout_unsupported=0 direct_scanout_fallbacks=0",
        "sophia_live_direct_scanout_verdicts schema=2 status=complete eligible=32 layer_count=26 layer_not_active=0 layer_resampled=0 layer_offset=0 layer_not_head_sized=0 layer_clipped=0 layer_not_dma_buf=0 layer_translucent=0 composition_required=0 composed_cursor=0",
        "sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=299 reason=none",
        "sophia_live_direct_scanout schema=1 status=test_passed output=1 scene_generation=299 reason=none",
        "sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=299 reason=none",
        "",
    ]
    .join("\n")
}

fn write_log(directory: &TempDir, text: &str) -> String {
    let path = directory.path().join("session.log");
    fs::write(&path, text).unwrap();
    path.display().to_string()
}

#[test]
fn standalone_verification_accepts_the_promoted_schema() {
    let directory = TempDir::new();
    let log = write_log(&directory, &passing_log());
    let report = direct_scanout::verify_standalone_logs(&[log]).unwrap();
    assert!(report.iter().any(|line| line.contains("30 flips")));
}

#[test]
fn duplicate_telemetry_fields_fail_closed() {
    let directory = TempDir::new();
    let text = passing_log().replace(
        "direct_scanout_attempts=30",
        "direct_scanout_attempts=30 direct_scanout_attempts=30",
    );
    let log = write_log(&directory, &text);
    let error = direct_scanout::verify_logs(&[log]).unwrap_err();
    assert!(error.contains("repeats field direct_scanout_attempts"));
}

#[test]
fn an_episode_cannot_flip_before_export() {
    let directory = TempDir::new();
    let text = passing_log().replace(
        "sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=299 reason=none\n",
        "",
    );
    let log = write_log(&directory, &text);
    let error = direct_scanout::verify_logs(&[log]).unwrap_err();
    assert!(error.contains("scene never exported"));
}

#[test]
fn standalone_verification_rejects_window_manager_chrome() {
    let directory = TempDir::new();
    let text = passing_log().replace("wm_policy=disabled", "wm_policy=required");
    let log = write_log(&directory, &text);
    let error = direct_scanout::verify_standalone_logs(&[log]).unwrap_err();
    assert!(error.contains("window manager ran"));
}

#[test]
fn probe_arguments_are_typed_before_the_display_is_taken() {
    let error = direct_scanout_gate::Probe::from_arguments(&[
        "zero".to_owned(),
        "1440".to_owned(),
        "20".to_owned(),
        "kitty".to_owned(),
    ])
    .unwrap_err();
    assert!(error.contains("width must be a positive integer"));

    let error = direct_scanout_gate::Probe::from_arguments(&[
        "2560".to_owned(),
        "1440".to_owned(),
        "20".to_owned(),
        "unknown".to_owned(),
    ])
    .unwrap_err();
    assert!(error.contains("workload must be"));
}

#[test]
fn identity_binding_preserves_the_archive_schema() {
    let directory = TempDir::new();
    let session = directory.path().join("input.log");
    let evidence = directory.path().join("evidence.log");
    let sophia = directory.path().join("sophia");
    let client = directory.path().join("kitty");
    let core = directory.path().join("core.kdl");
    let desktop = directory.path().join("desktop.kdl");
    fs::write(&session, passing_log()).unwrap();
    fs::write(&sophia, "sophia").unwrap();
    fs::write(&client, "client").unwrap();
    fs::write(&core, "core").unwrap();
    fs::write(&desktop, "desktop").unwrap();

    direct_scanout_archive::bind_evidence(&direct_scanout_archive::BindEvidence {
        session_log: &session,
        evidence: &evidence,
        source_commit: "0123456789012345678901234567890123456789",
        sophia_binary: &sophia,
        client_binary: &client,
        core_config: &core,
        desktop_profile: &desktop,
    })
    .unwrap();

    let text = fs::read_to_string(evidence).unwrap();
    let identity = text.lines().last().unwrap();
    assert!(
        identity.starts_with("sophia_direct_scanout_identity schema=1 status=bound source_commit=")
    );
    assert!(identity.contains(" client=kitty "));
    assert!(identity.ends_with(&format!(
        "desktop_sha256={}",
        direct_scanout_archive::sha256(&desktop).unwrap()
    )));
}

/// A session that flipped, opened an overlay, composed while it was up, and
/// resumed flipping only after a fresh validating commit.
fn overlay_log() -> String {
    [
        "sophia_live_session schema=18 status=bounded_complete display=:77 runtime_surfaces=0 runtime_max_surfaces=2 wm_policy=disabled wm_restarts=0",
        "sophia_live_native_resources schema=12 status=complete direct_scanout_attempts=30 direct_scanout_flips=30 direct_scanout_tests=2 direct_scanout_test_rejections=0 direct_scanout_refusals=0 direct_scanout_unsupported=0 direct_scanout_fallbacks=0",
        "sophia_live_direct_scanout_verdicts schema=2 status=complete eligible=32 layer_count=26 layer_not_active=0 layer_resampled=0 layer_offset=0 layer_not_head_sized=0 layer_clipped=0 layer_not_dma_buf=0 layer_translucent=0 composition_required=12 composed_cursor=0",
        "sophia_live_session_present schema=2 status=retired transaction=242 surface=2097166 source=2560x1440 target=2560x1440_0_0 clip=2560x1440_0_0 unit_scale=true",
        "sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=299 reason=none",
        "sophia_live_direct_scanout schema=1 status=test_passed output=1 scene_generation=299 reason=none",
        "sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=299 reason=none",
        "sophia_live_direct_scanout_overlay_proof schema=1 status=activated output=1 flips_before=10",
        "sophia_live_direct_scanout_geometry schema=2 status=composition_required command=rect output=1 head_width=2560 head_height=1440 layer_x=0 layer_y=0 layer_width=2560 layer_height=1440",
        "sophia_live_session_present schema=2 status=retired transaction=243 surface=2097166 source=2560x1440 target=2560x1440_0_0 clip=2560x1440_0_0 unit_scale=true",
        "sophia_live_direct_scanout_overlay_proof schema=1 status=withdrawn output=0 flips_before=10",
        "sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=400 reason=none",
        "sophia_live_direct_scanout schema=1 status=test_passed output=1 scene_generation=400 reason=none",
        "sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=400 reason=none",
        "",
    ]
    .join("\n")
}

fn overlay_verification(text: &str) -> Result<Vec<String>, String> {
    let directory = TempDir::new();
    let log = write_log(&directory, text);
    direct_scanout::verify_standalone_logs_with_overlay(&[log], true)
}

#[test]
fn overlay_verification_accepts_a_return_to_composition() {
    let report = overlay_verification(&overlay_log()).unwrap();
    assert!(
        report
            .iter()
            .any(|line| line.contains("sophia_direct_scanout_overlay schema=1 status=returned")),
        "the overlay window should be reported: {report:?}"
    );
}

/// A run that never opened one is a different proof, not a failed one. Every
/// existing caller and both archives verify through the same entry.
#[test]
fn a_run_without_an_overlay_still_verifies_without_the_requirement() {
    let directory = TempDir::new();
    let log = write_log(&directory, &passing_log());
    direct_scanout::verify_standalone_logs(&[log]).unwrap();
}

#[test]
fn a_run_without_an_overlay_is_refused_when_one_was_required() {
    let error = overlay_verification(&passing_log()).unwrap_err();
    assert!(error.contains("never activated"), "{error}");
}

/// An activation ends the eligibility episode, so a flip inside the window
/// reached the plane under a stamp the activation invalidated.
#[test]
fn a_flip_while_the_overlay_is_up_is_refused() {
    let text = overlay_log().replace(
        "sophia_live_direct_scanout_geometry schema=2 status=composition_required command=rect output=1 head_width=2560 head_height=1440 layer_x=0 layer_y=0 layer_width=2560 layer_height=1440",
        "sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=350 reason=none\nsophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=350 reason=none\nsophia_live_direct_scanout_geometry schema=2 status=composition_required command=rect output=1 head_width=2560 head_height=1440 layer_x=0 layer_y=0 layer_width=2560 layer_height=1440",
    );
    let error = overlay_verification(&text).unwrap_err();
    assert!(error.contains("while the overlay was up"), "{error}");
}

/// Without a painting refusal the window is satisfiable by a session that
/// simply stopped drawing, which proves nothing about composition.
#[test]
fn an_overlay_that_drew_nothing_is_refused() {
    let text = overlay_log()
        .lines()
        .filter(|line| !line.contains("sophia_live_direct_scanout_geometry"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = overlay_verification(&text).unwrap_err();
    assert!(error.contains("drew nothing"), "{error}");
}

/// `SuccessorComposedRetires`: the displaced direct frame is retired by a
/// composed successor, and a window with no retirement never exercised it.
#[test]
fn an_overlay_window_with_no_retirement_is_refused() {
    let text = overlay_log().replace(
        "sophia_live_session_present schema=2 status=retired transaction=243 surface=2097166 source=2560x1440 target=2560x1440_0_0 clip=2560x1440_0_0 unit_scale=true\n",
        "",
    );
    let error = overlay_verification(&text).unwrap_err();
    assert!(error.contains("no composed successor"), "{error}");
}

/// `ReProveAfterEpisodeChange`: eligibility returns only through a fresh test.
#[test]
fn a_flip_resuming_without_a_fresh_test_is_refused() {
    let text = overlay_log().replace(
        "sophia_live_direct_scanout schema=1 status=test_passed output=1 scene_generation=400 reason=none\n",
        "",
    );
    let error = overlay_verification(&text).unwrap_err();
    assert!(
        error.contains("without a fresh validating commit"),
        "{error}"
    );
}

/// An overlay left up at session end means the withdrawal never happened, so
/// re-eligibility was never asked for.
#[test]
fn an_overlay_that_never_withdrew_is_refused() {
    let text = overlay_log()
        .lines()
        .filter(|line| !line.contains("status=withdrawn"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = overlay_verification(&text).unwrap_err();
    assert!(error.contains("never withdrew"), "{error}");
}

/// Two activations cannot be paired into one window, and the second would be a
/// second episode the brackets silently swallow.
#[test]
fn an_overlay_that_opened_twice_is_refused() {
    let text = overlay_log().replace(
        "sophia_live_direct_scanout_overlay_proof schema=1 status=withdrawn output=0 flips_before=10",
        "sophia_live_direct_scanout_overlay_proof schema=1 status=activated output=1 flips_before=20\nsophia_live_direct_scanout_overlay_proof schema=1 status=withdrawn output=0 flips_before=20",
    );
    let error = overlay_verification(&text).unwrap_err();
    assert!(error.contains("opened twice"), "{error}");
}

/// The gate and the probe share one argument vocabulary, so the flag has to
/// survive alongside the positional arguments rather than displacing one.
#[test]
fn the_overlay_flag_parses_beside_the_positional_arguments() {
    let plain = direct_scanout_gate::Probe::from_arguments(&[]).unwrap();
    assert!(!plain.overlay_proof, "overlay proof is off by default");

    let flagged =
        direct_scanout_gate::Probe::from_arguments(&["--overlay-proof".to_owned()]).unwrap();
    assert!(flagged.overlay_proof);
    assert_eq!(flagged.width, plain.width, "the flag is not a width");
    assert_eq!(flagged.height, plain.height);

    let mixed = direct_scanout_gate::Probe::from_arguments(&[
        "1920".to_owned(),
        "--overlay-proof".to_owned(),
        "1080".to_owned(),
    ])
    .unwrap();
    assert!(mixed.overlay_proof);
    assert_eq!(mixed.width, 1920);
    assert_eq!(mixed.height, 1080, "the flag must not consume a position");
}

/// The gate reads its terminal from the descriptor rather than from `tty(1)`.
///
/// `Command::output` closes the child's stdin, so the subprocess this replaced
/// was asking `tty` about a null descriptor: on a real tty3 it answered "not a
/// tty" and exited 1, and the gate refused before it could start. Reading
/// `/proc/self/fd/0` asks the descriptor we already hold.
#[test]
fn the_gate_terminal_is_read_from_the_descriptor_not_a_subprocess() {
    use std::process::{Command, Stdio};

    // The mechanism that broke: a captured child cannot see our terminal.
    let captured = Command::new("tty").output().unwrap();
    assert!(
        !captured.status.success(),
        "a captured `tty` child can never identify the parent's terminal"
    );

    // The mechanism that replaced it always resolves for this process,
    // whatever it is attached to -- a terminal for the gate, a socket for a
    // test harness. What it must never do is fail the way the subprocess did.
    std::fs::read_link("/proc/self/fd/0")
        .expect("this process's own stdin descriptor always resolves");

    // And the decision the gate actually makes about what it resolved to.
    assert!(direct_scanout_gate::is_gate_terminal(Path::new(
        "/dev/tty3"
    )));
    assert!(!direct_scanout_gate::is_gate_terminal(Path::new(
        "/dev/tty1"
    )));
    let _ = Stdio::null();
}

/// A record decorated by `tracing` -- timestamp, level, module, ANSI colour --
/// ahead of the marker, as a verbose session log actually carries it.
fn decorated(record: &str) -> String {
    format!(
        "\u{1b}[2m2026-08-29T22:46:34.398135Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m \u{1b}[2msophia_backend_live::scanout::rendered_scanout::exporter::direct\u{1b}[0m\u{1b}[2m:\u{1b}[0m {record}"
    )
}

/// Episode records are read wherever the marker sits in the line, because the
/// session emits them through `tracing` and a verbose log decorates them.
///
/// Anchoring to the line start saw only bare records, which was none of them:
/// a physical run whose evidence plainly contained the fresh validating
/// commit was refused for lacking one, and `episode_sessions=0` in every
/// earlier gate summary was the same blindness passing vacuously -- the
/// episode-order rules had never actually run against hardware evidence.
#[test]
fn decorated_episode_records_are_read_like_bare_ones() {
    let text = [
        "sophia_live_session schema=18 status=bounded_complete display=:77 runtime_surfaces=0 runtime_max_surfaces=2 wm_policy=disabled wm_restarts=0",
        "sophia_live_native_resources schema=12 status=complete direct_scanout_attempts=30 direct_scanout_flips=30 direct_scanout_tests=2 direct_scanout_test_rejections=0 direct_scanout_refusals=0 direct_scanout_unsupported=0 direct_scanout_fallbacks=0",
        "sophia_live_direct_scanout_verdicts schema=2 status=complete eligible=32 layer_count=26 layer_not_active=0 layer_resampled=0 layer_offset=0 layer_not_head_sized=0 layer_clipped=0 layer_not_dma_buf=0 layer_translucent=0 composition_required=2 composed_cursor=0",
        "sophia_live_session_present schema=2 status=retired transaction=242 surface=2097166 source=2560x1440 target=2560x1440_0_0 clip=2560x1440_0_0 unit_scale=true",
        &decorated("sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=299 reason=none"),
        &decorated("sophia_live_direct_scanout schema=1 status=test_passed output=1 scene_generation=299 reason=none"),
        &decorated("sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=299 reason=none"),
        "sophia_live_direct_scanout_overlay_proof schema=1 status=activated output=1 flips_before=10",
        &decorated("sophia_live_direct_scanout_geometry schema=2 status=composition_required command=rect output=1 head_width=2560 head_height=1440 layer_x=0 layer_y=0 layer_width=2560 layer_height=1440"),
        "sophia_live_session_present schema=2 status=retired transaction=243 surface=2097166 source=2560x1440 target=2560x1440_0_0 clip=2560x1440_0_0 unit_scale=true",
        "sophia_live_direct_scanout_overlay_proof schema=1 status=withdrawn output=0 flips_before=10",
        &decorated("sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=400 reason=none"),
        &decorated("sophia_live_direct_scanout schema=1 status=test_passed output=1 scene_generation=400 reason=none"),
        &decorated("sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=400 reason=none"),
        "",
    ]
    .join("\n");

    let report = overlay_verification(&text).expect("decorated records satisfy every rule");
    assert!(
        report
            .iter()
            .any(|line| line.contains("sophia_direct_scanout_overlay schema=1 status=returned")),
        "{report:?}"
    );
    // And the order rules actually ran: a session whose episodes were seen is
    // counted, where the blind reader always reported zero.
    assert!(
        report
            .iter()
            .any(|line| line.contains("episode_sessions=1")),
        "episode records were not seen: {report:?}"
    );
}

/// The rules still bite on decorated records: a decorated flip inside the
/// window is a flip inside the window.
#[test]
fn a_decorated_flip_inside_the_window_is_still_refused() {
    let text = overlay_log().replace(
        "sophia_live_direct_scanout_overlay_proof schema=1 status=withdrawn output=0 flips_before=10",
        &format!(
            "{}\n{}\nsophia_live_direct_scanout_overlay_proof schema=1 status=withdrawn output=0 flips_before=10",
            decorated("sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=350 reason=none"),
            decorated("sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=350 reason=none")
        ),
    );
    let error = overlay_verification(&text).unwrap_err();
    assert!(error.contains("while the overlay was up"), "{error}");
}

/// Attempts with no readable episode records mean the reader is blind.
///
/// This is the rule that was missing. Every attempt emits an `exported`
/// record, so counters without episodes is not a quiet session -- it is a
/// matcher failing to match. `episode_sessions=0` sat in every gate summary
/// from archive 0001 onward while the order rules never ran, and nothing
/// asked why.
#[test]
fn attempts_without_readable_episodes_are_refused() {
    let text = passing_log()
        .lines()
        .filter(|line| !line.contains("sophia_live_direct_scanout schema=1 status="))
        .collect::<Vec<_>>()
        .join("\n");
    let directory = TempDir::new();
    let log = write_log(&directory, &text);
    let error = direct_scanout::verify_logs(&[log]).unwrap_err();
    assert!(error.contains("the reader is not matching"), "{error}");
}

/// A session whose every record is decorated verifies end to end, which is
/// what the promoted archives actually look like.
#[test]
fn a_fully_decorated_session_verifies() {
    let text = passing_log()
        .lines()
        .map(|line| {
            if line.is_empty() {
                line.to_owned()
            } else {
                decorated(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let directory = TempDir::new();
    let log = write_log(&directory, &text);
    let report = direct_scanout::verify_standalone_logs(&[log]).unwrap();
    assert!(
        report
            .iter()
            .any(|line| line.contains("episode_sessions=1")),
        "{report:?}"
    );
}

fn cost_record(population: &str, frames: usize, saturated: bool) -> String {
    format!(
        "sophia_live_direct_scanout_cost schema=1 population={population} frames={frames} offer_submit_us_min=90 offer_submit_us_p50=120 offer_submit_us_p99=300 offer_submit_us_max=310 submit_flip_frames={frames} submit_flip_us_min=8000 submit_flip_us_p50=8300 submit_flip_us_p99=9000 submit_flip_us_max=9100 saturated={saturated}"
    )
}

/// A session that measured both populations, as a cost run produces.
fn cost_log() -> String {
    format!(
        "{}\n{}\n{}\n",
        overlay_log().trim_end(),
        cost_record("direct", 27, false),
        cost_record("composed", 15, false)
    )
}

fn cost_verification(text: &str) -> Result<Vec<String>, String> {
    let directory = TempDir::new();
    let log = write_log(&directory, text);
    direct_scanout::verify_standalone_logs_with(&[log], true, true)
}

#[test]
fn cost_verification_reports_both_populations() {
    let report = cost_verification(&cost_log()).unwrap();
    assert!(
        report.iter().any(|line| {
            line.contains("sophia_direct_scanout_cost schema=1 status=measured")
                && line.contains("direct_frames=27")
                && line.contains("composed_frames=15")
        }),
        "{report:?}"
    );
}

/// The question is a comparison. One population is an anecdote about direct
/// frames with nothing to hold it against.
#[test]
fn a_run_with_only_one_population_is_refused() {
    let text = format!(
        "{}\n{}\n",
        overlay_log().trim_end(),
        cost_record("direct", 27, false)
    );
    let error = cost_verification(&text).unwrap_err();
    assert!(error.contains("nothing to compare against"), "{error}");
}

/// A truncated reservoir describes a prefix of the run. Reporting its
/// percentiles as the run's would be a quiet lie about what was measured.
#[test]
fn a_saturated_population_is_refused() {
    let text = format!(
        "{}\n{}\n{}\n",
        overlay_log().trim_end(),
        cost_record("direct", 4096, true),
        cost_record("composed", 15, false)
    );
    let error = cost_verification(&text).unwrap_err();
    assert!(error.contains("prefix of the run"), "{error}");
}

/// A gate asked to measure that finds no records ran a binary that cannot
/// measure -- the stale-build class `--validate-session-args` exists for.
#[test]
fn a_cost_run_that_measured_nothing_is_refused() {
    let error = cost_verification(&overlay_log()).unwrap_err();
    assert!(error.contains("measured no frame costs"), "{error}");
}

/// Runs that predate the instrumentation keep verifying, which is what lets
/// archives 0001 and 0002 stay in the corpus.
#[test]
fn a_run_without_cost_records_still_verifies_when_not_asked() {
    let directory = TempDir::new();
    let log = write_log(&directory, &overlay_log());
    direct_scanout::verify_standalone_logs_with(&[log], true, false).unwrap();
}

/// Cost records ride through `tracing` like the rest, so the reader must find
/// them wherever the marker sits.
#[test]
fn decorated_cost_records_are_read() {
    let text = format!(
        "{}\n{}\n{}\n",
        overlay_log().trim_end(),
        decorated(&cost_record("direct", 27, false)),
        decorated(&cost_record("composed", 15, false))
    );
    let report = cost_verification(&text).expect("decorated cost records are records");
    assert!(
        report
            .iter()
            .any(|line| line.contains("composed_frames=15")),
        "{report:?}"
    );
}

/// A population reported twice would have one summary silently win.
#[test]
fn a_population_reported_twice_is_refused() {
    let text = format!(
        "{}\n{}\n{}\n{}\n",
        overlay_log().trim_end(),
        cost_record("direct", 27, false),
        cost_record("composed", 15, false),
        cost_record("composed", 2, false)
    );
    let error = cost_verification(&text).unwrap_err();
    assert!(error.contains("reported twice"), "{error}");
}

/// The gate's `--cost` implies the overlay proof: without the window there is
/// no composed population to measure.
#[test]
fn asking_for_cost_asks_for_the_overlay_that_produces_it() {
    let probe = direct_scanout_gate::Probe::from_arguments(&["--cost".to_owned()]).unwrap();
    assert!(probe.cost);
    assert!(
        probe.overlay_proof,
        "a cost run must open the overlay it measures the composed side of"
    );
    assert!(
        probe.hold_seconds >= 35,
        "and must hold it long enough to have a distribution"
    );
}

/// A distribution over no frames is not a measurement. The session's own
/// emitter omits an empty population rather than reporting zeros, so evidence
/// claiming one came from something else.
#[test]
fn a_population_over_no_frames_is_refused() {
    let text = format!(
        "{}\n{}\n{}\n",
        overlay_log().trim_end(),
        cost_record("direct", 27, false),
        cost_record("composed", 0, false)
    );
    let error = cost_verification(&text).unwrap_err();
    assert!(error.contains("only one side"), "{error}");
}

/// The exact shape the first cost run produced: frames reached glass, but
/// every export was filed under the other population, so one side of the
/// measurement was empty. The record now shows it instead of vanishing.
#[test]
fn a_population_measured_on_only_one_side_is_refused() {
    let half =
        cost_record("direct", 0, false).replace("submit_flip_frames=0", "submit_flip_frames=30");
    let text = format!(
        "{}\n{}\n{}\n",
        overlay_log().trim_end(),
        half,
        cost_record("composed", 47, false)
    );
    let error = cost_verification(&text).unwrap_err();
    assert!(
        error.contains("only one side") && error.contains("0 offer samples"),
        "{error}"
    );
}

fn cursor_record(path: &str, updates: usize, failures: usize) -> String {
    cursor_record_with_plane(path, "accepted", updates, failures)
}

fn cursor_record_with_plane(path: &str, plane: &str, updates: usize, failures: usize) -> String {
    format!(
        "sophia_live_session_cursor schema=5 path={path} plane={plane} moves_coalesced=3 max_motion_to_submit_msec=2 initialization_max_msec=0 initialization_deferrals=0 max_update_msec=1 updates_primary_in_flight=4 buttons_routed=0 hardware_updates={updates} hidden_updates=0 hardware_failures={failures}"
    )
}

/// Schema 7 is read the same way: the reader takes fields by name, so the
/// rename and the two new cost fields must not disturb it.
fn cursor_record_schema_seven(path: &str, plane: &str, updates: usize, failures: usize) -> String {
    format!(
        "sophia_live_session_cursor schema=7 path={path} plane={plane} moves_coalesced=3 max_motion_to_submit_msec=2 initialization_max_msec=0 initialization_deferrals=0 max_update_msec=1 legacy_updates_primary_in_flight=4 buttons_routed=0 hardware_updates={updates} hidden_updates=0 hardware_failures={failures}"
    )
}

/// A session that moved a cursor over direct frames and kept flipping after.
fn cursor_log() -> String {
    [
        "sophia_live_session schema=18 status=bounded_complete display=:77 runtime_surfaces=0 runtime_max_surfaces=2 wm_policy=disabled wm_restarts=0",
        "sophia_live_native_resources schema=12 status=complete direct_scanout_attempts=30 direct_scanout_flips=30 direct_scanout_tests=1 direct_scanout_test_rejections=0 direct_scanout_refusals=0 direct_scanout_unsupported=0 direct_scanout_fallbacks=0",
        "sophia_live_direct_scanout_verdicts schema=2 status=complete eligible=32 layer_count=26 layer_not_active=0 layer_resampled=0 layer_offset=0 layer_not_head_sized=0 layer_clipped=0 layer_not_dma_buf=0 layer_translucent=0 composition_required=0 composed_cursor=0",
        "sophia_live_session_present schema=2 status=retired transaction=242 surface=2097166 source=2560x1440 target=2560x1440_0_0 clip=2560x1440_0_0 unit_scale=true",
        "sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=299 reason=none",
        "sophia_live_direct_scanout schema=1 status=test_passed output=1 scene_generation=299 reason=none",
        "sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=299 reason=none",
        "sophia_live_direct_scanout_cursor_proof schema=1 status=started output=1 flips_before=10",
        "sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=320 reason=none",
        "sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=320 reason=none",
        "sophia_live_direct_scanout_cursor_proof schema=1 status=finished moves=12 flips_after=24",
        "sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=400 reason=none",
        "sophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=400 reason=none",
        "",
    ]
    .join("\n")
        + &cursor_record("legacy_ioctl", 13, 0)
        + "\n"
}

fn cursor_verification(text: &str) -> Result<Vec<String>, String> {
    let directory = TempDir::new();
    let log = write_log(&directory, text);
    direct_scanout::verify_standalone_logs_proving(&[log], false, false, true)
}

#[test]
fn cursor_verification_accepts_motion_over_direct_frames() {
    let report = cursor_verification(&cursor_log()).unwrap();
    assert!(
        report.iter().any(|line| {
            line.contains("sophia_direct_scanout_cursor schema=2 status=rode_hardware")
                && line.contains("moves=12")
        }),
        "{report:?}"
    );
}

/// The claim is about a cursor that moved. Every archive so far had one that
/// was visible and still, which is why this proof exists at all.
#[test]
fn a_cursor_that_never_moved_is_refused() {
    let text = cursor_log()
        .lines()
        .filter(|line| !line.contains("cursor_proof"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = cursor_verification(&text).unwrap_err();
    assert!(error.contains("never moved over a direct frame"), "{error}");
}

/// A cursor that fell back to composition still looks like a moving cursor
/// on screen, and means the opposite of what this proof claims. Neither
/// hardware path is named `composited`, so it is refused as unrecognised
/// rather than read as one of them.
#[test]
fn a_cursor_that_left_the_hardware_path_is_refused() {
    let text = cursor_log().replace(
        &cursor_record("legacy_ioctl", 13, 0),
        &cursor_record("composited", 13, 0),
    );
    let error = cursor_verification(&text).unwrap_err();
    assert!(error.contains("unknown path"), "{error}");
}

#[test]
fn a_cursor_that_failed_on_hardware_is_refused() {
    let text = cursor_log().replace(
        &cursor_record("legacy_ioctl", 13, 0),
        &cursor_record("legacy_ioctl", 13, 2),
    );
    let error = cursor_verification(&text).unwrap_err();
    assert!(error.contains("failed while riding"), "{error}");
}

/// The proof control can report moves the hardware never performed if the
/// updates were dropped above the ioctl, so the cursor record has to agree.
#[test]
fn moves_the_hardware_never_performed_are_refused() {
    let text = cursor_log().replace(
        &cursor_record("legacy_ioctl", 13, 0),
        &cursor_record("legacy_ioctl", 1, 0),
    );
    let error = cursor_verification(&text).unwrap_err();
    assert!(error.contains("nothing moved on hardware"), "{error}");
}

/// The outcome this proof exists to rule out: a cursor that moved because
/// direct scanout had already stopped.
#[test]
fn a_run_with_no_flips_after_the_motion_is_refused() {
    let text = cursor_log().replace(
        "sophia_live_direct_scanout schema=1 status=exported output=1 scene_generation=400 reason=none\nsophia_live_direct_scanout schema=1 status=flipped output=1 scene_generation=400 reason=none\n",
        "",
    );
    let error = cursor_verification(&text).unwrap_err();
    assert!(error.contains("may have ended direct scanout"), "{error}");
}

/// An unbounded proof would leave a session moving a cursor forever.
#[test]
fn a_proof_that_never_finished_is_refused() {
    let text = cursor_log()
        .lines()
        .filter(|line| !line.contains("status=finished"))
        .collect::<Vec<_>>()
        .join("\n");
    let error = cursor_verification(&text).unwrap_err();
    assert!(error.contains("never finished"), "{error}");
}

/// Runs without cursor records verify unchanged, keeping archives 0001
/// through 0003 in the corpus.
#[test]
fn a_run_without_cursor_records_still_verifies_when_not_asked() {
    let directory = TempDir::new();
    let log = write_log(&directory, &passing_log());
    direct_scanout::verify_standalone_logs_proving(&[log], false, false, false).unwrap();
}

/// The atomic path proves the same claim as the legacy one, and the record
/// says which was driven. That field is the difference between archive 0004's
/// baseline and the run that has to match it.
#[test]
fn cursor_verification_accepts_the_atomic_path() {
    let text = cursor_log().replace(
        &cursor_record("legacy_ioctl", 13, 0),
        &cursor_record("atomic_plane", 13, 0),
    );
    let report = cursor_verification(&text).unwrap();
    assert!(
        report
            .iter()
            .any(|line| line.contains("path=atomic_plane") && line.contains("plane=accepted")),
        "{report:?}"
    );
}

/// A session cannot drive a plane the card refused. The record carries both
/// facts so this is checkable rather than assumed -- a run claiming otherwise
/// describes something that did not happen.
#[test]
fn a_cursor_claiming_a_plane_the_card_refused_is_rejected() {
    for plane in ["refused", "unprobed"] {
        let text = cursor_log().replace(
            &cursor_record("legacy_ioctl", 13, 0),
            &cursor_record_with_plane("atomic_plane", plane, 13, 0),
        );
        let error = cursor_verification(&text).unwrap_err();
        assert!(error.contains("the card reported as"), "{plane}: {error}");
    }
}

/// The legacy path over a card that would accept a plane is an ordinary run,
/// not a contradiction: capability is not a decision.
#[test]
fn the_legacy_path_over_a_capable_card_is_accepted() {
    let text = cursor_log().replace(
        &cursor_record("legacy_ioctl", 13, 0),
        &cursor_record_with_plane("legacy_ioctl", "accepted", 13, 0),
    );
    cursor_verification(&text).expect("a capable card driven legacy is ordinary");
}

/// The schema-7 shape reads the same, rename and all.
///
/// Schema 7 renamed the in-flight counter to name the path that counts it and
/// added what a blocking cursor-only commit cost. The reader takes fields by
/// name, so neither should disturb it -- and a reader that silently stopped
/// recognising the newest record would verify nothing on every run from here.
#[test]
fn the_current_cursor_schema_verifies() {
    let text = cursor_log().replace(
        &cursor_record("legacy_ioctl", 13, 0),
        &cursor_record_schema_seven("legacy_ioctl", "accepted", 13, 0),
    );
    let report = cursor_verification(&text).expect("the schema-7 shape reads");
    assert!(
        report
            .iter()
            .any(|line| line.contains("path=legacy_ioctl") && line.contains("plane=accepted")),
        "{report:?}"
    );
}

/// A schema-4 record still verifies, because archive `0004` is one.
///
/// That schema predates the atomic path, so the reader supplies what the
/// shape implies -- legacy ioctl, nothing probed -- rather than guessing or
/// refusing. A reader that only understood the newest schema would stop
/// checking the proof that archive exists to make, which is what the archive
/// corpus caught when this record's schema was bumped.
#[test]
fn the_previous_cursor_schema_still_verifies() {
    let legacy = cursor_record("legacy_ioctl", 13, 0);
    let schema_four = "sophia_live_session_cursor schema=4 path=legacy_ioctl moves_coalesced=3 max_motion_to_submit_msec=2 initialization_max_msec=0 initialization_deferrals=0 max_update_msec=1 updates_primary_in_flight=4 buttons_routed=0 hardware_updates=13 hidden_updates=0 hardware_failures=0".to_string();
    let text = cursor_log().replace(&legacy, &schema_four);
    let report = cursor_verification(&text).expect("archive 0004's shape still reads");
    assert!(
        report
            .iter()
            .any(|line| line.contains("path=legacy_ioctl") && line.contains("plane=unprobed")),
        "{report:?}"
    );
}

#[test]
fn an_archived_schema_16_session_is_still_read() {
    // A promoted archive is a recorded session and keeps the schema it had.
    // Refusing it by schema would silence every archive the moment the
    // emitter moved on; the field check is where old evidence is refused.
    let log = passing_log().replace(
        "sophia_live_session schema=18 status=bounded_complete display=:77 runtime_surfaces=0 runtime_max_surfaces=2 ",
        "sophia_live_session schema=16 status=bounded_complete display=:77 runtime_surfaces=0 ",
    );
    assert!(
        log.contains("schema=16 status=bounded_complete"),
        "the fixture must be the archive shape"
    );
    let dir = std::env::temp_dir().join(format!(
        "sophia-direct-scanout-archive-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("session.log");
    std::fs::write(&path, log).unwrap();
    let verdict = direct_scanout::verify_logs(&[path.to_string_lossy().to_string()]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(verdict.is_ok(), "{verdict:?}");
}
