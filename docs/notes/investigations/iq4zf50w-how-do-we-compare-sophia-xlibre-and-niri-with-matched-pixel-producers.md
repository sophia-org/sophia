---
id: iq4zf50w
date: 2026-10-29
kind: investigation
status: investigating
tags: [investigation, performance, benchmark, shell]
---
# How do we compare Sophia, XLibre and niri with matched pixel producers?

## Question and decision

Mason requested apples-to-apples benchmarks so that Sophia optimization is guided
by comparable work. Put the CPU-produced comparison first on the immediate
critical path, GPU-path qualification second, and further native optimization
experiments after those gates. W3 and single-submit decisions may continue as
written reviews. This note schedules work; it does not qualify a native package.

There is no universal CPU-filled default for XLibre or Wayland. The application
and toolkit choose the producer. Niri is one Wayland compositor, not Wayland as
a whole. Matching producers improves comparability without making security,
ownership or presentation duties identical.

## Evidence and correction

The completed `desktop-cpu-04` package compares a changing 1920×24 strip at actual
119.88 Hz, but its external clients CPU-fill shared memory while Sophia renders
through Vello/wgpu, reads RGBA back from the GPU and uploads it into an owned ring.
Its report explicitly limits the combined CPU result to these concrete paths.
Evidence: `~/.local/state/t331/desktop-cpu-04/{PLAN.txt,report/report.md}` and
its frozen `package.json` and arm requests. Keep that evidence immutable.

The XLibre arm uses pinned `56be9f4320ef`, modesetting/glamor, PageFlip disabled,
COPY-at-vblank, and no WM or compositor. Niri runs as a compositor. X Present
COPY/vblank feedback, Wayland presentation feedback and Sophia's exact native
kernel-completion joins are different clocks/events. Matching cadence does not
make their submit-to-completion spans common end-to-end latency.

Source inspection corrects the earlier proposed staging-buffer fallback:
Rust SDK `0706ea2c` already exposes `ShellConnection::image_upload_ring_fill`
in `crates/sophia-shell-client/src/lib.rs`. Its callback fills the mapped slot
directly; the extent must equal width × height × 4. The baseline GPU peer
`748c325` instead calls `Canvas::fill`/`read_rgba` and `image_upload_ring`.
The CPU lane needs a peer producer mode, not a new SDK API or intermediate copy.

## Comparison lanes

| Lane | Sophia | XLibre | Niri |
| --- | --- | --- | --- |
| CPU-produced pixels | CPU fills owned-ring slot directly; existing acceptance and presentation | CPU fills SHM image/pixmap; Present | CPU fills `wl_shm` buffer; compositor presentation |
| GPU-produced pixels | Same Vello/wgpu scene to GPU buffer; existing DMA-BUF capture/import and presentation | Same renderer/scene to GPU buffer; qualified compatible presentation path, including DRI3/Present where applicable | Same renderer/scene to GPU buffer; DMA-BUF submission and presentation |

The GPU row is a qualification target, not an implemented or proven equivalent
path. Any capture, copy, format conversion or synchronization required by a stack
stays in the measured path and is reported. A failed GPU-buffer adapter must not
silently fall back to readback and still be called the matched GPU lane.

### First: CPU producer implementation and qualification

- Start with host `d9116ff0` and peer `748c325` in isolated lanes. W2 and other
  experimental optimizations remain off; their frozen sources/releases stay intact.
- Add an explicit CPU producer selection to the benchmark peer and corresponding
  runner/recipe validation. Keep the existing GPU default and reject inconsistent
  declarations. CPU mode must not initialize Vello, wgpu or a GPU producer.
- Fill the mapped ring slot through `image_upload_ring_fill`. Match the external
  clients' `shade_mod128_a255` pixel recipe, semantic color/alpha, dimensions,
  full-frame damage and frame progression. Verify byte order, stride and padding
  rather than assuming equal format names mean identical pixels.
- Keep two reusable buffers/slots, 2120 generations, 2000 counted intervals and
  the existing warmup/anchor mapping. Use DP-1 1920×1080, scale 1, actual 119.88 Hz,
  VRR off and other outputs off. Match pacing and queue-depth limits.
- Preserve admission, immutable acceptance, release-after-last-lease and each
  stack's synchronization rules. Count required internal copies; do not remove
  architectural duties to improve the result.
- Retain the qualified XLibre COPY configuration for the first comparison and
  label it explicitly. Changing that topology is a separate qualification.

Device-free gates: exact expected pixels across producer implementations and
representative generations; no writes to a slot before its release; CPU mode
avoids GPU initialization; unsupported/mismatched modes refuse; the GPU default
and old recipe acceptance remain unchanged. Inspect compiled/source and runtime
producer evidence rather than inferring the mode from low CPU consumption.

### Second: GPU-buffer qualification

Before implementing or packaging adapters, produce a compatibility matrix for
all three stacks: renderer and dependency versions, scene, adapter, format and
modifier, buffer lifetime, submission/completion synchronization, copies/capture
and presentation feedback. Reuse an existing adapter only after verifying those
properties. Use the same renderer version and scene without an unrelated
single-submit or reuse optimization in one arm.

Missing transport or synchronization support blocks this lane; it is not a reason
to substitute readback or silently weaken acceptance. GPU execution time, if
collected, is separate diagnostic evidence from CPU accounting. A GPU-time figure
must not be inferred from CPU render/readback spans.

## Measurement, acceptance and rollout

Use fresh packages and retain all attempts, failures and arms. Before sealing,
freeze exact signed source/build/artifact identities, producer receipts, workload,
run order, accounting scope, anchors and refusal rules. Root reviews/seals;
Mason launches native attempts serially through SSH/PAM/VT. Pause builds during
captures. This documentation change authorizes no native launch.

Report client, server and combined CPU with the same bound-process scope; retain
per-run values and accounting intervals. Transient children, external workers and
whole-desktop CPU remain excluded unless a separately qualified accounting method
includes them. Report cadence and missed frames, and label each stack's latency
span separately. Do not pool different producer lanes or retrofit old evidence.

A comparison must pass pixel/producer identity, buffer lifetime, display/cadence,
process identity, sample/anchor and cleanup gates before it is used to rank work.
A CPU interval containing zero establishes neither a saving nor equivalence.
Use the matched baseline to select isolated Sophia changes and their mechanism
counters; do not turn a cross-stack gap into an avoidable-CPU budget.

## W3 and existing results

Collect-idle is closed: mechanism qualified, CPU combined [-35,+45] us/frame,
inconclusive, no promotion and no repeat. W2's two CPU benefits remain evidence
for its frozen workload, not measured wake-count reductions. Single-submit's
combined [-55,+25] result remains inconclusive; the peer result alone does not
justify productization.

Claude's signed local `dbe426932` in `~/dev/sophia-cpu-wake-design` reconciles
`516tpvf5` with canonical `tubnycgx`; it does not change the contract. W3 remains
design-only pending explicit decisions about rate enforcement, response reservation
timing, supersession and W2 cancellation, exhausted-credit/pacing-permit semantics,
producer cancellation/withdrawal and expiry notification. The source trace finds
no read of the advertised rate limit in Session/runtime. Do not assert an existing
rate gate. A serial benchmark with no occupied predecessor slot still performs
admission/capacity checks and processing; it is not evidence of zero server cost.

No protocol identifiers are allocated here. W2 C parity, public SDK reachability
and D1/t341 decisions remain separate gates. All sealed packages remain immutable.

## Connections

- [CPU critical path](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md): owns ordering and optimization decisions.
- [Repeated-work investigation](p4jd1ymh-which-complexity-and-repeated-work-costs-belong-on-the-frame-cpu-critical-path.md): preserves attribution scopes and the closed collection result.
- [Canonical standing-credit design](tubnycgx-can-a-bounded-frame-stream-cut-per-frame-control-exchanges-without-weakening-custody.md): W3 design gate, not a competing benchmark design.
