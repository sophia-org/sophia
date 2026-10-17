---
id: nfe0blnb
date: 2026-10-17
kind: plan
status: accepted
tags: [plan, shell, 9p, performance]
---
# Queue shell submit and pixel writes within existing custody bounds

## Scope and exit

Accepted by niltempus on October 17, 2026: B2 queued submit and B3 a bounded
pixel-write window. The prepared first-slice capture and its cost report precede
production implementation. Comparator `9e3c741f8`, integration `4b16974`, their
artifacts and all fourteen launch plans remain frozen through capture. Do not
change the installed desktop.

The exit is matching C/Rust protocol behavior, demonstrated reduction in dependent
waits, and fresh paired CPU/allocation/latency evidence under t331's unchanged
gates. Neither ZX nor a request-count model promises a performance pass. ACK
cadence, pending-read deferral, larger chunks, new limits, independent-resource
concurrency, implicit End and merged retirement are excluded.

On October 17 niltempus approved finishing B3 validation and one matched native
pilot before choosing the next architectural change. Use the measured remaining
costs to decide whether a proposed t304-style descriptor channel for shell content
should take priority. This does not approve that channel's implementation or
relax t331's +10% gates. Any later descriptor-path comparison must include producer
and Sophia CPU, GPU work, frame latency and memory, so moving work outside the
byte-upload measurement cannot count as eliminating it.

The matched B2/B3 pilot is now captured and replayed at comparator `6c87721fa`.
All 12,000 presentation joins and eight cost reports reproduce, but all four
+10% gates still fail. Panel CPU excess is 14.02–14.29% and allocation excess
45.49–46.01%; bulk excess is 15.71–15.76% and 54.06–54.21%, respectively.
See the [result and accounting limits](../investigations/2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md#b2b3-native-pilot--october-17).
Implementation and adoption are complete; the performance exit remains open.
Descriptor design is the recommended next architectural review, not an accepted
implementation or a replacement for the byte-path gate.

## Contract implementation requirements

The [accepted amendment](../../sophia-shell-files.md#queued-shell-writes) owns
the normative rules. Pin that contract in both SDKs during implementation;
retain existing errno precedence. The requirements below guide the changes
and their tests; they do not claim that either SDK implements them yet.

### B2: final record fragment followed by submit

An SDK may queue the final transaction-record Twrite and its submit Twrite in
that order on one shell connection without awaiting the final record reply.
Every earlier fragment must already have a complete successful reply. Both
operations use distinct tags. Setup, negotiation, the covering ACK and the single
live-transaction barrier remain unchanged. This applies to fresh legacy staging
and a reusable writer rearmed after its covering ACK, not to uncertain replay.

The export executes the requests in wire order. Custody still begins only at
the existing submit acceptance point; a record write alone never submits.
Absent or incomplete staging cannot gain custody. Refusal need not be EINVAL:
accepted/replay checks, inbound EAGAIN, missing-staging ESTALE and length checks
retain their existing precedence. The SDK must reconcile both tag results and
any Submitted event before applying submit retry policy.

After a short or refused final record write, stop issuing that transaction,
consume the queued submit refusal and do not retry an incomplete assembly merely
because submit returned EAGAIN. Preserve the existing failed-record behavior.
Unexpected successful submit or matching Submitted after the failed write is
not discarded: preserve authoritative custody, suppress replay and fail the
connection. An indeterminate disconnect remains unknown where custody was not
established; never report a known acceptance as refused or dropped. Matching
Submitted can precede Rwrite(submit). With a successful record write, ordinary
submit EAGAIN/backoff, identical retry and terminal rules continue unchanged.

### B3: sender credits and pixel ordering

Both SDKs use a fixed ring of at most four outstanding pixel Twrites, with an
equivalent sender ceiling `SHELL_FILE_MAX_QUEUED_UPLOAD_WRITES = 4` documented
beside the upload-slot constants. This is a client scheduling bound, not a field
in ContentLimits or a claim that the server can count replies still in transit.
Existing request credits, encoded output bytes and per-dispatch work budgets
can lower it. Reserve control capacity before admitting data; use a smaller
window or the existing sequential path when capacity is insufficient.

In Rust, the ring can gather only adjacent SlotWrite units for the same resource,
live binding, writer fid and grant. Retain every unit's ticket and byte ownership.
C pipelines fragments of its current borrowed buffer, without admitting another
caller buffer until completion. Preserve caller APIs and buffer lifetimes;
record the C struct layout change and rebuild consumers.
Do not pass a record, resource switch, cancellation or End. Fragment requests at
canonical chunk boundaries, negotiated msize overhead and iounit; do not require
a full canonical chunk to fit one transport message. Each later issued offset
assumes the requested preceding bytes were accepted. Match replies by tag,
buffer out-of-order outcomes and retire the contiguous resolved prefix in issue
order. Refill available credits without a batch-wide wait on the normal path.

End and the ordinary closing clunk remain behind successful completion of every
write in that resource. Presented/retirement and frame-to-frame barriers remain.
The ring replaces serialization, not per-ticket custody or output ownership.

### Short writes, backpressure and fencing

A positive short Rwrite advances the live binding only by its reported count.
An EAGAIN refusal of a completing chunk changes no bytes. Already queued writes
whose planned offsets now lie beyond that cursor refuse EINVAL without changing
bytes while that binding is live. Once expiry, cancellation, rejection, End or
revocation fences it, ESTALE takes precedence. This is a consequence of cursor
checks, not a new guarantee that every later request has the same errno.

The current export bounds its inbound queue at `INBOUND_RECORDS = 64` and drains
it through owner service. Four completing writes alone cannot fill an empty
queue. Count EAGAIN heads, invalidated tails and retries so repeated backpressure
is visible rather than inferred from CPU cost.

On a short result or retryable EAGAIN, stop refilling, drain all outstanding tags
for that window, and mark affected later units internally as invalidated pending
retry. Their EINVAL results must not prematurely settle tickets as Refused.
Resume from the confirmed accepted prefix without duplicating acknowledged
bytes. Apply the existing backoff only to EAGAIN; a positive short write needs
no delay after the tail drains. Count short writes: an aligned fragment should
fit the current chunk, so a short reply needs investigation. Tickets settle
Stored once all their bytes are confirmed, including any necessary resends.
Unexpected success in a tail that cannot be reconciled with the accepted prefix
is a protocol failure, not permission to guess a cursor.

For ambiguous flush or lost-reply recovery, getattr is allowed only on the same
live fid after the outstanding tail has drained or its tags have been settled by
flush. Validate its binding identity and cursor against the retained range.
If the binding is fenced, getattr answers ESTALE and recovery ends. Disconnect
does not permit reconnect-and-replay. A permanent head refusal, terminal resource
event or lost binding ends refill/retry; settle each ticket from its established
byte custody and existing cancellation rules, never mark unconfirmed bytes Stored.
Cancellation must consume/flush owned tags before releasing their storage; late
replies cannot reopen a slot or affect a successor resource.

### Deadlines, compatibility and fairness

Queueing extra writes renews no deadline. Only successful canonical chunks renew
the existing idle deadline. Partial, empty, refused and invalidated writes do
not; transfer and transaction assembly deadlines remain unchanged. Keep existing
inbound/scratch/staging charges and EAGAIN semantics. Do not hold a lock across
client streaming or buffer extra completed chunks to conceal backpressure.

The rule relies on the shell export's synchronous wire-order guarantee, not an
arbitrary 9P server's scheduling. No capability bit, node, transport version or
record format changes. Old clients remain sequential. New clients against older
exports must pass the same wire-order/cursor/errno fixtures before adoption;
if that compatibility fails, revise the design rather than infer support from
transaction-reuse or silently downgrade an authority error.
Name the oldest supported export revision in the fixtures and adoption evidence.

The four-credit ceiling is not derived from server max_pending. A server can
complete requests and release those slots during one turn. Retain byte and work
budgets; test another connection's events and controls under a continuously full
window. No fairness conclusion follows from the credit count alone.

The current server's read budget is one negotiated message per connection turn
(`Connection::read_budget`, called by `Server::read`): 65,536 bytes on this shell
wire. Four sender credits do not multiply that budget. A 4 MiB payload plus its
write headers therefore needs at least 65 input turns even with no waits between
writes. Do not use the earlier 69/4 estimate as a Session-pass prediction. A
different input budget is outside this slice; the matched pilot decides what
the existing window saves within the current bound.

## Task details

1. Finish the first-slice native capture and publish its remaining costs. The
   ADR/amendment is accepted; capture remains the prerequisite. Keep B2 and B3 in
   separate commits and diagnostic comparisons so their effects remain separable.
2. Add export fixtures with queued short/EAGAIN tails, expiry between queued
   writes, malformed pixels, incomplete/absent staging and inbound backpressure
   ahead of submit validation. Expect EINVAL or ESTALE according to binding state;
   no new custody or cursor changes on refused writes.
3. Implement B2 in both SDKs with two correlated operations. Tests withhold the
   final record reply while observing submit, reorder their replies, and cover
   early Submitted, record failure plus EAGAIN/ESTALE/EINVAL submit, unexpected
   acceptance, partial prior fragments, disconnect and immutable submit retry.
4. Implement B3 across adjacent Rust outbox units and the current C borrowed
   buffer with a fixed ring, bounded retained bytes and no per-frame ring
   allocation. Tests hold replies
   to observe four writes, refill one credit at a time, use smaller capacities,
   and keep tickets unresolved across a short head/EINVAL tail until successful
   resend. Cover EAGAIN mid-window, out-of-order replies, partial transport writes,
   canonical boundaries, zero/oversized Rwrite, terminal events, cancellation,
   and successor binding fences. Match C/Rust scenarios independently. Neither
   shell SDK issues Tflush for upload writes: both drain replies before releasing
   storage. SDK upload-flush races are therefore inapplicable; generic 9P flush
   tests remain separate from this window's drain/cancellation checks.
5. Verify actual request lengths/counts and ordered dependency traces before a
   native run. Require unchanged normal-path pixel and record request counts and
   zero resends in the deterministic workload. Count short replies, EAGAIN heads,
   invalidated tails and resends separately in fault cases. Use scripted-peer
   counters for fixtures and export/progress-feature counters for the ordered
   diagnostic; production SDKs expose no new metrics API. With sufficient
   credits, one request per chunk and replies withheld, require
   `min(4, adjacent chunks remaining)` outstanding pixel writes; verify lower
   windows under tighter budgets. For one-fragment records, the Rust peer must
   show the record and submit together without awaiting Rwrite(record): two
   waited request stages become one on reuse, and three become two on legacy,
   counting batched walk/open as one stage and excluding owner outcome waits.
   Prove covering ACK, record and submit can share a flush on Rust reuse. C keeps
   its confirmed-ACK barrier but must remove the record-reply wait before submit;
   document that scheduling difference in API-PARITY. These are dependency tests,
   not Session-pass predictions. Four credits do not promise 69/4 Session passes.
   Existing pass activity totals cannot identify ACK/read ordering; keep ordered
   diagnostic traces separate from CPU/allocation builds. Measure neighbor
   service latency and budgets.
6. Publish signed SDK commits, parity/ownership documentation and old/new export
   checks; adopt exact archives through both vendor gates. Use matched native
   B2, B3 and combined evidence as needed for attribution; run the full t331
   matrix only after passing pilots. Do not close t331 from a scheduling model.

Source audit: Sophia `a3c94bb86` export `write_upload` validates binding before
offset and returns EAGAIN before extending scratch. `submit_decoded` checks
accepted replay and inbound capacity before staging/length. Rust `9d56625`
has one Current::SlotWrite per queued chunk; B3 must span those units. A slot
write's permanent error currently settles Refused and continues the connection;
it is not universally a fatal protocol error. C `30b7c1f` has a single upload
operation to replace. Preserve C's documented confirmed-ACK barrier in this slice.

## Connections

Claude reviewed the draft against the export and both SDKs on October 17 and
found no blockers to recording it as proposed. The review does not establish
implementation behavior or native performance.

- [Accepted ADR](../decisions/s5msyyv2-pipeline-shell-writes-with-bounded-recovery-and-unchanged-custody.md).
- [t331 plan and unchanged gates](mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md).
- [Current shell contract](../../sophia-shell-files.md).
- [Current evidence](../investigations/2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md).
- [Concurrency investigation](../investigations/gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md).
- [Prior-art survey](../investigations/yhsocuvc-what-prior-art-on-9p-transport-cost-says-about-the-shell-wire.md).
- [ZX primary paper](https://github.com/fjballest/docs/blob/master/zx.pdf): inspiration for streaming and bounded flow control, not Sophia performance evidence.
