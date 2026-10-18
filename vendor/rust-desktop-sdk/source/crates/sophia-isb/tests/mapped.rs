use rustix::fs::{
    FallocateFlags, SealFlags, fallocate, fcntl_add_seals, fcntl_get_seals, fstat, ftruncate,
};
use rustix::io::{Errno, pread, pwrite};
use sophia_isb::{IsbError, MappedIsb, SEALS};

#[test]
fn exact_pixels_and_length_survive_sealing() {
    let mut isb = MappedIsb::create(3, 2, 24).unwrap();
    assert_eq!((isb.width(), isb.height(), isb.stride()), (3, 2, 12));
    assert_eq!(isb.len(), 24);
    assert!(
        isb.pixels().iter().all(|byte| *byte == 0),
        "fresh backing is zeroed"
    );
    let pattern: Vec<u8> = (0..24).map(|i| i * 7 + 1).collect();
    isb.pixels_mut().copy_from_slice(&pattern);
    assert_eq!(isb.pixels(), pattern.as_slice());
    let fd = isb.finish().unwrap();
    assert_eq!(fstat(&fd).unwrap().st_size, 24);
    let mut back = [0u8; 25];
    assert_eq!(pread(&fd, &mut back, 0).unwrap(), 24);
    assert_eq!(&back[..24], pattern.as_slice());
}

#[test]
fn finish_verifies_every_seal_and_the_backing_refuses_change() {
    let isb = MappedIsb::create(2, 2, 16).unwrap();
    let fd = isb.finish().unwrap();
    assert!(fcntl_get_seals(&fd).unwrap().contains(SEALS));
    assert_eq!(pwrite(&fd, &[9], 0), Err(Errno::PERM), "WRITE");
    assert_eq!(ftruncate(&fd, 32), Err(Errno::PERM), "GROW");
    assert_eq!(ftruncate(&fd, 8), Err(Errno::PERM), "SHRINK");
    assert_eq!(
        fallocate(&fd, FallocateFlags::empty(), 0, 32),
        Err(Errno::PERM),
        "GROW by allocation"
    );
    assert_eq!(
        fcntl_add_seals(&fd, SealFlags::FUTURE_WRITE),
        Err(Errno::PERM),
        "SEAL"
    );
    let mut back = [1u8; 16];
    assert_eq!(pread(&fd, &mut back, 0).unwrap(), 16);
    assert_eq!(back, [0; 16]);
}

#[test]
fn dimensions_and_the_resource_bound_are_checked_before_allocation() {
    assert_eq!(
        MappedIsb::create(0, 1, u64::MAX).unwrap_err(),
        IsbError::Argument
    );
    assert_eq!(
        MappedIsb::create(1, 0, u64::MAX).unwrap_err(),
        IsbError::Argument
    );
    // One byte under the exact size is refused; the exact size is accepted.
    assert_eq!(MappedIsb::create(4, 2, 31).unwrap_err(), IsbError::Argument);
    assert_eq!(MappedIsb::create(4, 2, 32).unwrap().len(), 32);
    // A stride that does not fit u32, and a size that overflows u64, are
    // refused even with an unlimited bound.
    assert_eq!(
        MappedIsb::create(1 << 30, 1, u64::MAX).unwrap_err(),
        IsbError::Argument
    );
    assert_eq!(
        MappedIsb::create(u32::MAX / 4, u32::MAX, u64::MAX).unwrap_err(),
        IsbError::Argument
    );
    // The size must also fit a slice length and a file offset: at most
    // isize::MAX and i64::MAX. One row more than that limit is refused; on a
    // 32-bit target a single 4 GiB row already exceeds isize::MAX.
    let limit = (isize::MAX as u64).min(i64::MAX as u64);
    let width = (1u32 << 30) - 1;
    let stride = u64::from(width) * 4;
    let height = u32::try_from(limit / stride + 1).unwrap();
    assert!(stride * u64::from(height) > limit);
    assert_eq!(
        MappedIsb::create(width, height, u64::MAX).unwrap_err(),
        IsbError::Argument
    );
}

/// The kernel's own view of the writable mapping: shared, writable and not
/// copied on fork ("dc").
#[test]
fn the_writable_mapping_is_shared_and_excluded_from_fork() {
    let isb = MappedIsb::create(8, 8, 256).unwrap();
    let address = isb.pixels().as_ptr() as usize;
    let smaps = std::fs::read_to_string("/proc/self/smaps").unwrap();
    let mut entry = None;
    for line in smaps.lines() {
        if let Some((range, _)) = line.split_once(' ')
            && let Some((start, end)) = range.split_once('-')
            && let (Ok(start), Ok(end)) = (
                usize::from_str_radix(start, 16),
                usize::from_str_radix(end, 16),
            )
        {
            entry = (start..end).contains(&address).then_some(line);
            if entry.is_some() {
                assert!(line.contains("memfd:sophia-image"), "{line}");
            }
            continue;
        }
        if entry.is_some()
            && let Some(flags) = line.strip_prefix("VmFlags:")
        {
            let flags: Vec<&str> = flags.split_whitespace().collect();
            for flag in ["rd", "wr", "sh", "dc"] {
                assert!(flags.contains(&flag), "missing {flag}: {flags:?}");
            }
            return;
        }
    }
    panic!("mapping not found in smaps");
}

#[test]
fn builders_move_between_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MappedIsb>();
    let mut isb = MappedIsb::create(1, 1, 4).unwrap();
    isb.pixels_mut().copy_from_slice(&[4, 3, 2, 1]);
    let fd = std::thread::spawn(move || isb.finish().unwrap())
        .join()
        .unwrap();
    let mut back = [0u8; 4];
    assert_eq!(pread(&fd, &mut back, 0).unwrap(), 4);
    assert_eq!(back, [4, 3, 2, 1]);
}
