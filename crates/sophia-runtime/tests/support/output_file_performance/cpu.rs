use std::collections::BTreeMap;
use std::fs;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Task {
    name: String,
    cpu_ns: u64,
    voluntary: u64,
    involuntary: u64,
}

pub type Tasks = BTreeMap<(u32, u32), Task>;

/// Read every thread, including the test owner and libtest main thread. Missing
/// schedstat or disappearing tasks are errors rather than zero CPU samples.
pub fn snapshot(pids: &[u32]) -> Result<Tasks, String> {
    let mut tasks = Tasks::new();
    for &pid in pids {
        let directory = format!("/proc/{pid}/task");
        for entry in fs::read_dir(&directory).map_err(|e| format!("{directory}: {e}"))? {
            let entry = entry.map_err(|e| e.to_string())?;
            let tid = entry
                .file_name()
                .to_str()
                .ok_or("non-UTF8 task id")?
                .parse::<u32>()
                .map_err(|e| e.to_string())?;
            let path = entry.path();
            let sched = fs::read_to_string(path.join("schedstat"))
                .map_err(|e| format!("schedstat required for {pid}/{tid}: {e}"))?;
            let cpu_ns = sched
                .split_whitespace()
                .next()
                .ok_or("empty schedstat")?
                .parse::<u64>()
                .map_err(|e| e.to_string())?;
            let status = fs::read_to_string(path.join("status")).map_err(|e| e.to_string())?;
            let field = |key: &str| -> Result<&str, String> {
                status
                    .lines()
                    .find_map(|line| line.strip_prefix(key))
                    .map(str::trim)
                    .ok_or_else(|| format!("missing task field {key}"))
            };
            tasks.insert(
                (pid, tid),
                Task {
                    name: field("Name:")?.to_owned(),
                    cpu_ns,
                    voluntary: field("voluntary_ctxt_switches:")?
                        .parse::<u64>()
                        .map_err(|e| e.to_string())?,
                    involuntary: field("nonvoluntary_ctxt_switches:")?
                        .parse::<u64>()
                        .map_err(|e| e.to_string())?,
                },
            );
        }
        if !tasks.keys().any(|&(owner, _)| owner == pid) {
            return Err(format!("no tasks for process {pid}"));
        }
    }
    Ok(tasks)
}

pub fn interval(
    before: &Tasks,
    after: &Tasks,
    elapsed: Duration,
) -> Result<serde_json::Value, String> {
    if !before.keys().eq(after.keys()) {
        return Err("task membership changed during idle measurement".into());
    }
    if elapsed.is_zero() {
        return Err("zero idle interval".into());
    }
    let mut total = 0u64;
    let mut rows = Vec::new();
    for (key, first) in before {
        let last = &after[key];
        let delta = |a: u64, b: u64| b.checked_sub(a).ok_or("task counter decreased");
        let cpu_ns = delta(first.cpu_ns, last.cpu_ns)?;
        total = total.checked_add(cpu_ns).ok_or("CPU accounting overflow")?;
        rows.push(serde_json::json!({
            "pid": key.0, "tid": key.1, "name": last.name, "cpu_ns": cpu_ns,
            "voluntary_context_switches": delta(first.voluntary, last.voluntary)?,
            "involuntary_context_switches": delta(first.involuntary, last.involuntary)?,
        }));
    }
    // Integer arithmetic keeps the declared two-percent boundary exact.
    let passed = u128::from(total) * 100 <= elapsed.as_nanos() * 2;
    Ok(serde_json::json!({
        "elapsed_ns": elapsed.as_nanos().to_string(), "combined_cpu_ns": total,
        "combined_percent_one_core": total as f64 / elapsed.as_nanos() as f64 * 100.0,
        "passed": passed, "tasks": rows,
    }))
}

#[test]
fn combined_cpu_budget_includes_owner_and_peer() {
    let task = |cpu_ns| Task {
        name: "fixture".into(),
        cpu_ns,
        voluntary: 0,
        involuntary: 0,
    };
    let before = BTreeMap::from([((1, 1), task(0)), ((2, 2), task(0))]);
    let mut after = BTreeMap::from([((1, 1), task(100_000_000)), ((2, 2), task(100_000_000))]);
    assert_eq!(
        interval(&before, &after, Duration::from_secs(10)).unwrap()["passed"],
        true
    );
    after.get_mut(&(2, 2)).unwrap().cpu_ns += 1;
    assert_eq!(
        interval(&before, &after, Duration::from_secs(10)).unwrap()["passed"],
        false
    );
    after.remove(&(2, 2));
    assert!(interval(&before, &after, Duration::from_secs(10)).is_err());
}
