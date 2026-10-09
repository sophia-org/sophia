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
