//! Descriptor and mapping accounting. One test in its own binary, so no
//! concurrent test opens descriptors or maps memory between counts.
use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
use sophia_isb::{IsbError, MappedIsb};

/// Open descriptors and mappings of ISB memfds in this process.
fn held() -> (usize, usize) {
    let fds = std::fs::read_dir("/proc/self/fd").unwrap().count();
    let maps = std::fs::read_to_string("/proc/self/maps")
        .unwrap()
        .lines()
        .filter(|line| line.contains("memfd:sophia-image"))
        .count();
    (fds, maps)
}

fn vm_size() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix("VmSize:"))
        .unwrap();
    line.trim()
        .trim_end_matches(" kB")
        .trim()
        .parse::<u64>()
        .unwrap()
        * 1024
}

#[test]
fn every_outcome_releases_exactly_what_it_created() {
    let baseline = held();
    assert_eq!(baseline.1, 0);

    // Refused before allocation: nothing was created.
    assert_eq!(MappedIsb::create(4, 4, 63).unwrap_err(), IsbError::Argument);
    assert_eq!(held(), baseline);
    // Above the slice-length and file-offset limit, with no resource bound.
    let limit = (isize::MAX as u64).min(i64::MAX as u64);
    let width = (1u32 << 30) - 1;
    let height = u32::try_from(limit / (u64::from(width) * 4) + 1).unwrap();
    assert_eq!(
        MappedIsb::create(width, height, u64::MAX).unwrap_err(),
        IsbError::Argument
    );
    assert_eq!(held(), baseline);

    // A live builder holds one descriptor and one mapping.
    let mut isb = MappedIsb::create(4, 4, 64).unwrap();
    isb.pixels_mut()[0] = 1;
    assert_eq!(held(), (baseline.0 + 1, 1));

    // Abandoning it unmaps and closes both.
    drop(isb);
    assert_eq!(held(), baseline);

    // Finishing unmaps and hands over only the descriptor.
    let fd = MappedIsb::create(4, 4, 64).unwrap().finish().unwrap();
    assert_eq!(held(), (baseline.0 + 1, 0));
    drop(fd);
    assert_eq!(held(), baseline);

    // A mapping refused after the memfd was created and allocated closes
    // the memfd. An address-space limit just above current use makes only
    // the 4 MiB mapping fail; nothing else allocates while it is lowered.
    let saved = getrlimit(Resource::As);
    let ceiling = vm_size() + 1024 * 1024;
    setrlimit(
        Resource::As,
        Rlimit {
            current: Some(ceiling),
            maximum: saved.maximum,
        },
    )
    .unwrap();
    let refused = MappedIsb::create(1024, 1024, u64::MAX);
    setrlimit(Resource::As, saved).unwrap();
    assert_eq!(
        refused.unwrap_err(),
        IsbError::Refused(rustix::io::Errno::NOMEM)
    );
    assert_eq!(held(), baseline);
}
