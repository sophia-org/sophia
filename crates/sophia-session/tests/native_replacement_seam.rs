//! One resolver seam for native replacement (t306, t310). Hotplug, seat
//! resume, a rejected or timed-out terminal switch and startup recovery all
//! suspend, capture and close their owner, then schedule the topology phase,
//! which alone resolves the admitted output profile, constructs the
//! replacement and resumes onto it. The owner loop has no device-free driver,
//! so this checks the source: no Session path constructs an owner outside
//! that resolution, and only the topology phase resumes onto one.

use std::fs;
use std::path::{Path, PathBuf};

fn sources(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

/// Session source files that contain `needle`, relative to `src`.
fn containing(needle: &str) -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&root, &mut files);
    let mut hits = files
        .iter()
        .filter(|path| fs::read_to_string(path).unwrap().contains(needle))
        .map(|path| {
            path.strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    hits.sort();
    hits
}

#[test]
fn no_session_path_constructs_a_native_owner_outside_admitted_resolution() {
    assert_eq!(
        containing("new_with_seat_mirroring_mapping_and_cursor"),
        Vec::<String>::new()
    );
    assert_eq!(
        containing("from_resolved_replacement("),
        [
            "live_session/output_replacement/runtime.rs",
            "live_session/output_replacement/startup.rs",
        ]
    );
}

#[test]
fn only_the_topology_phase_resumes_onto_a_replacement() {
    // The helpers' own definitions, and the one phase that calls them.
    assert_eq!(
        containing("resume_native_scanout_from_scene"),
        [
            "live_session/owner_loop/renderer_image_handoff.rs",
            "live_session/owner_loop/topology_phase.rs",
        ]
    );
    assert_eq!(
        containing(".resume_native_scanout"),
        ["live_session/owner_loop/renderer_image_handoff.rs"]
    );
}

#[test]
fn initial_activation_recovery_precedes_any_output_effect_dispatch() {
    // Startup retirement can discard only an undispatched effect. Keep that
    // ordering explicit: the owner loop has no device-free execution driver.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/live_session");
    let owner = fs::read_to_string(root.join("owner_loop.rs")).unwrap();
    assert!(
        owner
            .find("include!(\"owner_loop/initial_runtime.rs\")")
            .unwrap()
            < owner
                .find("include!(\"owner_loop/session_control.rs\")")
                .unwrap()
    );
    // The phase fragments form a nested include chain, rather than direct
    // children of owner_loop.rs. Check every link to the recovery prelude.
    for (parent, child) in [
        ("session_control.rs", "policy_input_phase.rs"),
        ("policy_input_phase.rs", "session_lock_phase.rs"),
        ("session_lock_phase.rs", "physical_input_phase.rs"),
        ("physical_input_phase.rs", "physical_input_loop.rs"),
    ] {
        let source = fs::read_to_string(root.join("owner_loop").join(parent)).unwrap();
        let include = format!("include!(\"{child}\")");
        assert_eq!(source.matches(&include).count(), 1, "{parent} -> {child}");
        assert_eq!(
            containing(&include),
            [format!("live_session/owner_loop/{parent}")]
        );
    }
    let physical = fs::read_to_string(root.join("owner_loop/physical_input_loop.rs")).unwrap();
    assert!(
        physical
            .find("initial_native_activation_failure.take()")
            .unwrap()
            < physical.find("include!(\"wm_phase.rs\")").unwrap()
    );
    assert_eq!(
        containing("wm.take_output_topology_effect()"),
        ["live_session/owner_loop/wm_phase.rs"]
    );
}
