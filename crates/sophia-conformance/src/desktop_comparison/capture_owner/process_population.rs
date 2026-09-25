/// Sample overlapping populations in one `/proc` pass.
///
/// Stack and workload totals are views over the aggregate population. Reading
/// every `smaps_rollup` three times would add avoidable controller work to the
/// measurement it is trying not to perturb. The returned PID/start pairs are
/// the exact members of the final roots entry, retained transiently so teardown
/// still owns a descendant after it changes parent or process group.
fn sample_process_populations<const N: usize>(
    proc_root: &Path,
    roots: [&BTreeSet<u32>; N],
    workload: &WorkloadOwner,
) -> Result<ResourceSnapshotsWithWindows<N>, String> {
    if N == 0 {
        return Err("process sampling requires at least one population".to_owned());
    }
    let processes = read_process_table(proc_root)?;
    if roots
        .iter()
        .any(|population| population.iter().any(|pid| !processes.contains_key(pid)))
    {
        return Err("a sampled process root disappeared".to_owned());
    }

    let adopted_workload_roots = workload.adopted_population_roots(&processes);
    let mut totals = [ResourceSnapshot::default(); N];
    let mut last_population = Vec::new();
    for pid in processes.keys().copied() {
        let mut membership =
            std::array::from_fn::<_, N, _>(|index| descends_from(pid, roots[index], &processes));
        if descends_from(pid, &adopted_workload_roots, &processes) {
            membership[0] = true;
            membership[N - 1] = true;
        }
        if !membership.iter().any(|included| *included) {
            continue;
        }
        let Some(stat) = processes.get(&pid) else {
            continue;
        };
        if membership.last().copied().unwrap_or(false) {
            last_population.push((pid, stat.start_ticks));
        }
        let process = proc_root.join(pid.to_string());
        let memory = match fs::read_to_string(process.join("smaps_rollup")) {
            Ok(memory) => memory,
            Err(error) if roots.iter().any(|population| population.contains(&pid)) => {
                return Err(format!(
                    "sampled process root {pid} has no readable memory population: {error}"
                ));
            }
            Err(_) => continue,
        };
        let fd_count = match fs::read_dir(process.join("fd")) {
            Ok(entries) => entries.filter_map(Result::ok).count(),
            Err(error) if roots.iter().any(|population| population.contains(&pid)) => {
                return Err(format!(
                    "sampled process root {pid} has no readable fd population: {error}"
                ));
            }
            Err(_) => continue,
        };
        for (index, included) in membership.into_iter().enumerate() {
            if !included {
                continue;
            }
            let total = &mut totals[index];
            total.processes = total.processes.saturating_add(1);
            total.pss_kib = total.pss_kib.saturating_add(memory_kib(&memory, "Pss:"));
            total.rss_kib = total.rss_kib.saturating_add(memory_kib(&memory, "Rss:"));
            total.anonymous_kib = total
                .anonymous_kib
                .saturating_add(memory_kib(&memory, "Anonymous:"));
            total.private_dirty_kib = total
                .private_dirty_kib
                .saturating_add(memory_kib(&memory, "Private_Dirty:"));
            total.cpu_ticks = total.cpu_ticks.saturating_add(stat.cpu_ticks);
            total.minor_faults = total.minor_faults.saturating_add(stat.minor_faults);
            total.major_faults = total.major_faults.saturating_add(stat.major_faults);
            total.threads = total.threads.saturating_add(stat.threads);
            total.fds = total
                .fds
                .saturating_add(u64::try_from(fd_count).unwrap_or(u64::MAX));
        }
    }
    if totals.iter().any(|total| {
        total.processes == 0
            || total.pss_kib == 0
            || total.rss_kib == 0
            || total.threads == 0
            || total.fds == 0
    }) {
        return Err("sampled process population is empty or unreadable".to_owned());
    }
    Ok((totals, last_population))
}

pub(super) fn read_process_table(proc_root: &Path) -> Result<BTreeMap<u32, ProcStat>, String> {
    let mut processes = BTreeMap::new();
    for entry in fs::read_dir(proc_root)
        .map_err(|error| format!("could not enumerate process population: {error}"))?
    {
        let entry = entry.map_err(|error| format!("could not read process entry: {error}"))?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(source) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        if let Ok(stat) = parse_proc_stat(&source) {
            processes.insert(pid, stat);
        }
    }
    Ok(processes)
}

fn descends_from(mut pid: u32, roots: &BTreeSet<u32>, processes: &BTreeMap<u32, ProcStat>) -> bool {
    for _ in 0..=processes.len() {
        if roots.contains(&pid) {
            return true;
        }
        let Some(stat) = processes.get(&pid) else {
            return false;
        };
        if stat.ppid == 0 || stat.ppid == pid {
            return false;
        }
        pid = stat.ppid;
    }
    false
}

pub(super) fn parse_proc_stat(source: &str) -> Result<ProcStat, String> {
    let close = source
        .rfind(')')
        .ok_or("process stat lacks a command terminator")?;
    let fields = source[close + 1..]
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    let number = |index: usize, name: &str| {
        fields
            .get(index)
            .ok_or_else(|| format!("process stat lacks {name}"))?
            .parse::<u64>()
            .map_err(|_| format!("process stat {name} is not an integer"))
    };
    Ok(ProcStat {
        ppid: u32::try_from(number(1, "ppid")?).map_err(|_| "process ppid is too large")?,
        minor_faults: number(7, "minor_faults")?,
        major_faults: number(9, "major_faults")?,
        cpu_ticks: number(11, "utime")?.saturating_add(number(12, "stime")?),
        threads: number(17, "threads")?,
        start_ticks: number(19, "start_ticks")?,
    })
}

fn memory_kib(source: &str, field: &str) -> u64 {
    source
        .lines()
        .find_map(|line| line.strip_prefix(field))
        .and_then(|rest| rest.split_ascii_whitespace().next())
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn clock_ticks_per_second() -> Result<u64, String> {
    let output = Command::new("getconf")
        .arg("CLK_TCK")
        .output()
        .map_err(|error| format!("could not query process clock rate: {error}"))?;
    if !output.status.success() {
        return Err("getconf CLK_TCK failed".to_owned());
    }
    String::from_utf8(output.stdout)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| "getconf CLK_TCK did not return a positive integer".to_owned())
}
