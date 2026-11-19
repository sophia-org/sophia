# Stage 3: backend-local retained Release

Status: source proposed for signed frozen review before any build. Stages 1 and 2
remain pinned at 2269c4d33 and 9fd03c08c. This checkpoint follows the stage-3 consult:
use a harness-owned per-facade cell rather than add fields to the 999-line worker.
No crates/*/src item changes, no feature, bridge, runtime API or worker-loop change.
CPU/frame and security remain priorities; this fixture measures neither.

## Placement and actual paths

CombinedRelease exclusively borrows one real NativeGbmRendererWorker facade.
That borrow prevents a second simultaneous sidecar for that same facade; its
Free/Reserved/Retained state permits one outstanding obligation. Reservation
refuses in-flight, stalled, quarantined or detached facades. Reservation itself
sends nothing and releases nothing. The fixture does not implement cancellation
of an unused reservation or the complete combined-job admission lifecycle.

The cell uses the existing bounded core.command_sender, WorkerCommand::Release
and slot tokens. Existing correlated_facade constructs the actual core/facade and
result channel without a GPU; its command receiver is controlled by the test.
One test sends a supplied raw WorkerResult through that result channel and drains
result_receiver directly exactly once. It never passes that result to poll(),
creates no public ScanoutLease with a second Drop release, and adds no second
production result-drain path. Supplied results are not real rendered buffers.

Retain first checks the output key (identity) and Reserved state (capacity), then
Exported (outcome). Foreign output, Deferred, Failed, an unreserved result or a
second result is returned as the original WorkerResult. Deferred still contains
the PendingRenderedFrame owed to the existing owner retry/latest-wins path;
Failed releases nothing. A successful retain stores output, lease id and slot
incarnation from Exported in its own DiscardedWorkerLease, leaving the ordinary
facade.discarded_release untouched. Existing result request/correlation validation
and output-incarnation retirement ownership are caller obligations; this helper
is not a replacement for poll's checks or a combined-result dispatcher.

Flush uses nonblocking try_send. Full and Disconnected leave the exact obligation
in the dedicated cell; neither increments the ordinary release_enqueue_failures
counter. The returned status is the fixture's observation of those attempts.
A successful send frees the cell because the real channel now owns the command,
not because a resource was destroyed. A later empty flush sends nothing. Ordinary
flush_discarded_release retains its own behavior and counters unchanged.

Disconnected has no recovery or retirement proof here: the cell keeps its Release
indefinitely while the harness remains alive, and the test deliberately ends with
it held. Dropping this sidecar at fixture teardown is not a production-safe cleanup
path. It must not be used as a reclamation claim. There is no fabricated detach
receipt, Release ACK or successful quota clearance on Disconnected.

## Slot and composition limits

The occupied-slot test fills the actual three-slot LiveRendererFrameSlotPool,
enqueues Release, receives its exact token, and proves try_acquire still defers at
both points. Only an explicitly supplied pool.release(token) makes that slot
available with a fresh incarnation; the old token becomes stale. That last call
is a pool primitive assertion, not execution of run_worker's GPU lease removal,
buffer drop/recycling or device teardown. Those remain unproved.

The real service's Release arm also checks output lookup, lease id and token before
removing WorkerLeaseBuffer and releasing the slot. This source mapping is not a
fixture execution claim. The cell's raw-result output check is narrower than the
complete incarnation/correlation contract.

In-facade placement and poll-ordered flushing remain a later stage. So do the
runtime credit join, retain-before-settle ordering across crates, validation-success
wake composition, native retirement and fallback. Runtime cfg(test) types remain
inaccessible through the backend dependency; no duplicate allocator, manufactured
completion or default-off bridge substitutes for that scope gate.

## Written tests and post-review gates

Seven tests cover reservation/one enqueue, foreign and non-Exported return,
Full with the ordinary retained obligation coexisting, Disconnected retention,
occupied slot refusal before physical release, second-result preservation and
facade admission guards. They have not run at this source checkpoint.

After source acceptance: targeted backend tests and the existing renderer-worker
and shutdown controls, isolated full repository gate and fmt. Named mutants in a
separate worktree/pool: remove the foreign-output check (foreign return red), drop
the dedicated obligation on Full (held-cell assertion red, later channel delivery
also required), swallow Deferred (owner return red), remove try_acquire's occupied
guard (queued Release is not slot freedom red). Record exact assertions and frozen
identity; restore and rerun targeted tests before the full gate. No native work,
push, production promotion, timing or three-service/CPU claim is authorized.
