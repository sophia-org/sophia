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
