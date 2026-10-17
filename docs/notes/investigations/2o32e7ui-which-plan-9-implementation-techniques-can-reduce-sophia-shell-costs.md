---
id: 2o32e7ui
date: 2026-10-16
kind: investigation
status: investigating
tags: [investigation, 9p, performance]
---
# Which Plan 9 implementation techniques can reduce Sophia shell costs

## Question

Which maintained Plan 9 and 9P implementations offer techniques worth testing
against Sophia's remaining costs? niltempus requested this comparison after the
mixed t331 readiness pilot. Maintenance does not establish a speed ranking.

## Current resumption

B3 validation is complete in signed Rust SDK `01474a9` and C SDK `5246f96`.
The final fault tests cover terminal status with four outstanding writes, a stale
head, partial transport output and cancellation with unissued chunks. Both SDK
suites pass, as does strict Rust Clippy. The independent review found no blockers.
Production adoption `ec410a09a` is signed and pushed. Snapshot checks, C
export/lifecycle gates, the full default Sophia suite, formatting and offline
metadata checks pass. The final source commits add only the reviewed test-scope
clarifications; their repeated snapshot checks also pass.
Comparator merge `6c87721fa` is signed and pushed with those exact archives;
its repeated snapshot checks pass. Integration remains `4b16974`. The matched
native B2/B3 pilot completed twelve arms: eight files/IPC cost arms for panel and
bulk, plus four separate progress arms. Offline replay reproduces all 12,000
exact presentation joins, journal reports and eight cost reports. Every arm
records normal console recovery. Both Claude sessions and Muse report no work
during the strict hold, now released. The installed desktop is unchanged.

All four +10% gates still fail. Panel CPU is +14.02–14.29% and allocations
+45.49–46.01% versus IPC; bulk CPU is +15.71–15.76% and allocations
+54.06–54.21%. Files owner passes per frame are 17.548–17.607 for panel and
90.179–90.384 for bulk. The result below separates improvements from the
remaining gap. Evidence, frozen plans and offline replay are under
`t331-b3-20261017/native-candidate` in the evidence directory below.

The test scopes differ deliberately: Rust forces kernel socket backpressure;
C limits each dispatch to one output byte. Rust's queued chunks are dropped by
the terminal status in its cancellation test. Enqueuing ResourceCancel alone
does not preempt those chunks. Prioritizing cancellation by dropping unissued
same-resource chunks is a follow-up investigation, outside B3; it needs its own
ordering and custody review before implementation.

The server's one-msize input budget remains the main limit on bulk pass counts.
Four client credits do not imply four pixel writes serviced in one owner pass.
The pilot supports prioritizing descriptor-path design for bulk content, with
producer/Sophia CPU, GPU work, latency and memory measured together. It does not
prove a descriptor speedup or solve the panel's remaining per-pass costs. The
descriptor implementation is not yet approved. The
[accepted plan](../plans/nfe0blnb-queue-shell-submit-and-pixel-writes-within-existing-custody-bounds.md)
owns the remaining performance gate. Evidence is under
`~/.local/state/sophia/development-evidence/t331-b3-20261017/`.

niltempus subsequently directed the shared descriptor design review. Its
[proposed lifecycle and byte layouts](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md)
answer the nine lock questions for both roles, with sealed memory first and
DMA-BUF capture behind the renderer ownership decision. This is design work;
t331's measured failure and the current byte contracts remain in force.
The draft now includes exact discovery/limits/formats layouts and a C/Rust
custody fixture matrix. Thirty-two packet vectors received independent review;
19 companion-object cases have local layout checks. Real ancillary/seal and
SDK custody fixtures remain unrun, and the design remains proposed.

## B2/B3 native pilot — October 17

The matched candidate is comparator `6c87721fa`, integration `4b16974`, Rust
SDK `01474a9` and C SDK `5246f96`. The timing peer uses Rust; these figures do
not establish C client performance. Each arm offered 1,000 frames at 60 Hz in a
60-second session. The bulk arm adds a 4 MiB upload to each panel frame. CPU and
allocation instrumentation run separately, with separate progress arms. The
replay's `comparison.json` uses whole-workload bounds, without interpolation or
subtracting instrumentation overhead.

| Workload and metric | Files | IPC | Files excess |
| --- | --- | --- | --- |
| Panel CPU, ms/uploaded MiB | 8.982–8.991 | 7.866–7.878 | 14.02–14.29% |
| Panel allocations/frame | 1,663–1,666 | 1,141–1,143 | 45.49–46.01% |
| Bulk CPU, ms/uploaded MiB | 1.6206–1.6209 | 1.4003–1.4006 | 15.71–15.76% |
| Bulk allocations/frame | 5,218–5,220 | 3,385–3,387 | 54.06–54.21% |

CPU includes all threads in Session and the peer, including foreign code, but
excludes other processes. Allocation counts cover their Rust global allocations
and reallocations, excluding foreign allocators. This synthetic byte-upload
workload does not measure a GPU producer's readback cost. It is a pilot, not the
full qualification matrix or an uninstrumented latency result.

Compared with the first-slice pilot, files CPU fell about 5.8% for panel and
4.0% for bulk; allocations fell about 10.5% and 8.6%. IPC CPU rose about 1.6%
in both workloads, so the narrower ratios are not entirely files improvements.
These are sequential campaigns, not repeated-run confidence intervals.

Separate progress arms bracket files owner passes/frame at 17.548–17.607 for
panel, against IPC 8.019–8.043, and 90.179–90.384 for bulk, against IPC
45.069–46.208. The previous files figures were about 22.6 and 102.3. Bulk still
has 67.863–67.999 pixel-bearing owner passes per frame; panel has 2.997–3.000.
These are observed activity counts, not exclusive wake causes or CPU attribution.
They support the one-msize input-budget explanation and rule out the earlier
69/4 bulk-pass projection for this implementation.

Keep t331 open with its existing gates. The recommended next design work is the
[proposed descriptor channel](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md),
starting with its accounting and shared lock/shell ownership decisions. It may
avoid repeated bulk byte transport, but no measured result yet establishes that
benefit. Keep the panel's request/pass and allocation costs explicit: three
pixel-bearing passes cannot explain its entire gap. Do not widen the server
budget, claim a descriptor pass, or start another fixed-cost slice from this
pilot alone. No candidate task is promoted and no new transport is implemented
by this result checkpoint.

## October 17 implementation and validation history

Production `a3c94bb86` implements the approved reusable writer (`ee22309ba`) and
imports signed Rust SDK `9d56625` and C SDK `30b7c1f`. Both source branches and
Sophia's first-slice adoption were pushed. Later checkpoints remain local as
noted below. The [decision](../decisions/7mls2e2q-reuse-a-shell-transaction-writer-without-changing-custody.md)
and [t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md#approved-reusable-writer-and-cumulative-ack-slice--october-17)
remain authoritative. No desktop installation has run.

The reconciled baseline captured all eight arms at comparator `363e06998`: merge `10d25e680`
carries production `116075541` and its existing SDK optimizations; the later
commit adds per-arm diagnostic output only. Offline replay reproduces all 8,000
frame joins and the cost reports. The original two bulk-allocation arms overlap
reviewer activity; clean recaptures now supply their comparison. The held-ACK Rust regression and C
negotiation-probe guard pass. Comparator `b67cb38d5` now carries the implementation;
its deterministic ledger reduces panel requests by about 30% and bulk requests
by about 16%, including startup. Fourteen native arms have now captured: eight
candidate cost arms, four separate progress arms, and clean recaptures of the
two provisional baseline bulk-allocation arms. Offline replay reproduces all
14,000 exact presentation joins, journal/cost reports and console recovery.
Comparator `9e3c741f8` adds separate owner-pass activity diagnostics; integration
`4b16974` reads the new format while retaining historical captures. These are
observations of work within passes, not inferred wake causes, and are excluded
from CPU/allocation builds. The completed campaign is under
`t331-reuse-20261017/native-candidate`; its exact comparator `9e3c741f8` and
integration `4b16974` identities remain in the plans and reports. The temporary
baseline checkout at `363e06998` was removed after its clean recapture.
The completed baseline lives under
`~/.local/state/sophia/development-evidence/t331-reuse-20261017/native-baseline`.
Any further native capture requires attended readiness and a fresh quiet window.

CPU-20 remains superseded preparation, with no capture. Cost-15 and perf-18 are
historical evidence, not qualification of this candidate. No quiet hold is active.
The pilot remains above the +10% ceiling: panel CPU +23.03–23.34% and allocations
+62.61–63.16%; bulk CPU +22.31–22.37% and allocations +68.48–68.62%. The full
matrix remains open. Request reductions alone do not explain those costs.

niltempus has accepted the [B2/B3 plan](../plans/nfe0blnb-queue-shell-submit-and-pixel-writes-within-existing-custody-bounds.md)
and directed implementation. The contract now permits queued final-fragment
submit and a four-write pixel window. The capture prerequisite is complete.
B2 is signed as Rust `7722502` and C `574a1eb`, adopted by signed Sophia `406f73f31`.
Rust's shell-client suite and strict Clippy pass; the full C suite passes.
Independent tests hold and reverse both replies, preserve early Submitted after
a failed record, reject incomplete-record retry and keep earlier C fragments
sequential. Initial candidates `b8b7b55`/`c0f0375` passed startup exchanges with
metadata and content negotiation on oldest supported export `116075541` and
current `feaf3cbc7`. The review follow-ups passed the current export again.
These are startup exchanges, not the entire export failure matrix. Logs are under
`~/.local/state/sophia/development-evidence/t331-b2-20261017/`.
The production export's incomplete/absent-staging and backpressure precedence
fixture passes for both writer paths. Further compatibility fixtures and B3
qualification remain open. No B2 native performance result exists yet.

The review follow-ups restore C's record-write ESTALE classification and preserve
a complete successful submit reply on terminal failure in both SDKs, even when
the record reply or journal event is missing. The ordinary AwaitSubmitted path
uses the same terminal rule; live progress still waits for the journal and its
covering ACK. Tests retain Unknown when no authoritative reply or valid event
establishes custody, and malformed journal data still fails validation. C reports
record ESTALE as a stale session; Rust retains its previous protocol-error
category. API-PARITY records that difference and B2's C consumer rebuild
requirement. Both signed follow-ups are upstream; B3 changes now sit above them locally.

Publication is complete: both signed SDK candidates and Sophia's result notes
are upstream. Exact archives have been imported locally. Both snapshot checks,
the Rust SDK suite and C production export/lifecycle gates pass. The full default
Sophia suite passes after adding the missing native-session guard to
`output_snapshot_allocations.rs`; that test also passes with its feature enabled.
Formatting, metadata and diff checks pass. After niltempus unlocked signing,
the guard was committed as `d3a94f878`, followed by B2 adoption `406f73f31`.
The installed desktop is unchanged.

B3 export coverage has begun locally: the real-socket split-upload test now
issues a short head and its tail before reading replies, checks tail EINVAL and
recovers the exact bytes. A direct export fixture verifies EAGAIN leaves the
cursor unchanged, queued tails refuse EINVAL while live, and a closing binding
answers ESTALE without more custody. Both pass. These tests do not establish
SDK windowing or expiry timing.

Both B3 SDK windows are signed and published: Rust `365e5f3`, C `63efea3`.
Documentation follow-ups `f3df891`/`303dfd0` record cancellation after a permanent
head refusal; the implementation bytes are unchanged.
Sophia `bba813fa8` records the export fixtures above. Rust gathers
four adjacent canonical units with separate tickets, matches replies by tag and
settles in issue order. Its tests cover credit refill across eight chunks, reversed
replies, short/EAGAIN heads with invalidated tails, End after drain and a terminal
status before write replies. C windows one borrowed buffer, with independent
tests for reversed replies, cursor recovery, idle wake, repeated backoff, explicit
low-level retry, contradictory tails and fencing. C's full suite passes. Rust's
shell/protocol suites and strict Clippy pass. Both also test iounit fragmentation;
C's eight-slot attached-client fixture proves a three-write window and refill.
The inline Rust window avoids a per-burst allocation; this is a storage property,
not a measured allocation-gate result.

C keeps the low-level client clockless and adds upload_retry; sophia_ss owns an
independent supplied-clock deadline and reports the earlier submit/upload wake.
Five control slots are reserved: default C session storage now has nine request
slots, while direct eight-slot clients use three upload credits. Both C structs
grow and consumers must rebuild. API-PARITY records the buffer/ticket distinction,
fragment scheduling and progress-or-time versus time-only retry. Recovery counts
remain in fixtures/instrumented evidence. Claude's C review found no blocker;
the control reserve correction is applied. Claude also reviewed Rust `365e5f3`
and C `63efea3` without adoption blockers. No B3
snapshot, comparator ledger or native pilot has run. No quiet hold is active.
SDK logs are under `~/.local/state/sophia/development-evidence/t331-b3-20261017/`.
Next: complete old/current export, remaining fault and fairness checks before
adopting B3; then collect
matched native evidence. The existing +10% gates remain unchanged.

The B2-only comparator checkpoint is signed and published as `87d135484`.
Its device-hidden supplied-owner diagnostic passes four 64-frame arms; files
panel counts remain 72 walk, 72 open, 648 read, 1,477 write and 68 clunk. Bulk
counts remain 136 walk, 136 open, 6,661 write and 132 clunk; reads are 1,032
against 1,031 in the earlier reusable-writer diagnostic. These totals include
startup. B2 removes a dependency wait, not requests. This fixture has no Session
owner loop, physical presentation or qualified CPU/allocation measurement.
Evidence: `t331-b2-20261017/candidate-ledger`, including per-arm source counters
and `comparison.json`. The comparator's existing mixed-activity labels still
include the previously reported Other overlap; do not infer wake causes from them.

Comparator `c014ad37f` adds an opt-in ordered shell trace. Three four-arm,
64-frame runs prove every steady B2 submit was dispatched before its record
reply finished sending: 320/320 panel and 512/512 bulk. Negotiation remains
sequential. The observation is dispatch, not socket arrival; it conservatively
proves that submit did not wait for the record reply. A separate fixture checks
partial reply delivery and a sequential negative control. A compile-time guard
rejects combining the trace with cost instrumentation; its allocation and timing
overhead are not performance evidence.

The extra bulk read is startup grouping. Runs 01 and 02 returned the first 168
journal bytes together; run 03 returned 112 then 56. Every later event-read
offset and reply length matches, including the final parked read. Thus the
traced 1,031/1,032 totals differ by one Tread and one Rread header, with identical
journal payload. This reproduces the earlier counter difference; the historical
untraced capture cannot identify its particular split. Evidence lives under
`t331-b2-20261017/ordered-trace-01`, `ordered-trace-02` and `ordered-trace-03`.
The first two include the separate progress sampler; the third omits it. All
ran before the trace commit, whose final changes only format code and clarify
dispatch wording. Raw summaries retain the earlier word "arrival"; interpret
their request phase as dispatch. B3 compatibility, fault and neighbor-service
checks are next. No B2/B3 native cost result exists yet.

niltempus approved completing B3 validation and one matched native pilot before
deciding whether to prioritize the proposed descriptor channel. The +10% gates
remain unchanged. The new supplied-clock socket fixture proves that partial
pixel scratch at one millisecond before the idle deadline does not renew it;
expiry fences later burst writes, and a late write or clunk from that fid cannot
affect a successor binding. All thirteen socket fixtures pass against current
export `0d79bd170` and oldest supported export `116075541`, using test-only
overlays on the latter. This establishes these export behaviors, not complete
B3 SDK interoperability or neighbor fairness. Logs and the old-export overlay
are under `t331-b3-20261017/`. No desktop or native measurement was started.

The same direct-export backpressure/tail fixture also passes both revisions.
C candidate `303dfd0` passes all three independent vector and native-file session
tests against both exports, including allocation, upload, cancellation and ACKs.
The test now accepts an explicit absolute `SOPHIA_C_SDK_SOURCE` directory so a
candidate can be tested before replacing the vendor pin; default runs still
check the pinned SDK. Rust candidate `f3df891` passes independent metadata and
content startup against both exports. These do not prove a full four-credit
window against production or neighbor fairness; those gates remain open.

Rust test-only follow-up `10c9226` adds an independent upload peer. It queues
five canonical chunks followed by End, checks every ticket becomes Stored,
then cancels a second resource. Against both exports the host verifies all
262,144 accepted bytes exactly; both runs pass, as does strict example Clippy.
The generic ignored fixture accepts an absolute `SOPHIA_SHELL_UPLOAD_PEER`
executable. This proves the multi-chunk path and End/cancellation interoperability,
not the maximum simultaneous wire depth or neighbor latency. The ordinary C
peer also exercises transport fragmentation with a 4,096-byte msize. Remaining
fault audit, depth/fairness diagnostics, adoption and the native pilot are open.

The fault audit found a Rust custody error before adoption: a zero or oversized
Rwrite removed the outstanding tag before validation, so teardown could label
issued bytes DroppedUnsent. Signed Rust `c1d3515` preserves Unknown and permits
Stored only for a valid positive reply within the issued length. The regression
failed before the fix and passes after it. The full shell suite and strict Clippy
pass. C already failed closed; signed `17f4b0f` adds matching invalid-count tests
and parity wording, with its full suite passing.

Comparator `e9dafb67a` stages those exact archives for qualification; production
has not adopted B3. Both snapshot checks pass. Two four-arm 64-frame traces
have identical pixel totals: panel 192 writes / 11,796,480 bytes; bulk 4,608 /
280,231,936. Short replies, EAGAIN, invalidated tails and other pixel errors are
all zero. B2 overlap remains 320/320 panel and 512/512 bulk. The first run used
Rust `10c9226`; the second includes the custody fix. Evidence is under
`t331-b3-20261017/ordered-candidate-01` and `ordered-candidate-02`.

The server dispatch trace sees at most one panel or two bulk pixel writes before
their replies finish sending. This does not measure the client's four-credit
window: the server reads at most 65,536 bytes per connection turn. A 4 MiB
payload plus request headers needs at least 65 input turns, so 69/4 was never a
valid server-pass prediction. The accepted plan now records this bound; B3 does
not change it. Native benefit remains unmeasured.

A separate two-export driver fixture keeps four writes queued for 128 turns,
completes 448 writes within the cumulative read budget, and answers a neighbor's
event read and control request on every supplied-owner iteration. One run reports
42.2 microseconds median, 74.4 microseconds p99 and 76.7 microseconds maximum
for that iteration. These are unqualified fixture timings, not native Session
latency or real-role performance. All ten Unix-driver tests pass. The remaining
fault review and native pilot still control adoption and qualification.

## First-slice native pilot — October 17

All fourteen arms captured 1,000 frames at 60 Hz, 1080p, scale 1, VRR off and
idle host load. Offline collection, journal metrics and exact kernel joins
reproduce the recorded reports. All console recovery checks pass. The three
other agents report no crunch activity during the hold; `quiet-window.txt`
records their confirmations and release. This is a pilot, not the full gate.

| Workload / measure | Files | Paired IPC | Conservative excess |
| --- | --- | --- | --- |
| Panel CPU ms/MiB, ordinary allocator | 9.538267–9.548299 | 7.741651–7.752949 | 23.03–23.34% |
| Panel allocations + reallocations/frame | 1,857–1,860 | 1,140–1,142 | 62.61–63.16% |
| Panel + 4 MiB CPU ms/MiB, ordinary allocator | 1.687707–1.688053 | 1.379412–1.379823 | 22.31–22.37% |
| Panel + 4 MiB allocations + reallocations/frame | 5,708–5,711 | 3,387–3,388 | 68.48–68.62% |

The clean baseline bulk-allocation recaptures are 6,078–6,080 files and
3,384–3,386 IPC operations/frame. The earlier panel baseline was 2,082–2,086
files and 1,142–1,144 IPC. Candidate allocations are lower, but neither workload
passes +10%. CPU and allocation figures above come from separate build modes;
do not substitute allocation-instrumented CPU for the ordinary-allocator result.

The IPC panel CPU arm also rose from 7.529965–7.541559 to 7.741651–7.752949
ms/MiB, about 2.8%. The files arm fell from 10.405274–10.418833. The ratio
improvement therefore includes comparator drift; it is not entirely a files-side
saving. Projections based on allocations per historical pass remain hypotheses,
not grounds to change the gate or assign the remaining gap to another task.

Separate progress runs bracket Session passes over the peer's full workload:
22.615–22.672 per panel frame versus IPC 8.018–8.040; 102.297–102.389 per bulk
frame versus 45.014–45.532. Files observes about five record, five submit and
three pixel passes per panel frame; bulk observes eight, eight and 72. These
categories overlap and are not wake causes. There is no matched native progress
baseline for the first slice, so historical pass counts do not prove its exact
pass saving. The results support testing B2/B3; they predict no automatic pass.

Evidence: `~/.local/state/sophia/development-evidence/t331-reuse-20261017/native-candidate/`
contains immutable run directories and `offline-replay/{summary,comparison}.json`.
The latter records exact ratios and false +10% verdicts for both cost measures
on both workloads. The disposable replay script is
`/tmp/t331-replay-reuse-candidate.py`; reports and raw logs are the retained evidence.

## SDK review follow-ups and candidate ledger — October 17

The subsequent [B2/B3 design](../plans/nfe0blnb-queue-shell-submit-and-pixel-writes-within-existing-custody-bounds.md)
was reviewed as proposed, then accepted by niltempus on October 17. It preserves
tail fencing, submit errno precedence, per-ticket custody and canonical-chunk
idle renewal. The frozen first-slice capture remains unchanged. B2 is now a local
SDK candidate as described in Current resumption; B3 has signed source candidates
but is not yet adopted into Sophia.

Signed Rust `9d56625` proves that the next retained-fid record is outstanding
while the covering ACK reply is withheld. Signed C `30b7c1f` waits for Negotiated
before discovery even when the caller has already acknowledged Submitted; a
scripted delayed-Negotiated test covers that ordering. C still waits for the
confirmed ACK before sending its next record, as documented in both parity files.
The full Rust shell suite, strict all-target shell Clippy, full C suite, both
vendor gates, Sophia runtime tests and independent C/Rust startup checks against
the production export pass. Production adoption is `a3c94bb86`.

Signed comparator merge `b67cb38d5` keeps IPC isolated and passes the supplied-owner
progress diagnostic on both wires. Across 64 frames, including startup/teardown:

| Request | Panel before → after | Panel + 4 MiB before → after |
| --- | --- | --- |
| Walk | 391 → 72 | 647 → 136 |
| Open | 391 → 72 | 647 → 136 |
| Clunk | 388 → 68 | 644 → 132 |
| Read | 648 → 648 | 1,032 → 1,031 |
| Write | 1,541 → 1,477 | 6,725 → 6,661 |
| Total | 3,359 → 2,337 | 9,695 → 8,096 |

The retained writer removes steady transaction lifecycle requests, with its
one-time discovery included. Write counts fall by 64 per workload; the single
bulk-read difference is not evidence of a general read reduction. IPC continues
to issue no 9P requests. This fixture has no Session owner loop or physical
presentation: its zero owner-pass counter is not a native result. CPU and
allocation savings were unmeasured at that checkpoint; the native pilot above
now records them. Evidence is under
`~/.local/state/sophia/development-evidence/t331-reuse-20261017/sdk-followups`
and `candidate-ledger`, with the comparison in `candidate-ledger/comparison.json`.

### ACK cadence finding and remaining attribution

Claude's independent review reproduces the lifecycle arithmetic: panel loses
319 walks, 319 opens and 320 clunks across 64 frames; bulk loses 511, 511 and
512. The one-time reusable-node probe accounts for the extra walk and open.
The measured write reduction is only one per frame in each workload. Do not
carry the earlier hypothetical panel ACK reduction from 11 to 5 into the pilot.
The unchanged pixel writes and nearly unchanged reads are expected for this slice.

The source offers an explanation for the limited ACK batching: the server retries
waiting reads after requests, and Rust sends eligible ACKs when its bounded drive
finds no further progress. Journal entries delivered separately can therefore
each trigger an ACK before the next entry arrives. The diagnostic's aggregate
counts do not identify which event pair shared a drive call or prove this cadence
in the native owner loop. In particular, the old 53/152 steady-frame ledger must
not replace the reconciled 64-frame totals, which include startup and teardown.

Removing requests does not establish an equal reduction in Session passes.
Clunks are not awaited, and several requests can share a pass. Predictions of
about 17 panel passes or a particular allocation gap remain hypotheses. The next
diagnostic must report owner passes on both wires, attribute setup, record,
submit, pixel, flush-only and cadence work where directly observable, and retain
unattributed or overlapping work explicitly. Do not infer those causes from
request totals or add diagnostic logging to CPU/allocation acceptance arms.

End-of-pass pending-read service, a different ACK cadence, queued submit and
pipelined pixel writes remain possible follow-up investigations. They are not
authorized implementation extensions of this slice; the matched native pilot
must establish the remaining costs before selecting another change.
The [9P transport prior-art survey](yhsocuvc-what-prior-art-on-9p-transport-cost-says-about-the-shell-wire.md)
records independent research and the deferred t334/t335 candidates.

### Owner-pass observation method

Comparator `9e3c741f8` uses a thread-local, allocation-free scope around each
owner-loop pass. Scope exit counts completion even on early return or continue.
Setup observes core 9P walks/opens across roles; shell writes distinguish record,
submit and pixel activity. IPC classifies decoded resource chunks as pixels and
other decoded frames as records, with no artificial separate submit operation.
Cadence observes a due pacing/deadline predicate, not a completed presentation.
Other includes core reads/writes/clunks and IPC input bytes; it deliberately
overlaps refined categories. Mixed passes remain explicit. Flush-only means
observed output bytes without another category, and unattributed means no
instrumented observation; neither establishes that no other work occurred.
Mixed includes the Other overlap: a single shell write or decoded IPC frame can
set both bits. Other is not residual, and mixed does not mean multiple kinds of
shell work. Presence counters cannot establish ACK/read ordering or identify
which event pair shared a reply; that requires an ordered trace.

Tests cover allocation-free accumulation, repeated activity counted once per
pass, mixed/unattributed/flush-only categories, bounded formatting and historical
schema parsing. The native-feature build check and both-wire supplied-owner
diagnostic pass. That fixture has no real owner loop, so native attribution still
requires separate progress captures. Diagnostic schema 3 cannot be mixed with
cost instrumentation. Evidence is in `t331-reuse-20261017/pass-ledger`.

The three release modes are built separately and hashed at their cargo-slot
paths. Baseline allocation executables retain their original hashes through
hard links within the original cargo slot, not binary copies in evidence.
Fourteen fresh plans pass source/artifact checks and device-free argument
validation. Initial preparation refused a changed preflight hash; the current
integration preflight was rebuilt and explicitly pinned, and the refusal log
is retained. The helper performs no builds and refuses existing run directories.
No quiet hold or desktop installation is active.

## Reconciled native baseline — October 17

The eight-arm baseline uses comparator `363e06998`, Rust SDK `f1cd924`, and
integration `fd2cb6c`. Each arm presented all 1,000 offered frames at 60 Hz.
Independent `collect`, `join` and `cost-metrics` replay reproduces the saved
frame joins and cost reports. Evidence and replay are under
`~/.local/state/sophia/development-evidence/t331-reuse-20261017/native-baseline`.

| Workload | 9P CPU ms/MiB | IPC CPU ms/MiB | 9P Rust alloc/realloc per frame | IPC Rust alloc/realloc per frame |
| --- | --- | --- | --- | --- |
| Panel | 10.405274–10.418833 | 7.529965–7.541559 | 2,082–2,086 | 1,142–1,144 |
| Panel + 4 MiB | 1.697965–1.698256 | 1.375569–1.375945 | 6,079–6,081 (provisional) | 3,386–3,388 (provisional) |

CPU comes from the CPU-only arms; allocations come from separate Rust-global
arms. CPU covers all threads in Session and the peer, including foreign code;
allocations cover their Rust global allocators only. Other processes are excluded.
These are conservative measurement bounds, not confidence intervals across runs.
The baseline predates reusable writers and ACK batching; it cannot establish
their effect or qualify the candidate.

Claude's review pane reported read-only searches and git reads at
11:56:53–11:58:58 UTC during the requested hold. Approximate realtime mapping
places the bulk-allocation intervals at 11:57:37–11:57:54 and
11:58:42–11:58:59, so both remain provisional, with all samples retained.
The CPU arms and panel-allocation pair ended before that activity. Muse reports
no activity. The quiet hold is released. See `quiet-window.txt` and
`offline-replay/summary.json` for provenance and timing limits.

## Implemented writer and ACK slice — October 17

The export reserves one bounded buffer at open, moves it through assembly and
accepted custody, and recycles it only after a covering ACK. Refused assemblies
retain their errno and readable bytes; malformed or foreign-epoch submit
envelopes do not poison valid staging. Expiry and revocation permanently fence
the token. Both SDKs probe after negotiation and its ACK, use legacy transactions
only after lookup ENOENT, retain successful writers and replace refused ones.

Rust batches automatic ACKs at existing drive boundaries. C preserves its
explicit cumulative ACK call and confirmed-ACK barrier; no automatic policy was
inserted into a manual API. Independent tests consume three records and send
one ACK in both SDKs. Manual control and snapshot holds remain covered.

The full Rust shell suite, strict shell Clippy, full C suite, both immutable
vendor gates and Sophia's runtime test suite pass. Both independent startup
peers pass the real export. Six new export tests cover reuse, refusal, replay,
ACK barriers, cancellation, expiry, revocation, idle failures and backpressure.
The real-wire fixture observes reusable writes and proves that the covering ACK
precedes the next record. These are protocol checks, not physical acceptance.

Evidence is under `~/.local/state/sophia/development-evidence/t331-reuse-20261017`.
The reconciled supplied-owner diagnostic completes 64 frames on each wire and
workload. Its per-arm totals include startup and teardown; do not present them
as steady-frame counts. Files panel: 391 walks, 391 opens, 648 reads, 1,541 writes
and 388 clunks (3,359 requests). Files bulk: 647 walks, 647 opens, 1,032 reads,
6,725 writes and 644 clunks (9,695 requests). IPC issues no 9P requests. This
fixture has no Session owner loop: it cannot attribute native owner passes or
establish performance savings. Raw journals and counters are in `baseline-ledger/`.

## Reconciled request ledger — October 17

Comparator peer `9c527d566` has the same workload as batching-14 `bf19952dd`.
It sends panel Begin, three canonical pixel chunks, End, FrameDemand, a grouped
Candidate, then Retire of the previous resource after Presented. The first
frame omits Retire; allocation setup precedes the measured loop. The grouped
Candidate is one file transaction and three IPC records.

The bulk case first sends a separate probe Begin, 69 pixel chunks, End and Retire,
waits for release, then runs the complete panel sequence. Negotiated msize is
65,536, its write payload bound 65,513, and canonical chunks are 61,440 bytes.
Neither larger msize nor short-write avoidance reduces these pixel-write counts.

| Steady-frame requests | Panel | Panel plus bulk |
| --- | ---: | ---: |
| Transaction walk/open/clunk | 15 | 24 |
| Upload walk/open/clunk | 3 | 6 |
| Record and submit writes | 10 | 16 |
| Pixel writes | 3 | 72 |
| Journal reads | 11 | 17 |
| ACK writes | 11 | 17 |
| Total | 53 | 152 |

ACK and read counts match batching-14 but can vary with scheduling. Clunk is
fire-and-forget, walk/open already share a setup batch, and idle relay counts
record skipped core turns. Requests do not each cause a flush. IPC sends ten
panel records and receives six; the probe adds 72 sent and three received.
Those record counts are not owner passes or transport syscall counts.

`Staging::new` uses `Vec::new`; it constructs an empty object without allocating.
First record bytes, fid tables, request decoding and owner passes need separate
allocation attribution. Ratios of allocations to passes do not establish an
exclusive causal cost per pass. The reusable writer predicts 15/24 fewer requests,
not a native gate verdict or a fixed number of eliminated passes.

## Earlier resumption — October 16

The audit identifies two concrete copy paths to measure: completed journal reads
allocate a temporary payload before copying it into the server reply buffer,
and the SDK copies upload payloads into its output buffer. Neither is a measured
explanation for the whole cost gap. Rio's request reuse is less novel for Sophia
than it first appears; its event waits do not remove custody checks.

Exact sizing now removes the two reallocations previously measured when gathering
four records. Unbuffered reads still allocate once; encoding into warmed
output storage and empty pending reads allocate nothing. The follow-up below
confirms one SDK payload copy, zero staging allocations on a second same-size
upload, and one socket send per upload in the isolated fixture. The approved
journal scratch experiment is now implemented: warmed journal reads allocate
nothing in all five fixture cases. Cost-11 native allocations fell only about
0.4%; CPU changed +1.8% for panel and -1.0% for bulk against cost-10. The gate
still fails. Both payload copies remain. SDK vectored output targets
copy traffic, not the warmed allocation count. The approved t331 slice excludes a new vectored
output ownership API; implementation needs a separately reviewed scope. t332
remains investigation only. Keep the installed desktop untouched.

## Maintained references checked October 16, 2026

| Project | Observed activity | Relevance |
| --- | --- | --- |
| [9front](https://github.com/9front/9front) | `a411fb63c`, October 16 | Complete system; rio, drawing, file services and kernel mechanisms. |
| [9legacy](https://9legacy.org/patch.html) | Patch page updated October 8 | Conservative fixes and individually reviewable kernel/library patches. |
| [plan9port](https://github.com/9fans/plan9port) | `b6564bd96`, August 26 | Unix-hosted libraries and applications, including lib9p and devdraw; not a complete OS. |
| [Go p9](https://github.com/hugelgupf/p9) | `3fd948847`, June 25 | 9P2000.L client/server; response and encoding storage reuse, separate payload vectors. |
| [diod](https://github.com/chaos/diod) | `de51d1ee1`, June 23 | C 9P2000.L server; request queues, cancellation and worker ownership. |

These dates establish maintenance, not recent performance work. Linux and QEMU
remain references from the [earlier comparison](lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md#t331-established-9p-implementation-comparison).

The complete 9front source tree is at `~/src/9front`, revision
`228167f32bf5b14fcb41e52eab527a1c8531638a` (September 29), with shallow history.
This audit reads that checkout, not the newer upstream head. Source hashes,
downloaded Go/diod files and maintenance records are retained in
`~/.local/state/sophia/development-evidence/t331-plan9-comparison-20261016/`.

## Rio: reuse has limits, and waits include cancellation

In [xfid.c](https://github.com/9front/9front/blob/228167f32bf5b14fcb41e52eab527a1c8531638a/sys/src/cmd/rio/xfid.c),
`xfidallocthread` recycles Xfid records with their workers and channels. It creates
more when the free list is empty; the inspected path has no fixed pool cap.
Sophia must preserve its explicit bounds rather than copy that growth policy.

Rio still allocates a message buffer per request. In
[fsys.c](https://github.com/9front/9front/blob/228167f32bf5b14fcb41e52eab527a1c8531638a/sys/src/cmd/rio/fsys.c),
`filsysproc` allocates `messagesize + UTFmax`, reuses it for the response, then
frees it in `filsysrespond` or `filsyscancel`. Reusing Xfid does not pool every
wire buffer.

`xfidread` waits on a particular window's data channel, window deletion and the
request's flush channel. The lesson is to represent every reason a request must
resume, including cancellation and lifetime changes. It is not permission to
wait only for data. The [rio manual](https://9p.io/magic/man2html/4/rio) also
explains that the draw device multiplexes itself: rio does not mediate application
access to it. Rio is not a comparable authenticated bulk-upload benchmark.

Sophia already retains connection input/output vectors and waiting/unwritten
queues (`crates/sophia-9p/src/connection.rs`) and a driver read buffer (`unix.rs`).
Waiting reads are small records, not new workers. An empty journal read returns
Pending at the tail without allocating. Retrying checks authority; delivery
checks remain live through partial writes. Rio's design does not eliminate these
obligations. The [change-token rejection](lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md#t331-change-token-audit)
still applies.

## Go p9: separate payload vectors expose a remaining SDK copy

At the pinned revision, [transport.go](https://github.com/hugelgupf/p9/blob/3fd948847ea46b413f8b98c8f226c6bb716807be/p9/transport.go)
gets an encoding buffer from `dataPool`, then passes header, encoded fields and
payload as separate vectors to `WriteTo`. Payload cleanup follows send. This
avoids concatenating payload into the encoding buffer. Whether it uses one
vectored syscall depends on the writer; this is not kernel zero-copy.

Sophia's vendored `sophia-9p-client/src/pipeline.rs::submit` still copies both
body slices into `out_buf`. Earlier cursor fixes avoid shifting sent prefixes,
but not this initial copy. A nonblocking vectored API must retain payload ownership
through partial sends; the current borrowed-slice API cannot keep the caller's
slice. Preserve byte/tag charges, flush reserves, secret scrubbing and terminal
settlement. Measure this path before designing a new ownership API.

## Diod: useful queue discipline, not a reason to add threads

[srv.c](https://github.com/chaos/diod/blob/de51d1ee1bd5ccf1d8c16b96227c8bb03ec50106/src/libnpfs/srv.c)
keeps queued and working request lists under server synchronization and uses
pthread workers in thread pools. `np_req_alloc` still allocates each request.
This provides a cancellation and ownership comparison, not evidence that threads
will improve Sophia's inline owner service. No scheduler change follows.

## Concrete candidates

The narrowest server-side candidate is completed-read storage. In
`sophia-9p/src/journal.rs::read`, a nonempty read gathers retained records into a
fresh vector. `connection.rs::read_now` returns a Read reply, then
`wire.rs::encode` copies its payload into the retained output vector. Temporary
storage can allocate or grow even though output storage is reusable.

An isolated fixture should count allocations and both copy stages for one-record
and multi-record reads, including partial offsets. Compare bounded scratch reuse
with a read-into-buffer design. Preserve acknowledgement retention and the
node/epoch delivery guard: acknowledgement must not free bytes still needed by
an unwritten reply. This is a source-proven copy path, not a whole-frame saving.

The second candidate is the SDK upload copy. Server-side
`connection/in_place.rs` already receives uploads into admitted storage; a generic
call for zero-copy reception would duplicate existing work. The client copy is
a different ownership boundary. Broad request pooling is lower priority without
allocation evidence.

## Isolated completed-read measurement

`crates/sophia-9p/tests/journal_read_cost.rs` measures the real Journal read and
wire encoder separately, with output capacity prepared outside the measured
region. One test runs all cases to avoid unrelated test threads polluting the
process-wide allocator counter. Assertions verify payload bytes, unchanged
retention until acknowledgement and final record release. The baseline below
predates exact sizing; the fixture now rejects reallocation regressions.

| 1,000 reads | Read allocations | Read reallocations | Encoding allocation operations |
| --- | ---: | ---: | ---: |
| One 256-byte record | 1,000 | 0 | 0 |
| Four records, 1,024 bytes | 1,000 | 2,000 | 0 |
| Four records, offset 17, count 700 | 1,000 | 2,000 | 0 |

All three cases also perform 1,000 empty-tail reads with zero allocation
operations. The test passes. Output is retained as `journal-read-cost.txt` in
the comparison evidence directory. These results isolate allocation churn; they
do not count native per-frame frequency, copied bytes or socket writes.

### Exact sizing result

`Journal::read` now reserves `min(count, tail - offset)` after validating the
offset and handling empty reads. All three baseline cases now use one allocation
and zero reallocations per completed read. Two additional cases request
`u32::MAX` bytes from partial offsets; the allocation equals the readable range,
not the requested count. Encoding and pending reads still allocate nothing.
Output is retained as `journal-read-cost-after.txt` in the same evidence directory.

This removes buffer growth, not the initial allocation or either payload copy.
Acknowledgement retention, read bytes and delivery guards are unchanged. The
same change and fixture are in comparator `840a6824f` to keep shared-core costs fair.
Native CPU/MiB and allocations/frame have not been measured for this change.

## Upload staging and copy accounting

`tests/upload_copy_cost.rs` exercises the vendored SDK's real `Pipeline::write`
and `poll` with a scripted Unix peer. Version negotiation finishes and its thread
joins before allocator snapshots. The fixture measures only queueing, then
overwrites the caller's payload before polling. The peer receives the original
bytes and returns a matching Rwrite. This confirms ownership of the queued copy;
it does not benchmark a shell export or authentication.

| Payload bytes | First staging allocations | First allocated bytes | Second staging allocations | Wire bytes per request |
| ---: | ---: | ---: | ---: | ---: |
| 256 | 2 | 407 | 0 | 279 |
| 4,096 | 2 | 4,247 | 0 | 4,119 |
| 65,513 | 2 | 65,664 | 0 | 65,536 |

No staging reallocations occur. The second same-size request allocates no bytes.
This is two requests per fresh connection, not a mixed-size steady-state claim.
The source's `submit` copies the payload once into `out_buf`, in addition to
23 bytes of framing. A 4 MiB upload therefore copies 4 MiB at this stage,
excluding framing and any compaction or allocator movement; this is a derived
byte count, not a measured 4 MiB workload.

An own-process strace records one successful `sendto` per Twrite for all six
requests, with the byte counts above. No partial send, EAGAIN or vectored send
occurs in this fixture. This does not predict syscall counts under native
backpressure. Retained evidence is `upload-copy-cost.txt` and
`upload-copy-syscalls.txt` in the comparison directory.

The journal fixture now reports logical bytes gathered and encoded separately.
For 1,000 reads of 1,024 bytes, each stage copies 1,024,000 payload bytes:
2,048,000 total. The partial 700-byte case copies 700,000 at each stage. These
counts follow the inspected slice copies and validated payload lengths, not
hardware copy counters; they exclude kernel traffic and allocator movement.
Results are in `journal-copy-cost.txt`.

### Journal scratch experiment

The owner approved this experiment. Connections now retain one scratch payload
only for exports that enable buffered reads. The default export method preserves
existing behavior; shell Events uses the journal's buffered method, forwarded
through `AuthenticatedFiles`. Encoding still copies into independent guarded
output before recycling the payload. Retained capacity cannot exceed the
negotiated read limit (frame limit minus 11); reset and close release it. The
shell contract accounts for this additional retained memory explicitly.

The fixture recycles a pre-sized buffer across 1,000 reads for each of the five
cases, measuring zero allocations and reallocations. The original fresh-buffer
cases still measure one allocation per read. Evidence is retained as
`journal-buffered-read-cost.txt`. Core tests verify reuse while earlier replies
are queued, partial delivery followed by revocation, pending/error cleanup,
oversized replies and scratch, and version reset. Existing journal tests retain
acknowledgement coverage. The full production core suite and 106 runtime library
tests pass (one runtime test ignored); strict core all-target and runtime library
Clippy pass. The comparator passes the same suites and Clippy checks. Native
savings were then tested in cost-11, below.

### Native cost-11 result

All eight 60 Hz pilot arms completed on comparator `8bbef6e00` (production
source `deae479d6`), with 1,000 frames each. Offline replay matches costs,
journal metrics and all kernel joins; console recovery is clean. CPU results
below come from ordinary-allocator runs; allocation counts come from separate
instrumented runs. Ranges are accounting bounds, not confidence intervals.

| Workload | 9P CPU ms/MiB | IPC CPU ms/MiB | 9P allocations/frame | IPC allocations/frame |
| --- | ---: | ---: | ---: | ---: |
| Panel | 11.526354–11.537126 | 7.641611–7.653430 | 2,321–2,324 | 1,140–1,143 |
| Panel + 4 MiB | 1.791618–1.791863 | 1.398314–1.398578 | 6,476–6,478 | 3,385–3,387 |

At interval midpoints, 9P allocations fell 0.39% and 0.38% versus cost-10.
CPU rose 1.83% on panel and fell 0.96% on bulk. The current CPU gaps to IPC are
50.8% and 28.1%; allocation gaps remain about 103.5% and 91.3%. Neither workload
passes the +10% ceiling. This supports a small allocation reduction, not a
repeatable CPU win or acceptance of the scratch candidate. Exact sizing and
scratch reuse changed together, so this pilot cannot separate their effects.

Raw captures and `offline-replay/summary.json` are under
`~/.local/state/sophia/development-evidence/t252-authenticated-20261015/t331-native-cost-11/`.
The quiet hold is released; Claude reports no work after its initial 13:06:38Z
status read and confirmation. All run directories are occupied. Further work
should attribute the remaining Session allocations before another transport API
change; the observed journal reduction is too small to explain the overall gap.

Direct encoding into queued output may remove a copy too, but needs a separate
review of rollback, output bounds and delivery guards. SDK owned vectored output
likewise remains separately scoped. Neither the copy counts nor strace establish
CPU cost, native frequency or the share of t331's remaining gap.

## Validation and limits

### Service-progress diagnostic implementation

Comparator `47ac3f2f7` adds `comparator-progress`, separate from CPU, allocation
and phase-profile features. Combining them fails compilation. Hot-path hooks
only increment relaxed atomic counters: no allocation, timestamps, formatting
or I/O. The existing sampler emits one bounded progress record every eighth
tick and at lifecycle boundaries; the peer emits at frame boundaries. Snapshots
have their own timestamp bounds and are not atomic across fields.

Counters cover Session passes, component passes, core polling turns/flushes,
accepted/stale readiness handoffs, core read/write bytes, encoded replies,
pending-read attempts, peer content polls and admitted upload/receive waits.
Core totals include every server in the process. Encoded replies include errors
and are not presentation completions; use the journal for completed frames.
The wait count excludes authentication setup. Counts can reveal amplification,
but do not by themselves prove that authority checks or idle passes are useless.

The full core suite passes with instrumentation. Focused tests prove counter
updates/snapshots allocate nothing, pending attempts do not count as replies,
real-socket byte totals are exact, and accepted/stale handoffs increment the
right counters. Runtime tests verify bounded records and explicit progress mode;
strict Clippy passes for core, runtime/peer and native CLI. The mixed-mode
negative compilation check fails as intended.

Integration `59bee7f` validates schema, process identity, monotonic counters and
timestamps, duplicate/missing fields, mixed modes and sample bounds. Its tests
and strict library Clippy pass. Native reports preserve progress samples with
`costs: null`; cost/latency arms reject progress records. Artifact, foreground
tty, duration, journal, kernel-join and recovery checks remain in force.

Four optimized one-minute plans are frozen in
`t252-authenticated-20261015/t331-native-progress-12`: files/IPC, panel/bulk,
1,000 frames each at 60 Hz. Preparation and device-free argument validation pass.
Build artifacts stay in Cargo slots; no native arm has run. The helper now selects
these fresh diagnostic captures. A new coordinated quiet window is required.
Analysis must align counter snapshots with the offered-frame interval and retain
boundary uncertainty rather than divide whole-session startup totals by frames.

### Service-progress results

All four progress-12 arms completed on comparator `47ac3f2f7` and integration
`59bee7f`. Offline journal and native-join replay match the stored reports,
with 1,000 frames each and clean console/keyboard recovery. An independent
raw-record parse matches every stored progress sample and verifies per-process
identity, order and monotonic counts. Costs remain null.

The interval spans the first peer frame-start progress snapshot through the
last frame-end snapshot. Session lower bounds subtract snapshots wholly inside
that interval; upper bounds use enclosing snapshots. This retains sampler
boundary uncertainty instead of including whole-session startup. Values below
are counts per offered frame, rounded for display; they are not CPU times or
statistical confidence intervals.

| Counter | Files panel | IPC panel | Files bulk | IPC bulk |
| --- | ---: | ---: | ---: | ---: |
| Session/component passes | 33.08–33.16 | 8.03–8.20 | 120.07–120.12 | 45.02–45.51 |
| Core polling turns | 71.29–71.46 | 12.17–12.36 | 245.27–245.39 | 49.15–49.67 |
| Core flush calls | 138.32–138.62 | 16.06–16.40 | 489.28–489.47 | 90.04–91.01 |
| Accepted handoffs | 32.08–32.13 | 0 | 119.06–119.09 | 0 |
| Stale handoff fallbacks | 0 | 0 | 0 | 0 |
| Core replies encoded | 52.94–53.03 | 0–0.021 | 151.97–152.03 | 0–0.021 |
| Pending-read attempts | 30.08–30.12 | 0–0.021 | 114.06–114.08 | 0–0.021 |
| Peer content-poll calls | 38.567 | not instrumented | 87.046 | not instrumented |
| Peer wait API calls | 32.568 | not instrumented | 114.637 | not instrumented |

The handoff is exercised; stale observations do not explain the remaining gap
in these intervals. Files runs roughly four times as many Session passes for
panel and 2.7 times as many for bulk. This supports investigating service
amplification. It does not prove those passes unnecessary: authentication,
revocation, deadlines and protocol dependencies can require them.

Core counters include other 9P services even in the IPC arm, so do not label
their totals as shell-only traffic or infer one server's polling frequency.
Source inspection also confirms FileWire::wait_for_io can return immediately
when service is pending: peer wait counts are calls, not poll syscalls or sleeps.
The next bounded investigation should account for the roughly 53 panel and
152 bulk replies by request kind and SDK transition, then identify which
dependent exchanges force additional owner passes. Do not infer a new cache,
upload concurrency or skipped authority checks from these counts alone.

Evidence is `t331-native-progress-12/offline-replay/summary.json`, alongside
replayed journal/offers/samples and raw captures. It retains exact count bounds,
boundary snapshots and source hashes. All output directories are occupied.
Claude reports only initial 13:41–13:42Z status reads and confirmation, no later
crunch work during the hold. The hold is released.

### CPU attribution review after cost-11

Reclassified the retained perf-08 stacks by exclusive leaf, selecting exactly
the earlier Session/peer PIDs and whole-workload counter intervals. Selected
sample counts match all four original reports. This reuses existing evidence;
it is not a profile of the new scratch candidate. Values below are summed sample
periods across 1,000 frames, in milliseconds, not exact function runtimes.

| Exclusive leaf group | Files panel | IPC panel | Files bulk | IPC bulk |
| --- | ---: | ---: | ---: | ---: |
| Recognizable allocator functions | 59.2 | 37.1 | 93.3 | 83.2 |
| memcpy/memmove | 115.3 | 80.2 | 639.9 | 544.6 |
| memset | 7.0 | 2.0 | 44.1 | 31.1 |
| Unknown leaf | 36.1 | 44.1 | 44.1 | 41.1 |
| Other leaves | 1,042.1 | 586.8 | 4,668.0 | 3,472.4 |

Recognizable allocator leaves explain about 22 ms of the sampled panel
difference and 10 ms of the bulk difference. This grouping misses inlined or
unresolved allocation work and does not include allocator-induced cache effects;
it cannot establish the full cost of allocating. Still, allocation counts alone
are a poor guide to this observed CPU gap. Copy leaves differ by about 35/95 ms,
but missing caller stacks prevent assigning most of that difference to a
particular transport copy.

The strongest named user-space differences include syscall wrappers, clock
reads, custody checks, server wake service and the Session loop. The bulk
`__vdso_clock_gettime` leaf difference is about 128 ms. Grouping clock leaves by
their nearest visible application caller puts 88/41 ms in the files/IPC Session
loop; files client poll_io accounts for another 24 ms. These are execution
samples, not call counts. A clock optimization would be premature: more clock
work may simply indicate more service turns. Perf-09 independently puts excess
on-CPU polling in several owners, not only the server poll already targeted by
the readiness handoff.

The next diagnostic should count service turns and useful progress per offered
frame on the current candidate: Session loop passes, component service passes,
server turns/flushes, client waits, requests completed and bytes advanced. Count
readiness-handoff use and fallback separately. Use fixed numeric counters and
flush snapshots outside the measured hot path; do not add formatted records or
a clock query to each call. This can distinguish extra protocol progress steps
from repeated unchanged-state service before selecting a patch. Preserve all
authority checks and deadlines; no liveness cache or once-per-loop skip follows
from these stacks. A subsequent CPU profile must use the same current candidate.

Evidence: `cpu-attribution-review.json` (source hashes and complete exclusive
leaf maps) and `clock-callers-review.json` in the Plan 9 comparison directory.
No new native run, runtime patch or acceptance change was made for this review.

The owner questioned whether the current metric is guiding the right work.
Allocations/frame is diagnostic, not a measure of responsiveness. CPU/MiB here
includes the complete Session-plus-peer workload; it does not isolate 9P codec
or transport cost. Keep the existing +10% gate until explicitly revised, but
attribute CPU stacks and allocation owners before using it to choose another
optimization. Frame deadlines, input-to-presentation latency, idle CPU and
bounded memory remain distinct product outcomes. No matched rio-versus-IPC
benchmark has been run here. Rio's separate draw path makes an unadjusted
comparison with Sophia's upload/admission/presentation workload misleading.

Inspected pinned upstream source and corresponding Sophia paths at `01fca2595`;
source hashes are retained. The follow-up changes completed-read capacity and
adds regression assertions. The full production and comparator 9P suites and strict all-target
Clippy pass. Both copy fixtures pass, and strict Clippy passes for the new upload
fixture. No native benchmark win, hardware run or task completion
follows from this experiment. Plan9port and 9legacy are catalogued;
their performance paths have not yet been audited here. Task state stays in
todo.md; this is supporting research for t331, not another execution queue.

## Connections

- [t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md): accounting, bounds and acceptance criteria.
- [Cost-10 results](lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md#t331-readiness-handoff-pilot-results): mixed results motivating this comparison.
- [Independent uploads](gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md): separately scoped concurrency investigation.

### Extra-pass attribution and approved fixes, October 16

Source inspection explains the steady reply totals without treating every
Session pass as redundant. The panel frame uses five transactions at five
requests each, three slot-setup/cleanup requests, three pixel writes, and
11 event reads plus 11 acknowledgements: 53 replies. Bulk uses eight
transactions, two slots, 72 pixel writes and 17 read/ack pairs: 152 replies.
This is a source-derived accounting, not a per-request trace of every pass.

Launch relays amplify the cost of those passes. Previously each relay did
flush, nonblocking turn and flush on every LaunchBook poll. Files has two
such relays and the comparator's IPC path has one. Typical progress-12
intervals have 33 owner passes and 138 flushes for panel, or 120 and 489 for
bulk. Subtracting four relay flushes per files pass leaves six and nine;
IPC's typical 8/16 and 45/90 pairs fit two per pass. Aggregate core counters
cover all servers and cannot be called shell-only traffic.

The owner approved two separate fixes. Production `d817e6d8a` retains every
custody/retirement check but uses observed readiness and explicit relay
expiry instead of idle transport work. SDK `3edcd28` queues fresh writer walk
and open together, then waits for both successful replies. The shell export
executes those synchronous requests in connection order; the generic 9P API
does not gain this guarantee. Capacity fallback, failed/partial setup,
reordered replies and terminal-slot cleanup have deterministic coverage.
No writes are made concurrent, and no upload fid is cached.

Comparator `f5d0f0932` adds eight diagnostic counters: relay dispatch, idle
skip and expiry, plus walk, open, read, write and clunk requests. They use the
existing relaxed-atomic-only path. Schema 2 records remain bounded below
Linux PIPE_BUF; integration `fd2cb6c` validates them and retains schema-1 replay.
Progress mode still cannot supply CPU or allocation evidence. The relay-only
pilot keeps the old SDK so its effects can be measured separately. Native
benefit remains unproved, and the unchanged full gate still decides t331.

Four one-minute plans are frozen in `t331-native-relay-13`, with exact source
revisions and artifact hashes in `campaign.json`. All four pass preparation
and device-free argument validation, and their capture destinations are empty.
The initial validation omitted the native-scanout environment opt-in and was
refused before runtime setup; that refusal is preserved alongside the passing
checks. The installed `t252-check` selects the new plans. No native arm has run.
Claude's small push finished at 14:55:18Z, outside any new measurement window;
a fresh hold must precede the attended pilot.

Repository-wide formatting still reports the two pre-existing native-scanout
files; changed files are formatted. The default-feature workspace test command
stops at the existing `output_snapshot_allocations` test importing a native-only
module without a feature guard. The affected suites use `native-session` and an
empty configuration root. Neither limitation changes the native acceptance gate.
The full workspace run on production `70685cac7` passes with
`--features sophia-session/native-session`, `XDG_CONFIG_HOME` set to an empty
root and eight build/test workers. Its complete log, the default-feature
refusal and the formatting report are retained in `t331-native-relay-13`.

### Relay-only native results, October 16

All four relay-13 arms completed on comparator `f5d0f0932`, integration
`fd2cb6c`, with the previous SDK `d8558f4`. Independent offline replay matches
journal metrics, all 1,000 kernel/frame joins per arm and every raw progress
record. Console and keyboard recovery are clean. Evidence and hashes are in
`t252-authenticated-20261015/t331-native-relay-13/offline-replay/summary.json`.
Bounds bracket the first upload offer through the final terminal event;
counts below are per offered frame and include every core server in Session.

| Counter | Files panel, relay-13 | Files bulk, relay-13 | Previous panel / bulk |
| --- | --- | --- | --- |
| Session passes | 33.366–33.440 | 120.176–120.332 | 33.082–33.155 / 120.069–120.116 |
| Core polling turns | 5.134–5.167 | 5.141–5.170 | 71.287–71.464 / 245.272–245.389 |
| Core flushes | 5.993–6.001 | 8.990–9.001 | 138.321–138.621 / 489.275–489.465 |
| Encoded replies | 52.938–53.035 | 151.839–152.035 | 52.938–53.031 / 151.965–152.031 |

Relay dispatch and idle-skip counts match within two boundary calls on each
files arm: about 66.7 panel and 240.5 bulk per frame, with zero relay expiry
work. The shared fix also removes IPC's steady core flushes: both IPC arms
record zero. IPC Session passes are 8.028–8.814 panel and 44.991–45.071 bulk.
The wide panel upper bound includes work across sampling boundaries; it does
not establish a steady pass regression.

Request kinds independently match the source accounting: files panel has
about six walks, six opens, eleven reads, twenty-four writes and six clunks
per frame. Bulk has ten, ten, seventeen, 105 and ten respectively. This
confirms that reply traffic remains while idle relay service disappears.
The remaining Session-pass gap is expected: this pilot deliberately omitted
SDK batching. Progress instrumentation cannot establish CPU/MiB or allocation
savings, and t331 remains open.

The quiet hold is released. Claude reports no heavy work, packing, transfers
or SSH during the window; it made brief local Markdown, zk and Git inspection
calls without exact timestamps. That report is retained in `quiet-window.txt`.
Next, comparator `bf19952dd` carries the already-tested SDK pin for a separate
batching pilot. All relay-13 capture directories are occupied and must not be
reused.

### Batched writer native results, October 16

All four batching-14 arms completed on comparator `bf19952dd`, integration
`fd2cb6c` and SDK `3edcd28`. Independent replay matches journal metrics, all
1,000 frame joins and raw progress records in each arm. Console recovery is
clean. Evidence and hashes are in
`t252-authenticated-20261015/t331-native-batching-14/offline-replay/summary.json`.
The bounds use the same upload-to-terminal interval as relay-13.

| Counter per frame | Files panel | Files bulk |
| --- | --- | --- |
| Session passes | 27.426–27.469 | 109.977–110.203 |
| Core polling turns | 5.133–5.162 | 5.137–5.166 |
| Core flushes | 5.996–6.001 | 8.987–9.001 |
| Encoded replies | 52.975–53.035 | 151.740–152.035 |

Against relay-13, batching removes about six panel and ten bulk Session passes
per frame: roughly 18% and 8.5%. Reply counts remain about 53/152, consistent
with combining setup waits rather than dropping protocol operations. Idle
relay skips still cover almost every dispatch; expiry and stale handoff counts
are zero. IPC records 8.018–8.040 panel and 45.020–45.078 bulk passes per frame,
with zero steady core flushes. These are diagnostic counts, not cost results.

Claude reports no crunch activity during the hold from 15:25:26Z to release,
except its local-socket confirmation. The report is retained in
`quiet-window.txt`. All captures are occupied; the hold is released. Next,
measure CPU and allocations separately on this candidate before considering
the unchanged full gate. t331 remains open.

### Cost-15 results and fresh profiling, October 16

All eight separate CPU/allocation arms completed on comparator `bf19952dd`,
integration `fd2cb6c` and SDK `3edcd28`. Offline replay matches stored costs,
journal metrics and all 1,000 frame joins in every arm. Console recovery is
clean. Evidence is in `t252-authenticated-20261015/t331-native-cost-15`, with
raw-log hashes in `offline-replay/summary.json` and bounded comparisons in
`offline-replay/comparison.json`.

| Metric | Files panel | IPC panel | Files bulk | IPC bulk |
| --- | --- | --- | --- | --- |
| CPU ms per uploaded MiB, CPU-only | 10.078–10.085 | 7.695–7.707 | 1.6784–1.6787 | 1.3725–1.3729 |
| Allocations/frame, Rust-global | 2,088–2,090 | 1,140–1,142 | 6,089–6,091 | 3,386–3,388 |

Compared with cost-11, files CPU falls 12.51–12.65% for panel and 6.30–6.33%
for bulk. Allocations fall 9.95–10.15% and 5.95–6.00%. These are comparisons
between pilots, not repeated confidence estimates or whole-desktop savings.
Both relay readiness and SDK batching changed; their individual cost effects
cannot be separated by this pilot. The paired IPC gaps remain 30.77–31.06%
and 22.25–22.31% for CPU, and 82.84–83.33% and 79.72–79.89% for allocations.
The +10% gate fails; t331 remains open and the installed desktop is unchanged.

The hold is released. Claude reports light local source edits, zk/Git reads
and small local copies, but no builds, rendering, tests, heavy jobs, packing,
transfers or SSH during the window. Exact light-work timestamps were not
supplied; retain that limitation rather than claiming a completely idle host.

The owner requested fresh attribution before another optimization. Four
one-minute CPU profiles are prepared in `t331-native-perf-16` with the exact
cost-15 CPU-only artifacts. Sampling uses `cpu-clock:u`, 997 Hz, monotonic
clock and `dwarf,8192` stacks, matching perf-08. All source/artifact checks,
device-free validation, launcher syntax and a small permission smoke pass.
No desktop capture has started; a fresh hold precedes the attended run.
Analysis must select the Session/peer PIDs and the upload workload interval,
compare exclusive leaf periods and resolved callers, and report missing
stacks. User-space samples omit kernel execution and cannot alone explain
all process CPU or allocation counts. Do not use sampled costs for acceptance.

Perf-16 follow-up: only the first files-panel arm ran. Its process capture
completed and all 1,000 journal uploads and cost records replay, but the
presentation join fails because native completions use out-fence retirement
and observation-time fallback. Console recovery is clean. `result.json` says
`captured_only`; it precedes postprocessing and does not prove a joined report.
No `native-samples.json` was written, and the launcher stopped before IPC/bulk.
The timestamp reducer marks out-fence completion as non-kernel without marking
it as a missing page-flip timestamp; that explains the logged combination
`observation_fallback missing_kernel_timestamp=0`. It does not establish why
out-fence completion won throughout this run or implicate sampling as its cause.

The retained partial profile has 1,230 user-space samples in the workload
interval for Session 7616 and peer host PID 7701. The peer reports namespace
PID 1 in counters; perf identifies its unique host process. Selection and raw
hashes are in `t331-native-perf-16/analysis/partial-profile.json`. Without the
matched IPC profile, this cannot attribute the remaining differential cost.
The hold is released; Claude reports only light local Git/text work.

A fresh four-arm attempt is prepared in `t331-native-perf-17` with unchanged
binaries, sampling settings and strict presentation checks. Preparation,
device-free validation, artifact hashes and permission smoke pass. No capture
has started. If the same refusal recurs, investigate completion selection
before another run; do not relax acceptance or overwrite perf-16.

### Perf-17 panel attribution and bulk refusal, October 16

Perf-17 produced a valid files/IPC panel pair on the unchanged cost-15
artifacts. Offline journal/cost replay and all 1,000 presentation joins match
both reports. The bulk files arm completed its uploads and cost replay but
failed the exact kernel timestamp join; the launcher stopped before IPC bulk.
All three captures recovered the console cleanly. Raw evidence is under
`t252-authenticated-20261015/t331-native-perf-17`; selected samples, host PIDs,
intervals, source hashes and full leaf maps are in `analysis/profile-summary.json`.

| Exclusive leaf group, sampled ms across 1,000 panel frames | Files | IPC |
| --- | ---: | ---: |
| Copy | 136.4 | 79.2 |
| Recognizable allocator functions | 52.2 | 31.1 |
| Fill | 6.0 | 9.0 |
| Unknown | 41.1 | 35.1 |
| Other | 951.9 | 570.7 |

Selection uses Session/peer host PIDs 13508/13592 for files and 14421/14505 for
IPC, bounded by the workload counters. The peer counter PID is namespaced;
its unique perf command identifies the host PID. There are 1,184/723 samples,
including 119/148 single-frame stacks. User-space sampling omits kernel CPU,
misses some inlined/unresolved work and includes sampling perturbation. The
instrumented CPU totals are not acceptance measurements.

Largest named exclusive differences include memcpy (+57.2 ms),
`LaunchCustody::is_live` (+47.1 ms), `__vdso_clock_gettime` (+41.1 ms) and
`__internal_syscall_cancel` (+37.1 ms). Custody performs a zero-time poll of
both kernel identities plus active-flag checks; preserve its fail-closed
contract. Inclusive differences put component service at +189.6 ms and the
files transport turn at +139.4 ms. Those stacks overlap and must not be added.
They support investigating repeated service and concrete copy owners rather
than treating allocation count as the entire CPU gap. No cache, skipped
custody check or speculative optimization is authorized by a sample.

Bulk records 436 kernel completions, ending at native frame 437; frame 438
starts observation fallback, which persists for the remaining 567 records.
`frame_retirement.rs` uses a signaled out-fence when no pending callback is
available. `head_completion.rs::synthesize_out_fence_callback` permanently
sets `OutFenceAuthoritative`; `queue_page_flip_callback` then discards later
page-flip events. This explains persistence, but the first race/ordering cause
is still unproved. The sticky mode protects against mixing kernel and synthetic
serial bases, so removing it would not be a safe profiling fix.

The hold is released; mini Claude reports no crunch activity beyond its
confirmation and release message. All three captures are occupied. Before any
new native run, inspect event-pump ordering and reproduce the first transition
deterministically while preserving retirement safety and timestamp accuracy.
The partial bulk profile cannot support a paired differential claim. t331
remains open; no desktop installation or acceptance follows.

### Completion ordering reproduction, October 16

Ordinary `service_native` pumps every DRM card once, then performs retained
image, preview and frame-service work before `retire_ready` polls the fence.
A page-flip event can become readable after that first drain. The head ledger
still looks empty, so a signaled fence selects the irreversible fallback.
Only the hard-stall path currently performs a final collect-and-retire pass.
The same empty-ledger/fence choice exists in singleton and mirror retirement.

A device-free test in `tests/support/native_head_completion.rs` drives the
production native poller and physical-head reducer in both arrival orders.
An empty first read followed by fence synthesis causes the later, timestamped
event to be discarded; collecting that event before fence selection retains
PageFlipPreferred and the exact timestamp. All six head-completion tests pass,
including the existing topology-reset and late-event serial safety checks.
The test controls readiness and models the retirement choice; it does not run
DRM hardware or prove the precise event-arrival instant in perf-16/17.
The first test command used the filename as a filter and selected zero tests;
the corrected `head_completion` filter ran all six.

The narrow repair to evaluate is a card-scoped nonblocking recheck immediately
before the first transition from PageFlipPreferred to fence authority, only
when a signaled fence has no queued event. Route a newly collected event and
its timestamp through the existing ledger; otherwise retain fence fallback.
Do not add another card read to ordinary successful event retirement, reset
sticky authority, fabricate timestamps or wait indefinitely for a lost event.
The recheck must handle shared-card siblings, read failures, draining and
mirror retirement without duplicate intake. An event that becomes readable
only after the recheck can still require fallback, so this is not a guarantee
of kernel timestamps on every frame. Any grace interval or exact submission
identity redesign is a separate decision, not hidden inside this repair.

No runtime fix is implemented by this checkpoint. Existing native artifacts
remain frozen; no further physical run is prepared. First implement and test
the bounded recheck, then evaluate its diagnostic benefit without relaxing
native acceptance.

### Bounded completion recheck implementation, October 16

Signed production `93c542e17` and comparator `075fdfa21` now recheck the affected
card immediately before first fence fallback in singleton and mirror retirement.
The recheck runs only for PageFlipPreferred with no queued callback after a
signaled fence. Events and timestamps use the existing card router; a collected
event wins, otherwise the existing irreversible fallback remains. Heads already
using fence authority and ordinary queued-event retirement add no card reads.
A read/routing failure propagates instead of synthesizing a completion; mirror
service records the error while preserving its existing cleanup path. Drain
retirement uses the same singleton/mirror paths.

The two new checks cover recheck eligibility across event/fence/topology states
and routing both heads of a shared card with their distinct serials/timestamps.
The earlier arrival-window reproduction now calls the production completion
selection method. Existing late-event and topology-reset safety checks remain.
Production backend library tests pass (353, one ignored), DRM-event integration
tests pass (316, one ignored), and strict library Clippy passes. Comparator's
eight head-completion tests pass. These device-free fixtures exercise poller,
head state and selection, not a live DRM card or an end-to-end hardware fault.

This closes the stale first-drain window when the event is already readable at
the recheck. It cannot promise kernel timestamps if the event arrives later or
is lost. The strict native join remains unchanged. No custody checks, limits,
protocols or installed desktop files changed. A fresh four-arm profiling pilot
on the repaired comparator follows; old captures remain retained.

### Perf-18 complete paired profiles, October 16

All four profiles completed on repaired comparator `075fdfa21` and integration
`fd2cb6c`. Offline replay exactly matches journal metrics, costs and all 1,000
presentation joins per arm. Each arm has 1,003 native completion records, all
kernel-timestamped, with clean console/keyboard recovery. This is a successful
pilot on the repaired candidate, not proof that the recheck executed or that
all future event/fence races are eliminated. Evidence and raw hashes are under
`t252-authenticated-20261015/t331-native-perf-18/analysis`; `profile-summary.json`
contains selections and leaf maps, and `callers.json` contains caller grouping.

| Exclusive leaf group, sampled ms across 1,000 frames | Files panel | IPC panel | Files bulk | IPC bulk |
| --- | ---: | ---: | ---: | ---: |
| Copy | 121.4 | 79.2 | 601.8 | 554.7 |
| Recognizable allocator functions | 49.1 | 44.1 | 76.2 | 80.2 |
| Fill | 8.0 | 6.0 | 33.1 | 36.1 |
| Unknown | 41.1 | 44.1 | 38.1 | 41.1 |
| Other | 943.8 | 542.6 | 4,329.0 | 3,432.3 |

The selected files/IPC samples number 1,160/714 for panel and 5,063/4,132 for
bulk. Single-frame stacks number 225/151 and 814/728 respectively. Selection
uses host Session/peer PIDs and the whole upload interval. User-space sampling
omits kernel time, lacks many callers and perturbs execution; sampled periods
are not exact function runtimes. These profiles cannot replace cost acceptance.

The strongest bulk exclusive differences are libc syscall wrappers (+232.7 ms
for `__internal_syscall_cancel`), vDSO clock reads (+130.4 ms), custody liveness
checks (+107.3 ms), Session loop (+54.2 ms) and copies (+47.1 ms). Grouping all
syscall-named leaves by their nearest visible Sophia caller attributes a
+252.8 ms bulk difference to SDK `Pipeline::poll` (panel +38.1 ms). This names
user-space call-path samples, not kernel syscall duration or counts. Custody
callers split between file authentication polling (+55.2 ms bulk) and authority
authorization (+52.2 ms). Both checks retain their existing security contract.

Recognizable allocator leaves differ by only +5.0 ms panel and -4.0 ms bulk in
this sample. That does not make allocation free: inlining, unresolved frames
and cache effects remain outside this grouping. It does make raw allocation
count a weak guide to the largest measured CPU opportunities. Copy callers
remain mostly unresolved; do not assign their difference to a particular
buffer from this profile alone. Inclusive stack totals overlap and are not
additive.

Next investigate the SDK's bounded `Pipeline::poll` read/write cycle. Source
inspection shows each call writes queued output then attempts input reads;
the shell drive may call it repeatedly while draining replies and scheduling
follow-up requests. Count useful bytes/replies against WouldBlock reads and
writes in isolated diagnostics, and examine whether actual readiness can be
retained safely across the wait/drive boundary. Preserve EOF/HUP handling,
partial frames, backpressure, fairness budgets and all upload barriers. Do not
add a readiness syscall before every read or cache custody liveness on the
strength of these samples. A measured redundant-call pattern should precede
an optimization proposal.

The hold is released; mini Claude reports no crunch activity beyond its
coordination messages. All four capture destinations are occupied. No further
attended run is needed to begin the SDK investigation. t331 remains open;
installed desktop and acceptance thresholds are unchanged.

### SDK socket-call investigation, October 16

The isolated SDK upload fixture on `3edcd28` now has retained syscall evidence
in `t252-authenticated-20261015/t331-sdk-io-19`. A temporary test-only marker
surrounded each ShellConnection poll; external strace counted only calls
inside those markers. Handshake and scripted-peer pumping were excluded.
Three optimized runs each covered a normal 128 KiB upload and the existing
short-write/protocol-EAGAIN fixture. All six tests passed. The marker patch,
executable identity, raw traces, summaries and hashes are retained; SDK source
was restored and its worktree is clean. No production counters were added.

| Post-handshake client calls | Normal, all three runs | Short-write/EAGAIN, three runs |
| --- | ---: | ---: |
| Shell poll calls | 18 | 99–102 |
| Successful socket sends | 16 | 23 |
| Socket reads returning bytes | 14 | 21 |
| Socket reads returning WouldBlock | 34 | 124–127 |
| Socket sends returning WouldBlock | 0 | 0 |
| Read bytes | 609 | 686 |
| Sent bytes, including protocol/retries | 131,952 | 349,082 |

Sixteen normal Shell polls contain multiple WouldBlock reads. However, all but
one repeated empty read within a call follow a successful new send. The peer
could have answered that send in a real concurrent deployment; these are not
proven redundant reads. Even an empty read without an intervening send can
observe an asynchronous event, so absence of a send is not a safe skip rule.
The fixture alternates peer and client on one thread after handshake, making
it useful for transition counts but unsuitable for native readiness frequency.
The short-write fixture's EAGAIN is a 9P error reply, not socket backpressure.
Its extra empty polls reflect deadline retries in the busy-driving fixture;
strace perturbs their number. Do not extrapolate these counts to CPU savings.

Source attribution matches the trace: Pipeline::poll flushes queued output,
then reads to WouldBlock within fixed budgets. FileWire::drive drains replies,
queues dependent requests and repeats while state changes. This explains two
empty reads around useful progress without establishing that either is safe
to suppress. A broad readiness cache is not justified by this experiment.

A narrower clock opportunity is explicit in source: FileWire::drive evaluates
Instant::now before every drive_submit_retry call; the retry method immediately
returns when the current submission is not in RetryWait, and also need not
read time for same-pass refusal or event-driven progress. Move the clock read
behind those state checks, retaining the existing deadline comparison when
it actually controls retry eligibility. Existing scripted tests cover retry
after events, deadline expiry and no retry within the refusal pass. This is
the next bounded candidate, not a measured CPU win; no SDK behavior change is
implemented at this investigation checkpoint. Keep custody and socket error
handling unchanged. Broader socket scheduling needs independent readiness/
EOF/backpressure evidence before implementation.

### Conditional retry clock implementation, October 16

Signed SDK `510a434` is pushed on the t331 performance branch and vendored in
production `e7cf62692` and comparator `9c527d566`. The drive loop no longer
reads time before calling retry handling. Retry handling queries Instant only
after finding RetryWait, rejecting the refusal pass and checking for event
progress. The existing deadline comparison, request bytes and custody rules
remain unchanged. No socket read is skipped and no liveness result is cached.

The SDK workspace tests, all-target Clippy and formatting pass. Sophia's signed
vendor gate passes, as do 12 runtime interoperability tests in each checkout,
including independent C protocol and custody checks. Existing scripted cases
exercise deadline retry, event-driven retry and refusal-pass suppression.
Native savings are unmeasured; the change removes a concrete unnecessary clock
query but does not establish its share of the sampled clock cost.

The next four-arm CPU-only pilot, `t331-native-cpu-20`, compares the current
files/IPC panel and bulk workloads without perf sampling or allocation
instrumentation. It includes both the completion recheck and conditional retry
clock since cost-15; a difference from cost-15 cannot isolate the clock change.
This pilot cannot supply allocation evidence or replace the full paired gate.
