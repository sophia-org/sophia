#![cfg(all(feature = "gbm-platform", target_os = "linux"))]
//! Fixed-cadence qualification harness. The identical file runs against the
//! baseline and candidate. It neither takes DRM master nor presents on a CRTC.
use sophia_renderer_native_egl::{
    NativeDmaBufPlane, NativeGbmRenderedScanoutContext, NativeMultiPlaneDmaBufFrame,
    NativeRendererImageId, native_dmabuf_cpu_write_access,
};
use std::{
    fs::OpenOptions,
    os::fd::AsFd,
    time::{Duration, Instant},
};

fn cpu(clock: rustix::time::ClockId) -> u128 {
    let value = rustix::time::clock_gettime(clock);
    value.tv_sec as u128 * 1_000_000_000 + value.tv_nsec as u128
}

#[test]
#[ignore = "qualification: needs render node and explicit duration"]
fn capture_cost_at_fixed_cadence() {
    let node = std::env::var_os("SOPHIA_TEST_RENDER_NODE").expect("render node");
    let seconds: u64 = std::env::var("SOPHIA_CAPTURE_BENCH_SECONDS")
        .expect("explicit duration")
        .parse()
        .unwrap();
    assert!((1..=120).contains(&seconds));
    let open = || {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&node)
            .unwrap()
    };
    let allocator = gbm::Device::new(open()).unwrap();
    let mut source = allocator
        .create_buffer_object_with_modifiers2::<()>(
            1280,
            1440,
            gbm::Format::Xrgb8888,
            std::iter::once(gbm::Modifier::Linear),
            gbm::BufferObjectFlags::RENDERING,
        )
        .unwrap();
    let fd = source.fd_for_plane(0).unwrap();
    native_dmabuf_cpu_write_access(&fd, false).unwrap();
    source
        .map_mut(0, 0, 1280, 1440, |mapped| mapped.buffer_mut().fill(0x80))
        .unwrap();
    native_dmabuf_cpu_write_access(&fd, true).unwrap();
    let mut context = NativeGbmRenderedScanoutContext::from_backend_device_result(Ok(open()))
        .context
        .unwrap();
    let mut serial = 0;
    let mut capture = |context: &mut NativeGbmRenderedScanoutContext<_>| {
        serial += 1;
        let image = NativeRendererImageId::from_raw(serial);
        context
            .capture_renderer_image(
                image,
                NativeMultiPlaneDmaBufFrame {
                    width: 1280,
                    height: 1440,
                    format: gbm::Format::Xrgb8888 as u32,
                    modifier: u64::from(source.modifier()),
                    plane_count: 1,
                    planes: [
                        Some(NativeDmaBufPlane {
                            fd: fd.as_fd(),
                            offset: source.offset(0),
                            stride: source.stride_for_plane(0),
                        }),
                        None,
                        None,
                        None,
                    ],
                },
            )
            .unwrap();
        context.promote_renderer_image(image).unwrap();
        context.evict_renderer_image(image).unwrap();
    };
    for _ in 0..120 {
        capture(&mut context);
    }
    let warm = context.persistent_render_stats();
    let start = Instant::now();
    let thread_start = cpu(rustix::time::ClockId::ThreadCPUTime);
    let process_start = cpu(rustix::time::ClockId::ProcessCPUTime);
    let frames = seconds * 60;
    let mut samples = Vec::with_capacity(frames as usize);
    for index in 0..frames {
        let before = Instant::now();
        capture(&mut context);
        samples.push(before.elapsed().as_nanos());
        let next = start + Duration::from_nanos((index + 1) * 1_000_000_000 / 60);
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    let thread_ns = cpu(rustix::time::ClockId::ThreadCPUTime) - thread_start;
    let process_ns = cpu(rustix::time::ClockId::ProcessCPUTime) - process_start;
    let elapsed = start.elapsed().as_nanos();
    let stats = context.persistent_render_stats();
    assert_eq!(stats.snapshot_live_entries, 0);
    assert_eq!(
        stats.snapshot_captures - warm.snapshot_captures,
        frames as usize
    );
    println!(
        "capture_cost frames={frames} elapsed_ns={elapsed} thread_cpu_ns={thread_ns} process_cpu_ns={process_ns} pipelines={} samples_ns={samples:?}",
        stats.gl_pipeline_creations - warm.gl_pipeline_creations
    );
}
