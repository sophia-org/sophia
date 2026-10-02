//! Same fixed-cadence workload on the baseline and candidate, with three CPU
//! backings held as if they were in flight while only GPU generations advance.
use sophia_engine::{CompositorDisplayCommand, CompositorDisplayList, HeadlessOutput};
use sophia_protocol::{
    BufferSource, CommittedSurfaceState, OutputId, Rect, Region, Size, SurfaceContentSet, SurfaceId,
};
use sophia_renderer_live::LiveProductionCpuScene;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

fn cpu(clock: rustix::time::ClockId) -> u128 {
    let value = rustix::time::clock_gettime(clock);
    value.tv_sec as u128 * 1_000_000_000 + value.tv_nsec as u128
}

#[test]
#[ignore = "qualification: requires explicit duration"]
fn cpu_scene_cost_at_fixed_cadence() {
    let seconds: u64 = std::env::var("SOPHIA_CAPTURE_BENCH_SECONDS")
        .expect("duration")
        .parse()
        .unwrap();
    assert!((1..=120).contains(&seconds));
    let size = Size {
        width: 1280,
        height: 1440,
    };
    let output = HeadlessOutput {
        id: OutputId::from_raw(1),
        size,
        scale: 1,
    };
    let mut state = CommittedSurfaceState {
        surface: SurfaceId::new(1, 1),
        committed_generation: 1,
        geometry: Rect {
            x: 0,
            y: 0,
            width: size.width,
            height: size.height,
        },
        content: SurfaceContentSet::singleton(BufferSource::DmaBuf { handle: 1 }, size),
        damage: Region::empty(),
    };
    let list = CompositorDisplayList {
        output: output.id,
        commands: vec![CompositorDisplayCommand::Surface {
            surface: state.surface,
        }],
    };
    let mut scene = LiveProductionCpuScene::new(size);
    let mut busy = VecDeque::new();
    let mut render = || {
        state.committed_generation += 1;
        state.content = SurfaceContentSet::singleton(
            BufferSource::DmaBuf {
                handle: state.committed_generation,
            },
            size,
        );
        busy.push_back(
            scene
                .compose_display_list(output, std::slice::from_ref(&state), &list, None)
                .unwrap()
                .frame
                .bytes
                .clone(),
        );
        if busy.len() > 3 {
            busy.pop_front();
        }
    };
    for _ in 0..120 {
        render();
    }
    let start = Instant::now();
    let thread_start = cpu(rustix::time::ClockId::ThreadCPUTime);
    let process_start = cpu(rustix::time::ClockId::ProcessCPUTime);
    let frames = seconds * 60;
    let mut samples = Vec::with_capacity(frames as usize);
    for index in 0..frames {
        let before = Instant::now();
        render();
        samples.push(before.elapsed().as_nanos());
        let next = start + Duration::from_nanos((index + 1) * 1_000_000_000 / 60);
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    let thread_ns = cpu(rustix::time::ClockId::ThreadCPUTime) - thread_start;
    let process_ns = cpu(rustix::time::ClockId::ProcessCPUTime) - process_start;
    let elapsed = start.elapsed().as_nanos();
    println!(
        "cpu_scene_cost frames={frames} elapsed_ns={elapsed} thread_cpu_ns={thread_ns} process_cpu_ns={process_ns} samples_ns={samples:?}"
    );
}
