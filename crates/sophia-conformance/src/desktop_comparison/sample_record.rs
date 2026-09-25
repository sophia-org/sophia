fn parse_sample(source: &str, candidate: &str) -> Result<Sample, String> {
    let records = source
        .lines()
        .filter(|line| {
            line.starts_with("desktop_comparison_sample schema=1 status=complete ")
                || line.starts_with("desktop_comparison_sample schema=2 status=complete ")
                || line.starts_with("desktop_comparison_sample schema=3 status=complete ")
                || line.starts_with("desktop_comparison_sample schema=4 status=complete ")
        })
        .collect::<Vec<_>>();
    if records.len() != 1 {
        return Err(format!(
            "sample log requires exactly one completion record; found {}",
            records.len()
        ));
    }
    let fields = fields(records[0])?;
    let schema = fields.get("schema").copied().unwrap_or_default();
    let required = |name| {
        fields
            .get(name)
            .copied()
            .ok_or_else(|| format!("sample record lacks {name}"))
    };
    let stack = required("stack")?;
    if !STACKS.contains(&stack) {
        return Err(format!("unknown comparison stack {stack:?}"));
    }
    let workload = required("workload")?;
    if !SHORT_WORKLOADS.contains(&workload) && workload != "soak-2h" {
        return Err(format!("unknown comparison workload {workload:?}"));
    }
    let expected_version = match stack {
        "sophia" => candidate,
        "xlibre-xmonad" => XLIBRE_COMMIT,
        "niri" => NIRI_VERSION,
        _ => unreachable!(),
    };
    for (name, expected) in [
        ("backend", "native"),
        ("topology", TOPOLOGY),
        ("kitty", KITTY_VERSION),
        ("firefox", FIREFOX_VERSION),
        ("stack_version", expected_version),
        ("crashes", "0"),
        ("sample_loss", "0"),
    ] {
        if required(name)? != expected {
            return Err(format!(
                "sample {name} does not match the prepared contract"
            ));
        }
    }
    if matches!(schema, "2" | "3" | "4") {
        for (name, expected) in [("frame_source", "kernel_drm"), ("teardown", "clean")] {
            if required(name)? != expected {
                return Err(format!(
                    "sample {name} does not match the raw-capture contract"
                ));
            }
        }
    }
    let numeric = |name| {
        required(name)?
            .parse::<u64>()
            .map_err(|_| format!("sample {name} is not an integer"))
    };
    let compatible_numeric = |name: &str, fallback: u64| {
        fields.get(name).map_or(Ok(fallback), |value| {
            value
                .parse::<u64>()
                .map_err(|_| format!("sample {name} is not an integer"))
        })
    };
    if matches!(schema, "2" | "3" | "4") {
        for name in [
            "resource_samples",
            "anonymous_peak_kib",
            "private_dirty_peak_kib",
            "minor_faults",
            "major_faults",
            "resize_samples",
            "resize_p50_usec",
            "resize_p95_usec",
            "resize_p99_usec",
            "resize_max_usec",
            "frame_p50_usec",
            "frame_p95_usec",
            "frame_p99_usec",
            "frame_max_usec",
            "native_samples",
        ] {
            let _ = numeric(name)?;
        }
        let _ = required("native_timing")?;
        let _ = required("native_source")?;
    }
    if matches!(schema, "3" | "4") {
        for (name, expected) in [
            ("controller_outside_supervisor", "true"),
            ("visible_dp1", "true"),
            ("focused_owned", "true"),
            ("foreign_toplevels", "0"),
        ] {
            if required(name)? != expected {
                return Err(format!(
                    "sample {name} does not match the visibility contract"
                ));
            }
        }
        if numeric("visibility_samples")? == 0 {
            return Err("sample visibility_samples must be positive".to_owned());
        }
    }
    let duration_msec = numeric("duration_msec")?;
    let minimum_duration = match workload {
        "kitty-60s" => 60_000,
        "soak-2h" => 7_200_000,
        _ => 1,
    };
    if duration_msec < minimum_duration {
        return Err(format!("sample duration is below {minimum_duration}ms"));
    }
    for name in [
        "processes",
        "pss_peak_kib",
        "rss_peak_kib",
        "threads_peak",
        "fds_peak",
        "frame_samples",
        "frame_mean_usec",
    ] {
        if numeric(name)? == 0 {
            return Err(format!("sample {name} must be positive"));
        }
    }
    for name in ["cpu_msec", "launch_msec", "settle_msec", "resize_msec"] {
        let _ = numeric(name)?;
    }
    if schema == "4" {
        for name in [
            "stack_processes",
            "stack_pss_peak_kib",
            "stack_rss_peak_kib",
            "stack_threads_peak",
            "stack_fds_peak",
            "workload_processes",
            "workload_pss_peak_kib",
            "workload_rss_peak_kib",
            "workload_threads_peak",
            "workload_fds_peak",
            "frame_deliveries",
        ] {
            if numeric(name)? == 0 {
                return Err(format!("sample {name} must be positive"));
            }
        }
        for name in ["stack_cpu_msec", "workload_cpu_msec", "frame_duplicates"] {
            let _ = numeric(name)?;
        }
        if numeric("frame_deliveries")? < numeric("frame_samples")?.saturating_add(1) {
            return Err(
                "sample frame delivery population is smaller than unique frames".to_owned(),
            );
        }
    }
    Ok(Sample {
        scheduled: ScheduledSample {
            order: usize::try_from(numeric("order")?).map_err(|_| "sample order is too large")?,
            stack: stack.to_owned(),
            workload: workload.to_owned(),
            repetition: u8::try_from(numeric("repetition")?)
                .map_err(|_| "sample repetition is too large")?,
        },
        duration_msec,
        processes: numeric("processes")?,
        pss_peak_kib: numeric("pss_peak_kib")?,
        rss_peak_kib: numeric("rss_peak_kib")?,
        anonymous_peak_kib: compatible_numeric("anonymous_peak_kib", 0)?,
        private_dirty_peak_kib: compatible_numeric("private_dirty_peak_kib", 0)?,
        cpu_msec: numeric("cpu_msec")?,
        minor_faults: compatible_numeric("minor_faults", 0)?,
        major_faults: compatible_numeric("major_faults", 0)?,
        threads_peak: numeric("threads_peak")?,
        fds_peak: numeric("fds_peak")?,
        stack_processes: compatible_numeric("stack_processes", 0)?,
        stack_pss_peak_kib: compatible_numeric("stack_pss_peak_kib", 0)?,
        stack_rss_peak_kib: compatible_numeric("stack_rss_peak_kib", 0)?,
        stack_cpu_msec: compatible_numeric("stack_cpu_msec", 0)?,
        stack_threads_peak: compatible_numeric("stack_threads_peak", 0)?,
        stack_fds_peak: compatible_numeric("stack_fds_peak", 0)?,
        workload_processes: compatible_numeric("workload_processes", 0)?,
        workload_pss_peak_kib: compatible_numeric("workload_pss_peak_kib", 0)?,
        workload_rss_peak_kib: compatible_numeric("workload_rss_peak_kib", 0)?,
        workload_cpu_msec: compatible_numeric("workload_cpu_msec", 0)?,
        workload_threads_peak: compatible_numeric("workload_threads_peak", 0)?,
        workload_fds_peak: compatible_numeric("workload_fds_peak", 0)?,
        launch_msec: numeric("launch_msec")?,
        settle_msec: numeric("settle_msec")?,
        resize_msec: numeric("resize_msec")?,
        frame_mean_usec: numeric("frame_mean_usec")?,
        frame_p50_usec: compatible_numeric("frame_p50_usec", numeric("frame_mean_usec")?)?,
        frame_p95_usec: compatible_numeric("frame_p95_usec", numeric("frame_mean_usec")?)?,
        frame_p99_usec: compatible_numeric("frame_p99_usec", numeric("frame_mean_usec")?)?,
        frame_max_usec: compatible_numeric("frame_max_usec", numeric("frame_mean_usec")?)?,
        frame_deliveries: compatible_numeric(
            "frame_deliveries",
            numeric("frame_samples")?.saturating_add(1),
        )?,
        frame_duplicates: compatible_numeric("frame_duplicates", 0)?,
    })
}

fn fields(line: &str) -> Result<BTreeMap<&str, &str>, String> {
    let mut fields = BTreeMap::new();
    for token in line.split_ascii_whitespace() {
        let Some((name, value)) = token.split_once('=') else {
            continue;
        };
        if fields.insert(name, value).is_some() {
            return Err(format!("desktop comparison record repeats field {name}"));
        }
    }
    Ok(fields)
}
