# Session shell-work attribution diagnostic

This diagnostic ports only the shell-work attribution commit
`5e6db9f990f0f7b78d790ea1bb1664ff87fd121b` onto
`a56ace89116c02064b93462aa8d41e2f82dd2dab`. The earlier owner-pass diagnostics
are not included. The existing record schema, scopes and counter boundaries
are unchanged; the private tests are mounted from `tests/support`.
Set `SOPHIA_SHELL_WORK_COST=1` in the Session host environment to enable it.
Other values leave it disabled. It changes neither feature defaults nor peer
records, including `gpu_bar_prepare`, and does not change any admission,
freshness, generation, cancellation, retry or release decision.

Disabled hooks still branch and access fixed thread-local state. They perform
zero added clock reads and emit no records. This is not a claim of zero disabled
CPU overhead. Enabled hooks take two thread-CPU samples per measured call and
count the reads, including nested timers. Snapshot generation adds one CPU and
one monotonic sample. Formatting/output and counter work perturb the program;
this build cannot serve as an uninstrumented performance comparison.

## Records and windows

The next external measurement compares instrumented workloads at two image
sizes to locate size-dependent work, not two host implementations. Worker
snapshots delimit completed-job windows; owner snapshots delimit a different
thread's observation windows. Do not subtract their CPU totals or sum nested
stages. A larger worker copy/pixel-validation cost would support investigating
that path; a small worker share would leave owner/backend attribution open.
There is no fused copy/validation implementation or savings claim in this port.

The host-owned output callback emits `sophia_shell_work_cost schema=1` rows.
Each snapshot has a summary, fourteen stage rows, eight boundary rows and four
record-class rows, all
with the same `tid`, `thread_sequence`, `snapshot`, `observed_monotonic_usec` and CPU stamps. Every
row remains below the evidence collector's 4096-byte limit, even with maximal
u64 counters. No payload or per-operation record is emitted.

Owner snapshots use the existing resource-sample cadence and one final scope
exit. Validation workers emit after each 256 completed validation jobs and on
thread exit. A job is counted after validation returns, before delivery to the
completion channel; failures count too. `job_bytes` is requested validation
bytes, not successful copy bytes. Worker rows have `role=validation`; they do
not represent owner-thread CPU. A worker blocked or stuck in validation has no
synthetic final snapshot; absence is not zero work or successful shutdown.
Other Session threads have no output scope in this experiment; their work is
not attributed to the owner or treated as zero CPU.

Use differences of counters from two snapshots with the same role, TID and thread_sequence. The process-local sequence is allocated
once per enabled thread and prevents joining a recycled TID to an older worker.
Rows are inclusive running totals, not intervals. Associate frame counts only
when the snapshot's monotonic interval can be joined to native evidence; never
assign a worker's 256-job interval to an owner's sampled frame interval merely
because they overlap. Existing owner-pass and peer timings retain their own
windows. CPUwin completion-window CPU is a separate measurement.

`clock_regressions` and `saturations` invalidate the entire thread measurement:
consumers must refuse `valid=0`, not clamp or derive a rate. Invalid rows remain
in captured evidence. Counter arithmetic saturates rather than wrapping.
`cpu_first_nsec`/`cpu_now_nsec` describe this thread's diagnostic clock span;
they do not make inclusive stage totals a partition of that span.

## Timed stages

All stage rows have `inclusive=1` and a `parent` relationship. A call contributes
to `calls`, elapsed thread CPU and the maximum. `errors` counts returned errors,
`empty` counts an empty settlement search, and `aborted` counts unwound scopes.
Successful calls are `calls-errors-empty-aborted`. An `Ok(false)` liveness
answer is a completed check, not an errno error. `bytes` counts requested bytes;
`ok_bytes` counts them only for a successful stage. Neither describes physical
memory traffic or bytes the kernel actually copied on a short read.

- `validate`: complete worker validation, including cancellation checks.
- `copy`: immutable owned-ring capture with generation checks before and after.
  `allocate` measures destination reserve/zero initialization; `read` measures
  the one exact payload read and its existing short-read/error check. Both nest
  inside `copy`, which nests inside `validate`.
- `pixels`: the subsequent pixel-validation loop, nested in `validate`.
- `admission`: owner offer-admission sweep, including no-offer/blocked/refusal
  paths. `reserve` nests inside it and includes exact allocation checks, worker
  credit reservation and resource-store admission. Calls are not admitted frames.
- `settle`: owner completion reconciliation through `complete_isb`, including
  empty searches. A settlement can carry a refused resource result.
- `source_release`: source-slot journal handoff, including backpressure/retry
  sweeps. This is not destination resource retirement.
- `retire`: a destination retire request, including immediate collection.
  `collect` also runs independently. `remove` measures final backing removal,
  which can run inside collection or after exact journal confirmation. Their
  parent labels explicitly prevent summing them as disjoint costs.
- `custody`: actual `LaunchCustody::is_live`, including the active-flag fast
  refusal and otherwise its unchanged zero-time pidfd poll.
- `image_alive`: the separate image connector process-alive poll.

## Call and record counts

Boundary rows count actual export-poll wrappers, per-file check wrappers,
`LaunchCustody::is_live` calls and image process checks separately. The tag is
the nearest enclosing operation: pre-turn, server turn, server flush, post-turn,
per-file check, image accept, image check, or other. Tags restore on every return
and unwind. These counts do not imply the checks are redundant, nor do wrapper
counts equal kernel syscall counts. No caching or check removal is introduced.

Service and flush-only turns are distinct, including failed turns. Record totals
per turn count successful typed decodes, not 9P messages, syscalls, candidate
acceptance or presentation. Staged-record decoding starts after envelope and
identity validation; owned-ring controls and ACKs have separate decoder counts.
Writes/refusals count the enclosing decoded-submit, owned-ring-control or ACK
operation, so a decoded but subsequently refused record is not called admitted.
Duplicates may be successfully decoded/written without creating new work.
No-input turns are counted explicitly; no extra transport drain is performed.

## Validation scope

Device-free tests exercise disabled gating, injected clock regression and counter
overflow, nested inclusive totals, error/empty/unwind restoration, bounded worker
cadence, and every emitted field through the strict record parser. The existing
real owned-ring and authentication reactor fixtures additionally pin payload,
short-read refusal, exact source release, and fresh pre/post checks. These are
not native latency, GPU cost, visual correctness or uninstrumented gain evidence.

## Incoming record classes and server publications

Each snapshot also emits four `kind=record_class direction=incoming` rows:
`frame_demand`, `candidate`, `resource_retire`, and `other`. These classify only
staged records after their existing typed decode succeeds. `decoded` counts
those successes; `admitted` counts successful Submitted journal custody for
the same records. Their difference includes retried attempts and failures
between decode and custody; it is not a count of unique refused resources.
The enclosing staged-write/refusal and decode-error counters remain separate.
Owned-ring controls and ACKs retain their own decoder counters and do not enter
these four staged-record classes.

For each class, `service_records` and `flush_records` count successful typed
decodes inside that transport invocation; `service_turns` and `flush_turns`
count invocations containing at least one such decode. These are transport
service calls, not owner passes, syscalls or protocol round trips. A turn may
contain several classes, so the turns-containing counts overlap. Class totals
also retain any decodes outside a service invocation. No per-turn row is
emitted and no causal reduction in turns is inferred from the counters.

The summary's `server_frame_permits` counts outgoing FramePermit records only
after their existing journal append commits successfully. It does not count
peer receipt, ACK, submission or presentation. This hook adds no clock reads;
failed appends do not increment it. The four class rows add four bounded lines
per aggregate snapshot, bringing the total to 27.
