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
