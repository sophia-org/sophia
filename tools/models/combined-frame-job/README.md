# Combined frame job: preliminary ownership projection

This models a PROPOSED contract, not existing code or a Rust refinement. Parent
contract: n7fj3c2p. Category B: owner and worker state plus asynchronous authority
changes. Two sequential frames retain the displayed predecessor; one job at once.

Source mappings (pinned in source-pins.json): Admit corresponds to the future
replacement of propose_frame_commit / reserve_owned_frame_copy. WorkerReturn
would combine OwnedRingRead::copy, IsbValidationWorker validation and render_frame.
Submit combines the proposed owner result checks and an ABSTRACT submission gate.
Present abstracts exact completion AND last-use proof; it does not establish them.
Replacement/withdrawal stand for events serviced by image_owner::service and the
owner lifecycle, not worker-side authority. EarlyRelease/EarlyRetire/guard bypass
are intentionally incorrect mutations, not findings in existing Rust.

## Brief / scenarios and enabled coverage

- S1: revocation or epoch replacement during private work: FreshSubmission,
  targeted by stale.cfg, removing the Current guard on Submit.
- S2: stalled worker plus cancellation: WorkerOwnership and WithdrawalProgress;
  release.cfg releases before worker completion, none.cfg checks weakly fair owner
  withdrawal without worker fairness. Withdrawal is progress, NOT reclamation.
- S3: frame N+1 with predecessor N retained: PredecessorLifetime; retire.cfg retires
  the displayed predecessor before the completion/last-use action.
- TypeOK and SubmittedOwnership are structural checks enabled in every config.

The finite bound is two submitted attempts and one epoch replacement. It explores
failure and success replies, stalling via stuttering, and withdrawal at every
phase. CHECK_DEADLOCK FALSE permits stopped, withdrawn and exhausted scenarios.
No assertion of eventual render or presentation is made for a stalled worker.

## Deliberate omissions: whole-plan gate NOT satisfied

No bytes, placements, resource registry, source generation, budget amounts,
renderer slot Deferred, job misroute, scene generation, deadlines, journal/ACK,
notification ordering, worker restart, GPU last-use, or actual ioctl state machine.
Submit is an abstract authorization point: it does not prove atomicity of a fresh
poll and syscall. Real uncertain return, pre-ioctl ledger and proven detach MUST
be modeled separately before treating the whole design as qualified. Present's
last-use assumption is not validated here. A passing state count is not coverage.

No implementation exists for trace validation; no Trace.tla or fabricated traces
are provided. Prototype instrumentation must eventually emit job/epoch/output,
owner action, capture finish, validation, render, submit outcome, exact completion
and release-proof events. That later trace mapping remains a gate, not completed.

## Run

From this directory, use the pinned local TLC jar, one worker, 512 MiB heap,
outer timeout 30 minutes, writing logs/states outside the source tree:

    timeout 30m java -Xmx512m -cp /home/niltempus/src/Specula/lib/tla2tools.jar tlc2.TLC -workers 1 -metadir /tmp/<unique-state-dir> -config none.cfg FrameJob.tla

none must exit 0 with complete checking and temporal property satisfied. stale,
release and retire must exit 12 naming respectively FreshSubmission,
WorkerOwnership, PredecessorLifetime. All are design controls. No device access,
Rust build, runtime change, CPU or three-service proof follows from these checks.

## Separate-worker transfer projection

Niltempus selected the separate validation worker and direct handoff to the
renderer. TRANSFER.md freezes the proposed ownership/cancellation contract.
Transfer.tla is a separate, uncomposed finite projection of that contract. Run
transfer-none.cfg with Transfer.tla using the same command recipe above; expect
exit 0 and complete checking. transfer-duplicate, transfer-credit and
transfer-cancel must exit 12 with UniqueOwner, CreditUntilConsumed and CancelGate.

The model checks unique envelope ownership, credit retained until owner consumption,
validated private input before rendering, and withdrawal independent of worker
progress. The cancellation check and render start are one abstract action; real
cancellation may race after a check and allow bounded private work. This does not
authorize publication, which remains an owner check outside this projection.
Busy renderer and blocked result route can stall indefinitely. Worker death
quarantines ownership without a reclamation action. Slot availability, queue
reservation, deadline wakeups and scheduler fairness are not proved. Consume
abstracts credit settlement, never a proof that scanout resources can be freed.
Scene-change disposition remains an owner decision. No runtime or prototype gate
is passed by either model, and no CPU/frame or three-service result is claimed.

## Source release / held-peer witness

SourceRelease.tla is a third, uncomposed design projection. source-none.cfg checks
ReleaseProof and HeldOfferSafe; source-early.cfg deliberately confirms before the
finished ticket and journal record and must fail ReleaseProof (exit 12).
source-journal.cfg asks the deliberately false JournalTimeoutAbsent property and
must fail it (exit 12): render/owner consumption and presentation complete while
journal room remains blocked until the peer's five-second WAIT. Review correction:
this schedule omits admission's reservation of four responses, so arbitrary lack
of room is outside the selected contract. No unreserved consumer of that capacity
was identified. Do not present this witness as a reachable held-lane timeout or
runtime risk; it exposes the projection's missing reservation, not a system bug.

The model pins two source slots and two sequential frames, with next offer gated
by observed release and presentation, matching held peer 273d98c2e. It treats the
other peer success conditions (acceptance/predecessor retirement/credit) as already
satisfied; they may only add delay in the real path. No overlapping frame credit
is introduced. Room is a one-bit abstraction of journal/ACK backpressure, not a
model of four reservations, deferred publication holds or actual ACK offsets.
Source release before Presented permits the next offer at Presented, so later
release than validation alone does not imply cadence harm. The real EAGAIN retry
at publish_releases is ordering, not evidence of exhausted reserved ACK capacity.

Finite clock milestones are 0, 16,667 us (rounded 60 Hz period), 100,000 us
(ILLUSTRATIVE parked deadline, not a selected production value), 4,999,999 us and
5,000,000 us (peer WAIT from source). An expired parked job cannot start rendering.
The journal timeout witness completes rendering before the first period, so it
does not depend on that illustrative parked bound. Present is restricted to the first period to isolate source publication delay
in the timeout witness; this is not an all-cadences model. Time can jump between
these milestones; no fairness, measured phase duration, GPU timing or timeout-rate claim
is made. Additional busy/late schedules need the eventual implementation trace.
Run the three source-*.cfg with SourceRelease.tla using the same TLC recipe.

## Approved scene fallback projection

FALLBACK.md records niltempus's selected one-shot conventional fallback. It maps
the current setter rollback, deferred queue success and retained Release path.
Fallback.tla starts at an abstract completed combined result and ends at ordinary
pipeline entry/refusal. It is a separate Category B projection, not a composed
frame pipeline or an implementation refinement. The installed content/claim pair
models only the two fields restored by the setter on Err; no whole-desktop or
GPU rollback claim follows. Current authority/geometry are grouped predicates;
actual bindings, all allocation rows and checks must be tested in the real seam.

Run fallback-none.cfg with Fallback.tla: all seven invariants enabled, expect full
checking exit 0. The six action mutations fallback-eligibility/retry/rollback/
release/prepared/fresh must fail EligibleFallback/OneFallbackEntry/RollbackPair/
LeaseHeld/PreparedAfterInstall/FreshInstall respectively (exit 12). No liveness or
CPU prediction is asserted. Later scene changes do not recursively re-enter the
combined path. Actual ordinary renders, deferrals, failures and owner visits must
be counted; one fallback entry does not mean one guaranteed renderer invocation.

## Forwarding witness amendment

Transfer.tla now splits the successful move from PublishForward(job). Forwarded
flags are indexed by exact job, so a delayed old store never marks its successor.
RetireNative denotes an externally established CompletedNativeOwner, not an EOF
or a timeout; its proof is assumed, not derived here. SettleRetired requires the
forwarded witness for renderer-owned lost results. An exact returned result may
settle before the flag store. ValidatorExit prevents further validation or flag
progress; absent a renderer result or witnessed retirement, credit remains held.
No validator fairness is assumed, and this projection proves no reclaim latency.

All transfer configs now enable ForwardedBeforeRetirementSettlement and
ForwardedOnlyAfterMove in addition to their previous checks. transfer-forwarding
removes the settlement witness guard and must fail the former. Existing controls
remain. The atomic flag is modeled sequentially consistently; real Release/Acquire
and per-output channel teardown need implementation tests. This replaces only the
previous projection's omission of the flag, not its other documented limitations.

## Owner-turn composition and review corrections

OwnerTurn.tla is a narrow composition of returned-result bookkeeping, retained
Release, quota clearance, decline-fast latch and ordinary candidate entry. It
assumes an eligible scene-only fallback; it does not compose the authority,
validator, journal, or ioctl models. A live worker and exact token routing are
assumed. No liveness/fairness or full prototype qualification follows.

Source correspondence (source-pins.json): worker::release_discarded_result and
flush_discarded_release have ONE ordinary retained Option. The proposed combined
cell is separate, one per output facade, reserved at admission; it cannot overwrite
that Option. Retain is the new owner-callable combined disposition operation, not
an existing API. ClearCredit accepts token in cell or successful command transfer;
ProcessRelease is physical worker processing, not an ACK. Disconnected is omitted:
a failed send counts, but supplies neither successful transfer nor physical proof.

The fast path touches no candidate state before its decision. OrdinaryEntry maps
to candidates::begin_submission_inner followed by existing metadata_shell content
service submit_bundle (content.rs / native_service.rs), NOT a new fallback caller.
No second candidate insertion is allowed. Full in flush_discarded_release retains
the token; ordinary WorkerQueueFull/WorkerPending is deferred by
policy_presentation::handle_preview_refusal and projection::queue_retained_projection.
Fallback.tla Install(FALSE,FALSE) instead models a definitive setter error; its
rollback covers two fields only, not pre-error retirement-claim settlement.

OwnerTurn uses a two-command FIFO to expose backpressure (actual channel is 32),
three physical slots, one combined result and at most two ordinary dispatch attempts.
The ordinary discarded cell stays independently occupied. PriorLastUse only models
an externally proven release of the other slots. ProcessRender uses the
frame_slots::try_acquire occupied-slot guard: early enqueue is safe, actual aliasing
is not. It abstracts private rendering completion into acquisition, not GPU last use.

On no free slot, ProcessRender emits a deferred_reply. CollectDeferred is a distinct
owner export cycle that parks pending_frame; LaterOwnerExport is a later cycle
before retry. Counters returns/collections/laterCycles/attempts expose those steps.
They are model-action counts, not native pass or CPU estimates. Supersede abstracts
one newer ordinary frame replacing pending_frame (worker_export::export_from_worker);
its own admission/authority is external. These ordinary retries are NOT the proposed
worker-internal retry of a parked combined job. No dedicated wake is invented.

Run owner-none.cfg: complete checking with all ten safety invariants. owner-drop
must fail LeaseHeld when Full drops the retained token; owner-reuse must fail
NoOccupiedSlotReuse by bypassing try_acquire's occupied guard. Separately run
owner-witness-full/retry: expected failures NoBothFull/NoSupersededRetry establish
reachable Full on both sends and Deferred -> owner collection -> supersession ->
later export -> second dispatch. They are positive reachability witnesses expressed
as false invariants, not bugs. FullIsDeferred/OneEntry/SeparateRetention have no
independent mutations; they check the projection's structure, not Rust behavior.

Transfer Die now covers only queued/waiting/rendering/finished (renderer-held).
Validator-held rows exit through ordinary Fail or ValidatorExit; validator death
can hold quota until component end, including the move-before-flag window. The
frozen proof-01 remains 29,229 generated / 6,192 distinct. The narrowed development
check is 26,661 / 5,616 with CreditUntilConsumed and temporal WithdrawalProgress
passing. This correction is recorded beside the prior counts, not replacing them.
State counts are not coverage or implementation proof.
