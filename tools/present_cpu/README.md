# Unattended Present CPU regression checks

This harness starts new headless QEMU sessions and samples their Session,
generic X11 client and whole-guest CPU. It neither connects to the installed desktop nor
opens host DRM/input devices. Guest input is autonomous; no operator window
arrangement is required. KVM is mandatory for CPU comparisons.

The client uses **core pixmaps and software Present**, not DMA-BUF client buffers.
Two independent connections create disjoint 320x240 windows on the same head.
Each reuses eight immutable patterned pixmaps only after both Complete and Idle.
No terminal feature or client workload is disabled to improve the result.

## Build once, reuse the image

Use the lane's existing Cargo target/home, normal priority, eight build jobs and
eight test threads. Run builds under the usual device-hidden isolation wrapper.

```sh
python3 tools/present_cpu/build_record.py --target /absolute/reusable/cargo-target \
  --wrapper /absolute/isolation-launcher.py --out /absolute/new/build-record
```

`tools/build_qemu_session_initramfs.sh` includes the client and honors
`CARGO_TARGET_DIR`. To reuse an existing guest's libraries/modules instead of
rebuilding an image, append a small overlay. Use the same base image for both
revisions in a comparison. The base must contain the binaries' runtime libraries,
the matching kernel modules and the normal Sophia guest dependencies.

```sh
python3 tools/present_cpu/build_overlay.py \
  --base /absolute/existing/guest.img --kernel /boot/vmlinuz-KERNEL \
  --sophia /absolute/target/release/sophia \
  --client /absolute/target/release/examples/present_cpu_workload \
  --build-record /absolute/new/build-record/build.json \
  --out /absolute/new/image-directory
```

The resulting `guest.img.json` records the base, kernel, image, executable and
guest-init hashes, plus realpaths and ELF build IDs. The build receipt binds both
executable hashes to the source HEAD, diff and untracked hashes captured before
and after compilation; packaging refuses different binaries or a failed/changed
build. Each executable is hashed immediately after its build and checked again
after both builds. The receipt records Cargo/rustc versions, build flags and
configuration file hashes from inside the build wrapper, before and after.
Packaging source identity is recorded separately. Neither
the runner nor the overlay builder overwrites an earlier attempt.

## Probe, then record three runs

Arrange a quiet measurement window with other lanes. Ordinary builds do not need
a slot; CPU measurements need controlled load. Run:

```sh
python3 tools/present_cpu/run_qemu.py \
  --kernel /boot/vmlinuz-KERNEL --initramfs /absolute/image-directory/guest.img \
  --out /absolute/new/campaign --quiet-note 'Other lanes quiet; operator session unchanged'
```

The guest uses the existing normal-session startup application path and exits
when that app exits. Session-emitted mode, successful app exit and client
completion records are required; proof mode intentionally polls at 1 ms and is
invalid for this baseline. Kernel panic, lockup or RCU stall invalidates a run.

The runner probes both workloads for ten measured seconds after ten seconds of
grace. Any invalid probe stops the campaign. `--probe-only` stops after the probes.
The full campaign then runs each workload three times for 60 seconds, alternating
their order. `--baseline /absolute/old/campaign/SUMMARY.json` compares against a
stored baseline with the same kernel, base image, client, guest recipe, QEMU,
vCPU count, memory, host kernel/CPU/microcode, KVM parameters, governor and workload. Keep host-load records with the result;
configuration equality alone cannot establish a quiet host.

- **Open:** absolute offered schedule, five Presents/s/window by default, below
  previously observed guest capacity. `--rate` changes this explicitly for both
  revisions. Missed offers, buffer starvation or lateness of one period invalidate
  the run. Lowering the rate only on the candidate is not a valid comparison.
- **Closed:** the next request follows both Complete and Idle. No rate cap;
  throughput and latency are outcomes. CPU is always reported per completed
  request alongside throughput, so slowing the client cannot imply a saving.

Targets default to `last_msc + 1`; `--target zero` offers target=0. Both use
options=0, divisor=0 and remainder=0. Visible bindings must all be Hardware or all
Unclocked in the measured counter interval. Fake/mixed bindings invalidate this
visible workload. Unclocked virtio is legitimate: it uses real retirement and
does not wait for a synthetic 1 Hz field. It cannot measure W1 cache benefits.

## Evidence and gates

The `sophia_owner_wait` record attributes actual polls to the shortest selected
cap. Equal caps keep the first reason in the owner loop's existing order. The
selected reasons must sum to `owner_waits`; expired reasons must sum to
`owner_wait_deadlines`. Queued authority items that bypass polling count in
neither. Pending work flags overlap and are reported separately. The analyzer
requires identical timestamps and owner TIDs for wait and Present counters and
normalizes both by completions inside that counter interval. No wait budget or
readiness rule is changed by this instrumentation.

`--present-evidence full|aggregate` and `--renderer-worker private|shared` select
the existing opt-in modes. The guest must confirm the evidence mode and the
expected final worker count (two private workers or one shared worker for this
one-card, two-head fixture). Final worker errors and leased slots invalidate the
run. A baseline comparison must keep these settings equal unless
`--compare-setting present_evidence|renderer_worker` names the one changed
setting. Such a mode comparison also requires the exact same Session binary.
Pair the arm order in measurement campaigns; changing both settings cannot
attribute a benefit to either one. These experiments do not change defaults.

Each trial retains whole-guest CPU/steal and process/thread CPU and runqueue time, context switches, raw
send-to-Complete latency and UST/MSC pairs, event counts/modes, periodic clock and
render counters, hashes, commands, exit status and host before/after records.
Guest steal above 1% on any vCPU (or on average), or unattributed guest CPU above
20%, invalidates a run. Whole-guest CPU is vCPU capacity minus idle, iowait and
steal time; task attribution sums every thread's scheduler runtime by PID/TID
and start time. A worker changing its name retains its identity. New tasks are
counted from birth; a missing prior task invalidates attribution. Negative
accounting beyond two clock ticks per vCPU per idle/iowait/steal field also
invalidates the run. The owner
is identified by its Session-emitted TID, not assumed to be the process leader.
Context switches are scheduling proxies, not exact hardware wake counts. The
client samples the explicitly supplied parent Session PID and itself. The guest
recipe also enables bounded, system-wide `/proc/PID/stat` snapshots (at most
1024 processes) to name remaining CPU costs. That flag defaults off outside the
guest recipe; it reads CPU counters and identities, not commands or environments.
The guest enables scheduler statistics before warm-up. The [kernel's scheduler
statistics format](https://docs.kernel.org/scheduler/sched-stats.html) defines
per-CPU and per-task runtime in nanoseconds. Versions 15–17 are supported; unknown
formats fail closed. Guest tick-sampled user/system counters are retained as
diagnostics, since they undercounted this guest's busy time. Per-CPU runqueue
runtime includes IRQ while a task is running ([kernel implementation](https://github.com/torvalds/linux/blob/v6.18/kernel/sched/stats.h)),
so adding all IRQ to it would double-count some work. The independent capacity
estimate must lie between runqueue runtime and runtime plus IRQ/softirq, within
5% of busy time or two ticks per vCPU per idle/iowait/steal field, whichever is
larger. Attribution includes
all task runtime plus IRQ/softirq; no kernel work is subtracted from total cost.
The accounting method is part of baseline compatibility.
Session stdout/stderr go to a guest tmpfs file, exported after measurement with
all records intact as gzip/base64 over a dedicated virtio-serial channel backed
by a host file. Kernel console messages never share that channel. The runner validates the compressed stream,
preserves its exact decoded bytes and hashes, and bounds decoded size at 128 MiB.
Logs and measurement artifacts use a separate 256 MiB tmpfs so the unpacked
initramfs cannot exhaust their space. Filling that bound invalidates the run.
Both snapshots require at least 256 MiB of guest MemAvailable and log size no
larger than 128 MiB. Workload results are exported before the bulk log and kept
even on export failure; export errors power off with a failed result. Compression
streams directly into base64 with no second log copy. Raw size/hash and tmpfs
usage precede the stream; the host verifies them and records export wall time.
Streaming records through the emulated UART during a sample
adds substantial kernel IRQ work. Whole-guest CPU fields and interrupt tables
are retained to expose that cost; the accounting thresholds are unchanged.

Validity requires unchanged geometry/thread identities, no protocol/event loss,
no outstanding buffers after drain, monotonic clocks on each fixed source, no
counter resets or admission/render errors, and real progress. Counter gates
require no new capture contexts or pipelines after grace, exact full-repaint
reason totals, the stable-geometry partition, and bounded repaint area. This
fixture defaults to absent damage (the full pixmap). Explicit damage options
are described below. These core pixmaps do not prove DMA-BUF import reuse or
physical scanout pixels; those still need render-node proofs.

CPU snapshots bracket the sample. Five-second counter records use monotonic
timestamps and only records inside that sample; their shorter spans are reported
separately. Full runs require at least 80% counter coverage (40% for ten-second
probes, because the existing record period is five seconds). Final Complete/Idle drain contributes cohort latency and event
accounting, but is outside the CPU interval. Missing fields never become zeros.

An invalid run exits 2 and cannot establish performance. A valid comparison
requires at least three runs on each revision. Growth above five percentage
points in either unattributed share or CPU share outside Session/client
invalidates a comparison, even when the external worker has been identified.
`saving_observed` requires separated downward ranges for both Session and total
guest CPU per completion; shifting work between processes cannot satisfy it.
It flags median Session or
whole-guest CPU/completion
growth above 10%, throughput loss above 2%, or p95 latency growth above 10%, and
exits 3 for review. Range separation is flagged even below the thresholds. Ranges expose noise; a flag requires diagnosis,
not reruns until green. A first baseline is only `BASELINE_RECORDED`, not a saving.
Guest measurements are relative signals, not live acceptance or physical GPU
timing. W1, physical mixed-refresh and VT acceptance remain separate.

## Analyzer controls

```sh
python3 -m unittest discover -s tools/present_cpu -v
cargo clippy --offline --locked -p sophia-session --features native-session \
  --example present_cpu_workload -- -D warnings
```

The controls corrupt individual records to reject Fake fallback, missing/reset
counters, starvation, geometry changes, missing Idle, clock regressions, bad
latency/mode counts, resource churn and short samples. Separate controls prove a
valid performance regression fails the threshold gate instead of being called
invalid or accepted.

## Repository checks

`cargo run --offline --locked -p xtask -- check present-cpu` runs the deterministic
accounting, comparison, build-identity and export tests. The full `xtask check`
also runs them. QEMU measurements remain an explicit quiet-window command.

The first qualified reference is `t289-cpu-gates-01/baseline-05`: two probes and
three 60-second runs per arm, all valid in normal Session mode. It is an
Unclocked/core-pixmap guest baseline, not hardware or live CPU acceptance.

## Repaint attribution

New samples also emit `sophia_live_damage_causes` and `sophia_present_damage`
at the existing counter cadence. Inspect a fixed interval with:

```sh
python3 tools/present_cpu/damage_attribution.py session.log \
  --start-usec START --end-usec END
```

The reporter requires matching observation times, intact cumulative counters
and conserved full-plan subreasons. `full_*` causes count only full frames;
other causes count all successfully rendered frames. Causes overlap. Client
region counts describe successfully executed Pixmaps, not composed frames.
`rect_pixels` sums clipped rectangles including overlaps; it is not union area.
The earlier baseline has no attribution records and cannot supply these details.
The existing CPU comparison remains compatible through the unchanged total
`damage_full_plan_count`. A cause identifies an optimization candidate, not proof
that its conservative repaint is unnecessary.

The attribution interval must stay within one uninterrupted Session/native-owner
lifetime (for example, the CPU harness's validated interval). These damage records
do not carry an owner identity; counter monotonicity alone cannot prove it.
`full_without_usable_reduction` is the sum of unavailable/disabled/unknown-history
full reasons, not a claim that those frames carry no flags. `rebased` means an
edge marked as rebased was walked, including when a later precision check failed.

### Near-output-sized damage experiment

Use `run_qemu.py --clients=1 --size=head --damage=absent`, then separate runs
with `--damage=full` and `--damage=patch`, keeping the binary, head, cadence and
other options fixed. `head` leaves a 16-pixel margin inside one active RandR
CRTC. It refuses two clients, which would overlap. The default `small` case
retains the two disjoint 320 by 240 windows.

All three damage cases use the same pixels: a constant background and a 120 by
120 patch whose color alternates across an eight-pixmap pool. `absent` supplies
no update region, `full` supplies one full-size XFixes rectangle, and `patch`
supplies only the changed rectangle. The first Present always initializes the
full window. Before grace, nine completed Presents check every source pixmap
with GetImage against the expected background and patch, including pixels
outside the update region and one pool reuse. These reads are outside the
measured interval.
Window GetImage reads Sophia's separate core-drawing backing and cannot check
the Present raster. A component regression compares the whole renderer payload
after absent, full and patch updates, including an empty-update negative control.
Neither check is native framebuffer readback.

The result records the head and window geometry, patch, damage kind,
`source_pixel_checks_before_grace=9` and `pattern=fixed_patch_v1`. Older samples
alternated the whole background. The analyzer refuses comparisons across those patterns, sizes or
damage kinds; record repaint attribution separately for this experiment rather
than calling a workload change a Sophia speedup. Preserve every first failure.
Pixmap selection follows a fixed cursor, regardless of reply order; an unavailable
entry counts as starvation. A drained green frame after grace makes every arm
start with yellow. `unchanged_presents` must be zero. Comparison identity also
includes every active head's size and position: two composited heads with one
window must not be compared with a one-head run. Normalize attribution by the
recorded executed/completed Presents, reporting offered and starved counts;
frame cause counts describe output frames and can exceed the Present count.

Expected result: absent and explicit full damage may select full repaint because
the window nearly fills its head. Patch damage should retain precision when the
existing history proof permits it. Inspect the recorded refusal or threshold
before proposing a production change. Neither outcome alone explains a terminal
that uses DMA-BUF pixmaps.

The first valid series (`t289-damage-workload-01/probe-03-*`) found full repaint
through `PlanCoverage` in all three cases. The patch reached the executed-Present
counter as 14,400 pixels, but the CPU Present path supplied full canonical raster
damage. Precise canonical damage currently requires DMA-BUF in both
`runtime/present_pixmap.rs` and `drawing.rs`. The resulting 958,464 pixels cover
93.6% of the 1280 by 800 head, exceeding the 60% repaint threshold. This locates
a conservative producer limit; it does not justify changing the threshold or
establish the cause of a DMA-BUF terminal's full frames. The complete-payload
regression proves content equivalence separately, without native readback.
