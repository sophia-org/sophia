---
id: h3gtssf5
date: 2026-11-09
kind: investigation
status: closed
tags: [investigation, performance, rendering]
---
# Which functions consume CPU on the current lean SFT path without changing responsiveness

## Question and selection

Niltempus chose preserving responsiveness after the [T5 scheduling screen](y4mb4ylo-two-sft-credits-need-an-explicit-batching-policy-before-they-can-reduce-owner-servicing.md),
then authorized this next step: sample the current lean frame path, identify one
concrete expensive operation inside its existing stages, and prove a small change
before commissioning an effect comparison. No optimization candidate is selected.

The [post-lean decomposition](q2m7v8ka-what-the-post-lean-decomposition-says-about-the-next-native-frame-change.md)
identifies broad phases but runs diagnostic-owner-cost and strace together. Its
instrumented phase times cannot price normal execution. The earlier function
profile predates the current lean lane. This capture supplies function attribution
with a normal host and no simultaneous strace or owner-cost instrumentation.

## Frozen input and method

One arm at `~/.local/state/t331/hp-01/changing`, host master `ad0c5239d`, normal
release features native-session and experimental-native-frame-transaction.
Binary SHA256 `e42d0fe9b73a545ffdc3c22c3120c050579212c9c085d2928f17a251e810020d`
is byte-identical to the B3 candidate. Build evidence is
`~/.local/state/sophia-lean-hotpath-build-01/`, SHA256SUMS
`3dcc9dfd9eedba2601bec01369712a57aef91a9f374c057990961d686c2d56a1`.
No runtime source edit or install; source worktree `~/dev/sophia-hp1` and its
cargo-slot pool were held through independent recount; ordinary worktree cleanup
is authorized by the accepted result, with the pool left to nightly hygiene.

The dcp-01 b-arm provides held peer/SDK/runner/helpers/configuration and postflight:
owned-ring CPU strip 1920×24, two slots, one outstanding frame, r13 terminal-only
and lean, W2 off, 2120 generations, 120 warmup, iGPU HDMI-A-2 1920×1080 at 60 Hz.
There is no deliberate extra buffering or changed authority/freshness boundary.

Reuse the reviewed sft-profile-01 binding and decoder; the collector only changes
its request path to r60b1. Sample verified owner, validation worker, other Session
and peer TIDs at strict 999 Hz cpu-clock:uk, DWARF stack 16384, for 25 seconds after
warmup. No inheritance. Require stable process/thread identity and at least 1000
whole native intervals inside both the enable/disable acknowledgements and the
post-warmup native anchors. Loss, foreign samples, insufficient owner/peer samples,
event-setting drift or decode warnings refuse qualification. Unknown leaves and
incomplete unwind indications are reported, not silently discarded.

Package MANIFEST is
`6bd7ed52381e38a73cae89da7992375f3f607201f9613a6e267d2b90415a2020`
(86 entries, 407 pins). Nineteen device-free suites pass; the initial cadence test
imported the old analyzer, and its failing log is retained beside the corrected
run. Normal compiler feature rows, exact arm identity, failure reporting and
completion markers are tested. Readiness at
`~/.local/state/sophia-lean-hotpath-readiness-01/`, SHA256SUMS
`0e6a0e940a9767f96c36cd67ad8f5f109d376738d8cd80d12d88d3d681b88b93`,
passed check-only with zero device opens, connector-status reads or native launches.
Its file-open strace is preparation only, not native instrumentation.

Bounded independent review accepted owner handoff at
`~/.local/state/sophia-lean-hotpath-package-review-claude-01/`, SHA256SUMS
`b8a89a1cc3b00564deb221a916ed70ddba5416dea3fa75d146948224c1772ccc`.
Root handoff is sealed at `~/.local/state/sophia-lean-hotpath-handoff-01/`, SHA256SUMS
`f54e5f2b9da9c993dd47a14c38d03fe2288630c34a8fe37dffef65f666584c2b`.
One non-blocking metadata inconsistency is retained honestly: package.json's unused
`perf_collection=false` is stale. Enforced `instrumentation=perf-cpu-clock` and the
attempt's `perf_collection=true` identify this capture. Fix the field in a future
package; this frozen package is unchanged. The handoff receipt records the limit.

## Saved result, November 9

The owner launch and offline decode passed; the independent parser matches every
reported count, period sum and symbol/DSO-keyed table. All 136 root input hashes
matched, package inputs unchanged. Result seal:
`~/.local/state/sophia-lean-hotpath-result-01/`, SHA256SUMS
`25c5bf5e9699e7b8feb40d5cd2e44931508dfe56eb489cfe48758fdca79c427d`.
Independent recount: `~/.local/state/sophia-lean-hotpath-recount-claude-01/`,
SHA256SUMS `6b029693f5a1d56fac9c0d41e43df574c59e6fdd205cb1e902081c10b9c069f7`.
Acceptance: `~/.local/state/sophia-lean-hotpath-audit-acceptance-01/`, SHA256SUMS
`737854c427461ed0bdbae8d534e14ced75de28c7528f4212b828ab3fc8df2ffc`.

The selected interval spans native generation anchors 155..1656 (1501 complete
intervals). All 1069 samples fall inside it; one period value, 1001001 ns, no lost
records, empty decoder stderr. Forty TIDs are stable and bound before/after.

| Role | Samples | Share of selected sample periods | Unknown leaves | Leaf-only stacks |
| --- | ---: | ---: | ---: | ---: |
| Owner | 660 | 61.74% | 12 | 38 |
| Validation worker | 43 | 4.02% | 0 | 0 |
| Other Session threads | 237 | 22.17% | 51 | 9 |
| Peer | 129 | 12.07% | 1 | 1 |

All 2000 post-warmup completion gaps (121..2120) are 16666..16668 us. Postflight,
identity and cleanup passed, no late slots; seat unchanged, tty4 prompt restored,
PAM cleanup complete, no residual bound processes, tmpfs source removed and
canonical session.log regular. No CPU/frame reduction or saving claim. The root
recount initially compared tied inclusive rows in order; its corrected comparison
keys rows by symbol/DSO. Counts were unchanged; before-log retained.

### A symbol is not necessarily the operation its name suggests

The peer's largest leaf, `ClientOutbox::enqueue_after`, accounts for 36/129 peer
samples (3.37% of all selected samples). Saved disassembly places all 36 on byte
stores or pointer updates inside its inlined pixel-fill callback, between the
peer's two clock calls. The SDK invokes the callback before queue commit. This
is producer pixel work, not queue/transport or Session cost. Independent review
agrees. Attribution evidence: `~/.local/state/sophia-lean-hotpath-attribution-01/`,
SHA256SUMS `2b8a2b9d6cb2c4c108242b442c34e26bdb41067cf31740aebbe964156f74a151`.

This is known work: typed-fill candidate `521702c671333c42d4bcd61741a53195e49a36d8`
already demonstrated a peer reduction of 21.355..21.524 us/frame in the older
ordinary-r13 `sft-fill-clock-01` ABBA, without adoption. Its result seal is
`8bfb85ad12a366f45ff7d0a43cb3a34a9a4e5854a9f7735eacf02da81f4a3601`.
The current held peer retains the byte loop. That prior result does not price a
change on today's lean lane, and reusing it would optimize the test producer,
not Sophia's compositor. Do not repeat the old experiment or count it as a new win.

Owner inclusive `service_components` is 241/660 samples (36.52% within owner),
`service_native` 128/660 (19.39%). These nested shares overlap. Allocation and
copy leaves occur under multiple callers; this profile does not establish a
removable budget or justify weakening custody, changing wake order or rewriting
the queue. Other Session attribution is also limited by 51 unknown leaves,
mostly Mesa symbols. The recommended next bounded item is source review of
construction/allocation within component servicing: name one operation and prove
its removal preserves behavior before selecting code. No new capture or runtime
candidate is selected by this result. This sampling investigation is complete;
the broader optimization task remains open.

## Interpretation and exit

The completed owner launch followed bounded package review and timing-ready handoff.
`TEST SUCCESS` means capture, postflight, native cadence and cleanup passed and the
result was saved. It explicitly leaves offline hotspot decoding pending. A failed
capture is retained; no automatic repeat. Raw addresses/symbols remain private;
only address-free rankings are published.

Rank leaves and visible inclusive callers separately by role, weighted by sample
period. Inclusive rows overlap; sampling perturbs execution and cannot establish
exact CPU cost, removable saving, a security premium, or latency. Optimized code
can obscure inlined source work; choose no candidate if attribution is inadequate.
Raw process CPU records are retained without CPU/frame reduction. Do not pool
with dcp-01, subtract across windows, or compare against historical Niri numbers.

After saved-data qualification and independent recount, select at most one named
operation with a concrete smaller implementation. First prove its mechanism and
failure behavior device-free, retaining the existing frame/control/revocation
responsiveness. Any CPU/presented-frame comparison alongside latency, cadence and
frame counts requires a separately reviewed package. No saving claimed here.
M1/M2 remain deferred, B6 retained, M3 and T4 parked; no T1/T2/T3/B8/control-ring
implementation selected. The [current plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
and `todo.md` t344 own execution order.
