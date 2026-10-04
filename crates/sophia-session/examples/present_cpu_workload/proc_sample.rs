//! Explicit Session/client samples, plus optional guest-only CPU attribution.
use super::Result;
use serde_json::{Value, json};
use std::fs;

pub fn process(pid: u32) -> Result<Value> {
    let base = format!("/proc/{pid}");
    let stat = fs::read_to_string(format!("{base}/stat"))?;
    let fields = stat_fields(&stat)?;
    let mut threads = Vec::new();
    for entry in fs::read_dir(format!("{base}/task"))? {
        let path = entry?.path();
        // Thread exit during a snapshot is observable; do not manufacture a delta for it.
        let Ok(stat) = fs::read_to_string(path.join("stat")) else {
            continue;
        };
        let task = stat_fields(&stat)?;
        let Ok(status) = fs::read_to_string(path.join("status")) else {
            continue;
        };
        let Ok(schedstat) = fs::read_to_string(path.join("schedstat")) else {
            continue;
        };
        let sched: Vec<_> = schedstat.split_whitespace().collect();
        let value = |key: &str| -> Result<u64> {
            Ok(status
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .ok_or("missing context-switch counter")?
                .trim()
                .parse()?)
        };
        threads.push(
            json!({"tid": path.file_name().unwrap().to_str().unwrap().parse::<u32>()?,
            "name": stat.split_once('(').unwrap().1.rsplit_once(')').unwrap().0,
            "start_ticks": task[19].parse::<u64>()?, "runtime_ns": sched[0].parse::<u64>()?,
            "runqueue_ns": sched[1].parse::<u64>()?,
            "voluntary_switches": value("voluntary_ctxt_switches:")?,
            "involuntary_switches": value("nonvoluntary_ctxt_switches:")?}),
        );
    }
    threads.sort_by_key(|t| t["tid"].as_u64());
    Ok(
        json!({"pid": pid, "exe": fs::read_link(format!("{base}/exe"))?,
        "start_ticks": fields[19].parse::<u64>()?, "user_ticks": fields[11].parse::<u64>()?,
        "system_ticks": fields[12].parse::<u64>()?, "threads": threads,
        "clock_ticks_per_second": rustix::param::clock_ticks_per_second()}),
    )
}
fn stat_fields(stat: &str) -> Result<Vec<&str>> {
    let fields: Vec<_> = stat
        .rsplit_once(')')
        .ok_or("malformed /proc stat")?
        .1
        .split_whitespace()
        .collect();
    if fields.len() < 20 {
        return Err("short /proc stat".into());
    }
    Ok(fields)
}
fn guest_processes() -> Result<Vec<Value>> {
    let mut rows = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let path = entry?.path();
        let Some(pid) = path
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let stat = match fs::read_to_string(path.join("stat")) {
            Ok(stat) => stat,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        let fields = stat_fields(&stat)?;
        let mut tasks = Vec::new();
        for task in fs::read_dir(path.join("task"))? {
            let task = task?.path();
            let stat = match fs::read_to_string(task.join("stat")) {
                Ok(stat) => stat,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let fields = stat_fields(&stat)?;
            let schedstat = match fs::read_to_string(task.join("schedstat")) {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            tasks.push(json!({"tid": task.file_name().unwrap().to_str().unwrap().parse::<u32>()?,
                "start_ticks": fields[19].parse::<u64>()?,
                "runtime_ns": schedstat.split_whitespace().next().ok_or("short guest schedstat")?.parse::<u64>()?}));
            if tasks.len() > 1024 {
                return Err("guest process exceeded 1024 tasks".into());
            }
        }
        rows.push(json!({"pid": pid, "ppid": fields[1].parse::<u32>()?,
            "name": stat.split_once('(').ok_or("missing process name")?.1.rsplit_once(')').unwrap().0,
            "start_ticks": fields[19].parse::<u64>()?, "user_ticks": fields[11].parse::<u64>()?,
            "system_ticks": fields[12].parse::<u64>()?, "tasks": tasks}));
        if rows.len() > 1024 {
            return Err("guest process accounting exceeded 1024 processes".into());
        }
    }
    rows.sort_by_key(|row| row["pid"].as_u64());
    Ok(rows)
}
pub fn snapshot(session: Option<u32>, sample_guest_processes: bool) -> Result<Value> {
    Ok(
        json!({"monotonic_usec": super::now_usec(), "client": process(std::process::id())?,
        "session": session.map(process).transpose()?,
        "guest_stat": fs::read_to_string("/proc/stat")?,
        "guest_schedstat": fs::read_to_string("/proc/schedstat")?,
        "guest_meminfo": fs::read_to_string("/proc/meminfo")?,
        "guest_log_bytes": if sample_guest_processes {
            Some(fs::metadata("/run/present-cpu/session.log")?.len())
        } else { None },
        "guest_processes": sample_guest_processes.then(guest_processes).transpose()?,
        "loadavg": fs::read_to_string("/proc/loadavg")?}),
    )
}
