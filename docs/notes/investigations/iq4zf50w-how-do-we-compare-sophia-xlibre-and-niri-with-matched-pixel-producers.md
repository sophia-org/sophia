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

## October 29: CPU implementation and sealed first attempt

The matched CPU lane is implemented, device-free qualified and sealed as
`desktop-matched-cpu-01` on crunch. No native arm has run at this checkpoint.
The MANIFEST SHA256 is
`93fdaccd22b21332d2b9f93378871af2c8aed77cff5a5ecfd391eadce1902843`:
101 package files and 196 external pins. Read-only launch checks passed all
six SSH/idle-VT checks and both schema-4 diagnostic checks at that point.
This package was subsequently attempted and failed as recorded below; do not rerun it.

Signed local source identities (no push or merge):

- Peer `9cb0a37c7b088422c0a71706700f4c7a5adf23e5`, parent `748c325`:
  explicit CPU selection, direct slot fill through existing SDK0706ea2c,
  unchanged GPU default. Fourteen tests and clippy pass; pixel/alpha and
  accidental GPU-initialization mutations fail. A two-slot fixture proves
  acceptance alone does not make a held slot writable.
- Runner `c62d4382179a7952db1d36f952f0b5f2d1d98ad3`, parent `2329fca7`:
  paired producer configuration, schema4, exact observed CPU receipt and
  historical defaults preserved. 472 Rust tests, 44 cpuplan tests and clippy
  pass. Schema4 pins the peer configuration and rejects malformed numeric
  schema/receipt/configuration. cpuplan is reusable support, not this package's
  accounting entry point.
- Harness `3b24ccdd02a4f91a2bd0f1c6db12b82cdfae410f`, parent `13915e05`:
  `report --matched-cpu` refuses historical, GPU or unqualified Sophia rows.
  Twenty-four tests pass. External C sources are unchanged; rebuilt clients
  pass the fill-parity test. The package calls this harness and cpuwin directly.

Both Sophia arms use original host `d9116ff0` / binary `3897c98c`, new peer
`2a49d47d`, and runner `04b965d1`. Release builds ran serially in their own
cargo-slot pools; all 93 recorded protected paths were unchanged. Exact-host
argument preflights accepted 13 arguments without opening devices. Seven
package tests, eight external qualification tests, seven wrapper tests and
43 postflight mutation refusals passed; 38 frozen fixture inputs were unchanged.
Build/source/fixture receipts are under `matched-cpu-producer-01`,
`matched-cpu-runner-01`, and `matched-cpu-release-01` in local state; the sealed
package retains its build receipts and complete validation inventory.

Native order is niri1, xlibre1, sophia1, niri2, xlibre2, sophia2: two interleaved
observations, not ABBA. Pixel generation matches; custody, copies, presentation
feedback and bound-process coverage still differ. XLibre retains the qualified
PageFlip-off COPY path. A CPU stderr receipt is supporting evidence beside
binary/argv/PID binding and denied GPU admission, not an independent driver
trace. No CPU benefit or cross-stack result is claimed before the saved-data
audit. GPU-buffer qualification follows that gate; W3 and W2 holds are unchanged.

## October 29: first-attempt failure and repaired package

`desktop-matched-cpu-01` stopped after niri1 completed. The xlibre1 capture
finished but qualification failed because root preparation omitted
`preparation/xlibre-source.json`. This was a package defect, not a measured
rendering failure. Root verified the manifest, all 196 external pins and both
captures' PAM/terminal cleanup: no residual processes or rescue signals.
The remaining four arms never launched. No six-arm result or comparison is
claimed; all 143 saved files remain unchanged.

Fresh `desktop-matched-cpu-02` keeps the exact host, peer, runner and external
client binaries, workload, arm order and accounting rules. It copies the
qualified desktop-cpu-04 source receipt (SHA256 `d689be92…47ca`). Root checked
its driver hash and every excerpt against XLibre commit `56be9f4320ef` and the
installed commit marker. The original postflight source predicate is extracted
unchanged into a shared function. Preparation, seal and launch checks now
require the frozen receipt bytes and that predicate before any native arm.
This remains source-derived TearFree qualification, not a runtime query.

Four new device-free tests reproduce the actual01 missing-input failure,
supply only the receipt in memory and obtain identical old/new qualification
results for niri1 and xlibre1, and prove early refusal for missing or altered
receipts and a stale installed marker. Frozen-SHA corruption tests are not
additional predicate coverage. Existing seven wrapper tests, seven package
tests, eight qualification tests and 43 postflight mutation refusals pass.
The prior package is rehashed before/after the saved-data test. Claude's
read-only review found no blocker. The offline fixture depends on01 remaining
in place; launch check-only does not replay that fixture.

02 is sealed with 110 files and 196 external pins. Manifest SHA256:
`d17d5ef1997ea9663cf66ecda5b0b737bc5f2e02c3d9ced9be89ef21dd88808a`.
All six user-level SSH/idle-VT checks and both schema4 descriptor checks pass.
The sudo wrapper could not authenticate without Mason's terminal; no attempt
was started. Run from Mason's terminal:

```sh
bash ~/.local/state/t331/desktop-matched-cpu-02/run.sh --check-only
bash ~/.local/state/t331/desktop-matched-cpu-02/run.sh
```

The new package includes the failed result, failure receipt, root cleanup audit,
source provenance and repair parity under preparation. Additional root receipts
are in `~/.local/state/matched-cpu-repair-02`. A new six-arm result must stand
alone: do not pool or substitute01 rows. No native02 result exists yet.

## October 29: matched CPU result and root verdict

`desktop-matched-cpu-02` completed niri1, xlibre1, sophia1, niri2, xlibre2,
sophia2. Root's saved-data audit reproduced every CPU bound from raw samples,
validated generation120/2120 anchors and bound PID/starttime throughout each
window, and recomputed latency and completion gaps. All postflights and cleanup
passed. The source receipt repair changed no rendering or accounting rule.
Manifest `d17d5ef1` verifies; all 196 external pins and 266 saved files were
unchanged during audit. Root receipts are outside the package in
`~/.local/state/matched-cpu-audit-02/{audit.py,root-review.json}`. Claude independently
checked saved receipts and reported no discrepancy. No capture was rerun.

| Stack | First total CPU us/frame | Second total | Mean interval | Server/Session mean | Client/peer mean |
| --- | --- | --- | --- | --- | --- |
| niri | [295,340] | [315,355] | [305,347.5] | [285,305] | [20,42.5] |
| XLibre | [90,130] | [80,120] | [85,125] | [75,95] | [10,30] |
| Sophia | [1030,1070] | [1025,1070] | [1027.5,1070] | [825,847.5] | [202.5,222.5] |

Sophia minus niri is [680,765] us/frame; Sophia minus XLibre is [902.5,985].
These differences are resolved at tick-accounting resolution for this workload.
The result qualifies the matched CPU reference; it does not promote an
optimization or establish statistical confidence from two interleaved repeats.
The Session accounts for most measured Sophia CPU, but this measurement does
not apportion it among protocol, validation, copying and presentation work.
Historical GPU/readback rows are not a paired intervention and cannot isolate
readback cost or establish a causal optimization saving.

Every counted window contains2000 intervals over about16.68346 seconds, with
median completion gap8342us and zero missed-refresh gaps. Own-clock latency
medians: niri16643.9/16625.2us, XLibre8513.7/8521.9us, Sophia8110/8108us.
These span different submission/feedback boundaries; no end-to-end or photon
latency ranking follows. The XLibre path remains PageFlip-off COPY/vblank with
no WM/compositor. External producer evidence combines pinned source/binaries
and journal recipe; Sophia additionally has the exact CPU receipt and denied
GPU admission. This is not an exhaustive syscall/GPU-work trace.

CPU us/frame measures accumulated CPU execution per completed interval across
the bound processes. Latency measures elapsed time for an individual frame,
including waits and queueing. At119.88Hz, about1050 CPU us/frame is about12.6%
of one core, while a frame can spend about8110us between submission and
completion. Pipelining can sustain one completion every8342us even when each
frame's latency is two periods. Do not subtract averaged CPU/frame from a
frame's latency to infer exact waiting time: their scopes and boundaries differ.

Watch items retained: Sophia2's maximum sampler read bracket was270495ns,
within the existing gate (other arms at most61697ns); unclean reads84/93 were
outside Sophia's counted windows and do not alter the bounds. Native-owner
submission/retirement totals2124/2123 exceed benchmark frame counts; they are
not attributed as benchmark transactions. All cleanup/settlement gates pass.
Bound TGIDs exclude unmeasured workers/children and GPU execution; this is not
whole-desktop CPU. Pixel-producer matching does not equalize custody or copying.

Next gate is the written GPU-buffer compatibility matrix already specified
above. Keep this CPU reference and frozen experiments unchanged. No repeat of
collect-idle, W3 implementation or W2 production allocation is authorized by
this result.

## October 29: GPU-buffer source gate

The [compatibility matrix](nb3mgbwc-what-must-match-before-comparing-gpu-buffers-across-sophia-niri-and-xlibre.md)
records exact renderer versions, transports, layout, synchronization and lifetime
differences. The minimal next implementation reuses fresh Lom DMA exports with
an exact-device external constructor and an in-process bridge to the two existing
external clients. Native import/pixel/lifetime qualification follows; no matched
GPU result or zero-copy claim exists yet. This fresh-buffer lane is separate from
the two-slot CPU reference and cannot isolate GPU production cost by subtraction.
W2's identifier decision is now closed by the shell allocation ledger; parity,
public SDK pins and production qualification remain open.

## Connections

- [CPU critical path](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md): owns ordering and optimization decisions.
- [Repeated-work investigation](p4jd1ymh-which-complexity-and-repeated-work-costs-belong-on-the-frame-cpu-critical-path.md): preserves attribution scopes and the closed collection result.
- [Canonical standing-credit design](tubnycgx-can-a-bounded-frame-stream-cut-per-frame-control-exchanges-without-weakening-custody.md): W3 design gate, not a competing benchmark design.
