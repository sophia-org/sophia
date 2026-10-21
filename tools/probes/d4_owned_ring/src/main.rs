//! Stage observations only: one process, no wire, renderer, flip or CPU gate.
use sophia_owned_ring_probe::*;
use std::{collections::VecDeque, time::Instant};
const WARMUP: usize = 32;
const ITERATIONS: usize = 128;
enum Accepted {
    Fresh(FreshResource),
    Owned(OwnedResource),
}
impl Accepted {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Fresh(v) => v.bytes(),
            Self::Owned(v) => v.bytes(),
        }
    }
}
fn faults() -> (i64, i64) {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage fills one correctly sized writable object on success.
    assert_eq!(
        unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) },
        0
    );
    let usage = unsafe { usage.assume_init() };
    (usage.ru_minflt, usage.ru_majflt)
}
fn check_samples(bytes: &[u8], seed: u8) {
    for index in [0, bytes.len() / 8, bytes.len() / 4 - 1] {
        let v = (index as u8).wrapping_add(seed);
        assert_eq!(
            &bytes[index * 4..index * 4 + 4],
            &[
                v & 127,
                v.wrapping_mul(3) & 127,
                v.wrapping_mul(5) & 127,
                255
            ]
        );
    }
}
fn arm(name: &str, bytes: usize, retain: usize) -> std::io::Result<()> {
    let setup = Instant::now();
    let mut ring = if name == "fresh" {
        None
    } else {
        let geometry = Geometry {
            slots: SLOTS,
            pixel_bytes: bytes,
        };
        let producer = Producer::new(geometry, name != "pread_unsealed")?;
        let source = Source::admit(
            producer.file().try_clone()?,
            geometry,
            if name == "mmap_copy" {
                Method::Mapped
            } else {
                Method::Pread
            },
        )?;
        Some((producer, source))
    };
    let setup_ns = elapsed(setup);
    let ring_bytes = if ring.is_some() {
        Geometry {
            slots: SLOTS,
            pixel_bytes: bytes,
        }
        .total()?
    } else {
        0
    };
    println!(
        "{{\"event\":\"setup\",\"arm\":\"{name}\",\"bytes\":{bytes},\"retain\":{retain},\"setup_ns\":{setup_ns},\"ring_bytes\":{ring_bytes},\"peak_resource_bytes\":{}}}",
        retain * bytes
    );
    let mut resources: VecDeque<(usize, Accepted)> = VecDeque::new();
    for iteration in 0..WARMUP + ITERATIONS {
        let before = faults();
        // Hold each resource until its release, independently of slot reuse.
        // At capacity release one before admitting the next destination.
        let start = Instant::now();
        if resources.len() == retain {
            let (old, value) = resources.pop_front().unwrap();
            check_samples(value.bytes(), old as u8);
            drop(value);
        }
        let release_ns = elapsed(start);
        let generation = (iteration as u64 + 1) * 2;
        let slot = iteration % SLOTS;
        let start = Instant::now();
        let source = if let Some((producer, _)) = &mut ring {
            producer.publish(slot, generation, iteration as u8)?;
            None
        } else {
            Some(FreshResource::produce(bytes, iteration as u8)?)
        };
        let producer_ns = elapsed(start);
        let start = Instant::now();
        let (accepted, stages) = if let Some((_, source)) = &ring {
            let (value, stages) = source.capture(slot, generation, || {})?;
            (Accepted::Owned(value), stages)
        } else {
            (
                Accepted::Fresh(FreshResource::accept(source.unwrap(), bytes)?),
                Stages::default(),
            )
        };
        let session_ns = elapsed(start);
        resources.push_back((iteration, accepted));
        assert!(resources.len() <= retain);
        let after = faults();
        if iteration >= WARMUP {
            println!(
                "{{\"event\":\"sample\",\"arm\":\"{name}\",\"bytes\":{bytes},\"retain\":{retain},\"iteration\":{},\"producer_ns\":{producer_ns},\"session_ns\":{session_ns},\"release_ns\":{release_ns},\"allocate_ns\":{},\"copy_ns\":{},\"scan_ns\":{},\"generation_ns\":{},\"minor_faults\":{},\"major_faults\":{}}}",
                iteration - WARMUP,
                stages.allocate_ns,
                stages.copy_ns,
                stages.scan_ns,
                stages.generation_ns,
                after.0 - before.0,
                after.1 - before.1
            );
        }
    }
    let start = Instant::now();
    for (old, value) in resources {
        check_samples(value.bytes(), old as u8);
        drop(value);
    }
    let final_resource_release_ns = elapsed(start);
    let start = Instant::now();
    drop(ring);
    let ring_release_ns = elapsed(start);
    println!(
        "{{\"event\":\"drained\",\"arm\":\"{name}\",\"bytes\":{bytes},\"retain\":{retain},\"final_resource_release_ns\":{final_resource_release_ns},\"ring_release_ns\":{ring_release_ns},\"remaining_resource_bytes\":0}}"
    );
    Ok(())
}
fn main() -> std::io::Result<()> {
    // Reverse order on alternating runs to expose warm/order sensitivity.
    let reverse = std::env::args().skip(1).any(|arg| arg == "--reverse");
    let mut arms = ["fresh", "mmap_copy", "pread_sealed", "pread_unsealed"];
    if reverse {
        arms.reverse();
    }
    println!(
        "{{\"event\":\"config\",\"warmup\":{WARMUP},\"iterations\":{ITERATIONS},\"slots\":{SLOTS},\"reverse\":{reverse},\"native\":false,\"cross_process\":false}}"
    );
    for bytes in [PANEL, BULK] {
        for retain in [1, 4] {
            for name in arms {
                arm(name, bytes, retain)?;
            }
        }
    }
    Ok(())
}
