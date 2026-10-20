//! Opt-in local wall-time decomposition, not a native performance verdict.
use sophia_isb::MappedIsb;
use std::{
    fs::File,
    hint::black_box,
    io::{Read, Seek},
    time::Instant,
};

const WARMUP: usize = 64;
const ROUNDS: usize = 512;

fn minor_faults(stat: &mut File) -> u64 {
    let mut bytes = [0; 4096];
    stat.rewind().unwrap();
    let len = stat.read(&mut bytes).unwrap();
    let text = std::str::from_utf8(&bytes[..len]).unwrap();
    // comm (field 2) may contain spaces or parentheses. Field 10 is minflt.
    text.rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .nth(7)
        .unwrap()
        .parse()
        .unwrap()
}

fn fault_control(width: u32, height: u32, canvas: &mut [u8]) {
    let mut stat = File::open("/proc/thread-self/stat").unwrap();
    for _ in 0..WARMUP {
        black_box(minor_faults(&mut stat));
    }
    let before = minor_faults(&mut stat);
    let mut image =
        MappedIsb::create(width, height, u64::from(width) * u64::from(height) * 4).unwrap();
    let created = minor_faults(&mut stat);
    fill_pixels(image.pixels_mut(), 1);
    let filled = minor_faults(&mut stat);
    let fd = image.finish().unwrap();
    let finished = minor_faults(&mut stat);
    drop(fd);
    let closed = minor_faults(&mut stat);
    fill_pixels(canvas, 1);
    let reused = minor_faults(&mut stat);
    println!(
        "isb_local_faults schema=1 width={width} height={height} create={} fill={} finish={} close={} reused_fill={} timed=false",
        created - before,
        filled - created,
        finished - filled,
        closed - finished,
        reused - closed,
    );
}

#[inline(never)]
fn fill_pixels(pixels: &mut [u8], generation: usize) {
    let shade = (generation % 128) as u8;
    for (index, byte) in black_box(pixels).iter_mut().enumerate() {
        *byte = if index % 4 == 3 { 255 } else { shade };
    }
}

fn sample(width: u32, height: u32, generation: usize) -> [u128; 5] {
    let bytes = u64::from(width) * u64::from(height) * 4;
    let start = Instant::now();
    let mut image = MappedIsb::create(width, height, bytes).unwrap();
    let created = Instant::now();
    // Match the comparator's shared pixel-fill workload on fresh backing.
    fill_pixels(image.pixels_mut(), generation);
    let filled = Instant::now();
    let fd = image.finish().unwrap();
    let finished = Instant::now();
    drop(black_box(fd));
    let closed = Instant::now();
    [
        (created - start).as_nanos(),
        (filled - created).as_nanos(),
        (finished - filled).as_nanos(),
        (closed - finished).as_nanos(),
        (closed - start).as_nanos(),
    ]
}

#[test]
#[ignore = "local diagnostic; run explicitly in release mode on an idle host"]
fn mapped_isb_stage_costs() {
    assert!(!black_box(cfg!(debug_assertions)), "requires --release");
    let mut clock_cost = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let start = Instant::now();
        clock_cost.push(start.elapsed().as_nanos());
    }
    clock_cost.sort_unstable();
    println!(
        "isb_local_clock schema=1 rounds={ROUNDS} median_ns={} subtracted=false",
        clock_cost[ROUNDS / 2]
    );
    for (width, height) in [(1920, 24), (1024, 1024)] {
        // Diagnostic control only: an already populated ordinary canvas is
        // not an ISB substitute. Both fills call the same compiled function.
        let mut canvas = vec![0; width as usize * height as usize * 4];
        for generation in 0..WARMUP {
            black_box(sample(width, height, generation));
            fill_pixels(&mut canvas, generation);
        }
        fault_control(width, height, &mut canvas);
        let mut samples = Vec::with_capacity(ROUNDS);
        let mut reused_fills = Vec::with_capacity(ROUNDS);
        for generation in 0..ROUNDS {
            if generation % 2 == 0 {
                let start = Instant::now();
                fill_pixels(&mut canvas, generation);
                reused_fills.push(start.elapsed().as_nanos());
            }
            samples.push(sample(width, height, generation));
            if generation % 2 != 0 {
                let start = Instant::now();
                fill_pixels(&mut canvas, generation);
                reused_fills.push(start.elapsed().as_nanos());
            }
        }
        reused_fills.sort_unstable();
        println!(
            "isb_local_fill_control schema=1 width={width} height={height} rounds={ROUNDS} median_ns={} p95_ns={} producer_alternative=false",
            reused_fills[ROUNDS / 2],
            reused_fills[ROUNDS * 95 / 100],
        );
        // Percentiles are per stage, not additive. The total is measured
        // independently; all timer overhead and descheduling remain included.
        for (index, stage) in ["create", "fill", "finish", "close", "total"]
            .into_iter()
            .enumerate()
        {
            let mut values: Vec<_> = samples.iter().map(|row| row[index]).collect();
            values.sort_unstable();
            println!(
                "isb_local_cost schema=1 width={width} height={height} bytes={} rounds={ROUNDS} stage={stage} min_ns={} median_ns={} p95_ns={} mean_ns={}",
                u64::from(width) * u64::from(height) * 4,
                values[0],
                values[ROUNDS / 2],
                values[ROUNDS * 95 / 100],
                values.iter().sum::<u128>() / ROUNDS as u128,
            );
        }
    }
}
