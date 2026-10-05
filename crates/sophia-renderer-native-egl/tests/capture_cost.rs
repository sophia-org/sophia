#![cfg(all(feature = "gbm-platform", target_os = "linux"))]
//! Fixed-cadence qualification harness. The identical file runs against the
//! baseline and candidate. Reuse modes are qualification-only controls.
//! It neither takes DRM master nor presents on a CRTC.
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
    let parameter = |name: &str, default: u32| -> u32 {
        std::env::var(name)
            .map(|s| s.parse().expect("integer capture parameter"))
            .unwrap_or(default)
    };
    let width = parameter("SOPHIA_CAPTURE_BENCH_WIDTH", 1280);
    let height = parameter("SOPHIA_CAPTURE_BENCH_HEIGHT", 1440);
    let rate = u64::from(parameter("SOPHIA_CAPTURE_BENCH_RATE", 60));
    assert!((1..=4096).contains(&width) && (1..=4096).contains(&height));
    assert!((1..=240).contains(&rate));
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
            width,
            height,
            gbm::Format::Xrgb8888,
            std::iter::once(gbm::Modifier::Linear),
            gbm::BufferObjectFlags::RENDERING,
        )
        .unwrap();
    let format = source.format();
    let modifier = u64::from(source.modifier());
    let fd = source.fd_for_plane(0).unwrap();
    native_dmabuf_cpu_write_access(&fd, false).unwrap();
    source
        .map_mut(0, 0, width, height, |mapped| mapped.buffer_mut().fill(0x80))
        .unwrap();
    native_dmabuf_cpu_write_access(&fd, true).unwrap();
    let mut context = NativeGbmRenderedScanoutContext::from_backend_device_result(Ok(open()))
        .context
        .unwrap();
    // Qualification only. These existing CPU/wall clocks stay off in ordinary
    // rendering. Copy includes import and submission, not a GPU-duration query.
    context.set_render_timing_enabled(true);
    let mode = std::env::var("SOPHIA_CAPTURE_REUSE_MODE").unwrap_or_else(|_| "reuse".into());
    match mode.as_str() {
        "fresh" => context.set_snapshot_reuse_enabled(false),
        "pool" => context.set_snapshot_reuse_modes(true, false, true),
        "reuse" => context.set_snapshot_reuse_enabled(true),
        _ => panic!("capture reuse mode must be fresh, pool, or reuse"),
    }
    let mut serial = 0;
    let mut capture = |context: &mut NativeGbmRenderedScanoutContext<_>| {
        serial += 1;
        let image = NativeRendererImageId::from_raw(serial);
        let capture_started = cpu(rustix::time::ClockId::ThreadCPUTime);
        context
            .capture_renderer_image(
                image,
                NativeMultiPlaneDmaBufFrame {
                    width,
                    height,
                    format: format as u32,
                    modifier,
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
        let promote_started = cpu(rustix::time::ClockId::ThreadCPUTime);
        context.promote_renderer_image(image).unwrap();
        let evict_started = cpu(rustix::time::ClockId::ThreadCPUTime);
        context.evict_renderer_image(image).unwrap();
        let finished = cpu(rustix::time::ClockId::ThreadCPUTime);
        [
            promote_started - capture_started,
            evict_started - promote_started,
            finished - evict_started,
        ]
    };
    for _ in 0..120 {
        capture(&mut context);
    }
    let warm = context.persistent_render_stats();
    let warm_reuse = context.snapshot_reuse_stats();
    let start = Instant::now();
    let thread_start = cpu(rustix::time::ClockId::ThreadCPUTime);
    let process_start = cpu(rustix::time::ClockId::ProcessCPUTime);
    let frames = seconds * rate;
    let mut samples = Vec::with_capacity(frames as usize);
    let mut late_frames = 0;
    let mut calls_cpu = [0u128; 3];
    for index in 0..frames {
        let before = Instant::now();
        for (total, elapsed) in calls_cpu.iter_mut().zip(capture(&mut context)) {
            *total += elapsed;
        }
        samples.push(before.elapsed().as_nanos());
        let next = start + Duration::from_nanos((index + 1) * 1_000_000_000 / rate);
        late_frames += usize::from(Instant::now() > next);
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
    }
    let thread_ns = cpu(rustix::time::ClockId::ThreadCPUTime) - thread_start;
    let process_ns = cpu(rustix::time::ClockId::ProcessCPUTime) - process_start;
    let elapsed = start.elapsed().as_nanos();
    let stats = context.persistent_render_stats();
    let reuse = context.snapshot_reuse_stats();
    assert_eq!(stats.snapshot_live_entries, 0);
    assert_eq!(stats.snapshot_live_bytes, 0);
    assert_eq!(stats.import_cache.live_entries, 0);
    assert_eq!(
        stats.snapshot_captures - warm.snapshot_captures,
        frames as usize
    );
    assert_eq!(
        stats.snapshot_promotions - warm.snapshot_promotions,
        frames as usize
    );
    assert_eq!(
        stats.snapshot_evictions - warm.snapshot_evictions,
        frames as usize
    );
    assert_eq!(stats.capture_failures, warm.capture_failures);
    assert_eq!(stats.transfer_failures, warm.transfer_failures);
    let setup_cpu = (stats.capture_setup_cpu - warm.capture_setup_cpu).as_nanos();
    let copy_cpu = (stats.capture_copy_cpu - warm.capture_copy_cpu).as_nanos();
    let cleanup_cpu = (stats.capture_cleanup_cpu - warm.capture_cleanup_cpu).as_nanos();
    assert!(setup_cpu > 0 && copy_cpu > 0 && cleanup_cpu > 0);
    assert!(setup_cpu + copy_cpu + cleanup_cpu <= thread_ns);
    assert!(setup_cpu + copy_cpu + cleanup_cpu <= calls_cpu[0]);
    assert!(calls_cpu.iter().sum::<u128>() <= thread_ns);
    println!(
        "capture_calls capture_cpu_ns={} promote_cpu_ns={} evict_cpu_ns={}",
        calls_cpu[0], calls_cpu[1], calls_cpu[2]
    );
    // The fresh path's validation import is outside these stage counters.
    // Pooled source imports are reported separately below. Total CPU includes all.
    if mode != "fresh" {
        assert_eq!(
            reuse.allocations, warm_reuse.allocations,
            "warm pool allocates no new storage"
        );
        assert!(reuse.reuses > warm_reuse.reuses);
        if mode == "reuse" {
            assert_eq!(
                reuse.source_imports, warm_reuse.source_imports,
                "warm source import stays alive"
            );
        }
    }
    println!(
        "capture_reuse mode={mode} allocations={} reused={} source_imports={} source_hits={} source_rebinds={} retained_bytes={} source_bytes={} idle_count={}",
        reuse.allocations - warm_reuse.allocations,
        reuse.reuses - warm_reuse.reuses,
        reuse.source_imports - warm_reuse.source_imports,
        reuse.source_hits - warm_reuse.source_hits,
        reuse.source_rebinds - warm_reuse.source_rebinds,
        reuse.live_bytes,
        reuse.source_bytes,
        reuse.idle_count
    );
    println!(
        "capture_stages timing=true setup_cpu_ns={setup_cpu} copy_cpu_ns={copy_cpu} \
         cleanup_cpu_ns={cleanup_cpu} setup_elapsed_ns={} copy_elapsed_ns={} \
         cleanup_elapsed_ns={} contexts={} context_reuses={} surfaces={} imports={} \
         hits={} evictions={} transfers={} late_frames={late_frames}",
        (stats.capture_setup_elapsed - warm.capture_setup_elapsed).as_nanos(),
        (stats.capture_copy_elapsed - warm.capture_copy_elapsed).as_nanos(),
        (stats.capture_cleanup_elapsed - warm.capture_cleanup_elapsed).as_nanos(),
        stats.capture_context_creations - warm.capture_context_creations,
        stats.capture_context_reuses - warm.capture_context_reuses,
        stats.capture_surface_creations - warm.capture_surface_creations,
        stats.import_cache.imports - warm.import_cache.imports,
        stats.import_cache.hits - warm.import_cache.hits,
        stats.import_cache.evictions - warm.import_cache.evictions,
        stats.transfer_captures - warm.transfer_captures,
    );
    println!(
        "capture_cost width={width} height={height} rate={rate} format={format} modifier={modifier} frames={frames} elapsed_ns={elapsed} thread_cpu_ns={thread_ns} process_cpu_ns={process_ns} pipelines={} samples_ns={samples:?}",
        stats.gl_pipeline_creations - warm.gl_pipeline_creations
    );
}
