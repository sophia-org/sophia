//! Which committed surface the session may propose for the initial focus.

use sophia_protocol::{BufferSource, CommittedSurfaceState, Rect, Region, SurfaceId};

use crate::live_session::{PersistentLiveLayout, initial_session_focus_candidate};

#[test]
fn a_committed_surface_is_only_a_focus_candidate_once_it_is_visible() {
    let surface = SurfaceId::new(41, 1);
    let committed = [CommittedSurfaceState {
        surface,
        committed_generation: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: 640,
            height: 480,
        },
        content: sophia_protocol::SurfaceContentSet::singleton(
            BufferSource::CpuBuffer { handle: 1 },
            sophia_protocol::Size {
                width: 640,
                height: 480,
            },
        ),
        damage: Region::empty(),
    }];

    // Committed is not visible: not mapped yet, or gone hidden. Proposing one
    // asks for a focus the protocol refuses, returned as a fatal rejection.
    let hidden = PersistentLiveLayout::default();
    let mut visible = PersistentLiveLayout::default();
    visible.mapped_surfaces.insert(surface);
    for (wm, held, layout, expected) in [
        (true, None, &hidden, None),
        (false, None, &hidden, None),
        (false, None, &visible, Some(surface)),
        (false, Some(surface), &visible, None),
    ] {
        assert_eq!(
            initial_session_focus_candidate(wm, held, &committed, layout),
            expected
        );
    }
}
