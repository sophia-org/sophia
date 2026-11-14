# TRACE-MAP: model actions to the Rust calls that implement them

G3-5d b4 (GATE3-SCOPE.md, G3-5). One table per model, from each action of
its `Next` to the Rust function that carries it, the call site that
reaches it in `src`, the evidence class, and what observes it. The file
is documentation: it maps, it proves nothing. The line numbers come
from a read-only survey at 9b5cf6c5e, checked line by line by a second
read (151 citations, seven corrected below; behaviour claims were not
checked by either read). The cited files other than the combined row are
unchanged between 9b5cf6c5e and the b4 head; the row's own sites are
named by function, because the b4 source changes that file's lines.

The caveat first: every combined chain lives in `src`, and nothing
outside the tests starts it at this head. `turn_combined`
(combined_row.rs, "used once gate 4 wires the row") is called by the
test driver only, and production `ContentNative` carries
`combined: None` (live_session/metadata_shell/content.rs:60-68). The
call-site column is reached through the controlled driver until gate 4.

## Evidence classes

- R: observed by the row trace (tests/support/combined_row_steps.rs
  `Event`, checked by tests/support/component_reconnect/combined_trace.rs)
  in the Session controls; the projection table of the b4 design REV3
  section 2 says which guard each event checks.
- F: observed by the stub fixture's or validator's own counters in the
  accepted Session controls (reserve_attempts, composes, installs,
  ordinary_queues, classifies, accepts, release_attempts,
  release_transfers, checks; ValidatorProgress).
- B: observed only by the accepted backend/runtime unit controls; the
  row trace does not see it.
- E: environment; no Rust call of the row (peer behaviour, clock,
  renderer occupancy, grant replacement as a trigger).
- A: abstracted by the model.
- M: a mutation action; the guard it deletes is named.
- G: no Rust counterpart.

A row of class R is a checked projection only to the extent the REV3
table says; a row of any other class is a documentary correspondence,
and the final package lists it as an open mapped-call obligation.

## FrameJob

| action | Rust | call site | class | observed by |
|---|---|---|---|---|
| Admit | `propose_frame_commit` (sophia-runtime owned_copy/frame.rs:53) → `CombinedHooks::reserve` → `reserve_combined_frame` (W/combined.rs:281), `reserve_combined_release` (W/release.rs:118) → `IsbValidationWorker::reserve_combined` (isb_worker/combined.rs:232) → `CombinedHooks::submitted` | the image owner's hooked service | R | `Bound` |
| WorkerReturn(ok) | the validator loop (isb_worker.rs:166-177): `validate` :362, forward (isb_worker/combined.rs:158); `OutputRoute::serve` (W/combined.rs:506), `deliver` :582 | the worker thread | B | `Collected` sees the owner's take, not the return |
| WorkerReturn(not ok) | `validate` Err returns through the ordinary completion (isb_worker.rs:178-184); the plan comes home on `returned` | `collect_one`'s returned-plan path | R | `Removed(ReturnedPlan)` |
| Replace | the grant epoch replacement | `walk`'s `ended`; `join`'s grant match | E | `Removed(WalkEndedHeld/WalkEndedFailed)`, each carrying the current grant the walk read, checked against the entry's `Bound` grant; `Joined(Ordinary)` beside the old entry |
| RequestWithdrawal / ApplyWithdrawal | `CpuImageJobs::withdraw` (isb_jobs.rs:97), one call | content turn | B | running:345/:495 by the validator's and renderer's own results |
| Submit | `accept_combined_shell_content` (shell_content.rs:401) → `commit_accept` (combined_accept.rs:203) → `accept_combined_held` (W/combined.rs:426) | combined_arm.rs:138 | R (order), F (guard) | `Removed(Accepted)`; `accepts()` |
| Current | `join` (the exact entry) + the admitted deadline (combined_arm.rs:109) + `check_combined_consumption` (shell_content.rs:361) / `combined_consumption_guards` (combined_accept.rs:86) | combined_arm.rs | F | the fixture's checks; the d-0 check observation |
| Discard | `decline`, `walk`, `dispose`, `consumed_failed` (combined_arm.rs:110/:154/:87); `content_renderer_failed_at` (content_candidates.rs:473) | the arm and the walk | R | `Removed`/`Retained` with their causes; a `Dispose` cause carries the current grant, checked equal to the entry's |
| Present | abstract; the nearest Rust is `retire_native_output_on_target` (native.rs:762) and `observe_presentation` (content.rs:404) | native service | A | none in gate 3 |
| EarlyRelease / EarlyRetire | mutations; guards `release_reserved` (isb_worker.rs:293-302), `cancel_combined_release` (W/release.rs:138-145), RetirementProtected (combined_accept.rs:129-130) | — | M | backend unit rows |

## Transfer

| action | Rust | call site | class | observed by |
|---|---|---|---|---|
| Reserve | `reserve_combined` + `reserve_combined_frame`; guards W/release.rs:119-124 (resultRoom, not retired) | `CombinedHooks::reserve` | R | `Bound`; an undone reserve is `ReserveCancelled` |
| Capture | `OwnedRingRead::copy` (owned_ring.rs:243) from isb_worker.rs:379; sourceFinished = the read's Drop (owned_ring.rs:229-231) | the worker thread | B | runtime unit rows |
| Validate | `validate` (isb_worker.rs:362) | the worker thread | B | ValidatorProgress |
| Enqueue | `combined::forward` `try_send` (isb_worker/combined.rs:188); Full/Disconnected return the payload (:194-197) | the worker thread | B | runtime unit rows |
| Take | `serve` `try_recv` (W/combined.rs:518-521) | the renderer worker | B | backend unit rows |
| Render / RenderDone | `combined_render::attempt` (W/combined_render.rs:187, `try_acquire` :194) → `render_frame_in_slot` (W/service.rs:806); the cancel gate W/combined.rs:524-526; NoSlot parks :533-539 | the renderer worker | B | backend unit rows |
| Return | `deliver` (W/combined.rs:582). Divergence: a full result route drops the result and counts it (`undelivered`, :582-584) where the model waits on resultRoom | the renderer worker | B | backend unit rows |
| Consume | `take_combined_frame` (W/combined.rs:382) → `settle_combined_frame` :310 → `settle_combined_token` (isb_worker/combined.rs:377) | `collect_one`/`settle` through `CombinedHooks::collect` (image_owner.rs:195/:204) | R (the owner's take), B (credit and slot effects) | `Collected` is the owner's take before settlement, carrying the reservation's serial and job, the stored grant and the head, checked against `Bound`; validator credit and renderer slot effects are backend/runtime unit evidence |
| Fail | forward Err (:175-179) + `take_completed` (isb_worker.rs:327); renderer Stopped/Expired W/combined.rs:524-528 | the worker threads | B, then R | `Collected(Stopped/Expired)` |
| Request / Withdraw | `CpuImageJobs::withdraw`; the retry of an unconfirmed cancel is the walk's loop | content turn; `walk` | B; R | `CancelLater` (identity-matched to a prior unconfirmed cancel) |
| Die | quarantine (worker.rs:533/625/651/702/712); the result route Disconnected (W/combined.rs:462-468); the row keeps InFlight and reports | `collect_one`'s route error | E; R | `Stepped` never recorded for the kept InFlight; `cleanup_diagnostics`/errors |
| PublishForward | `identity.forwarded.store` (isb_worker/combined.rs:190); the reader `was_forwarded` (:30) has no `src` caller | the worker thread | B | runtime unit rows |
| ValidatorExit | the validator thread's end | — | E | replacement rows |
| RetireNative | `CompletedNativeOwner` (native_owner_retirement.rs:177, built :284); not connected to the row | native service | B | backend unit rows |
| SettleRetired | none. The walk releases or drops on grant end (`walk`'s `ended` arms) | — | G | open obligation |
| Busy / Available / HoldResult / DrainResult | renderer occupancy and the result route's room | — | E | — |

## SourceRelease

| action | Rust | call site | class | observed by |
|---|---|---|---|---|
| Begin | `OwnedRing::offer` (owned_ring.rs:140); `check_offer` :170 is HeldOfferSafe | admission.rs:166 | B | b3d publication rows (journal windows) |
| Validate / StartRender / FinishRender | the Transfer rows above | — | B | — |
| Consume | `collect_one`/`settle` → `receive_forwarded` (isb_jobs.rs:149) → `OwnedCopyOwner::reconcile` (owned_copy.rs:250) | the images step | R, then B | `Collected`; the b3d R4 status window |
| Publish | `owned_release` (release.rs:91) → `publish_owned_release` :121 (EAGAIN :135-137, MD7's guard) | `publish_releases` (owned_copy.rs:276/:313) | B | b3d R4/P3/FJ journal windows; MD7-R4 detected, MD7-FJ full-journal non-detection carried |
| Confirm | `OwnedRing::confirm_release` (owned_ring.rs:203; pending-release check :193-199) | owned_copy.rs:321 | B | b3d SlotReleased windows |
| ObserveRelease / AckRoom / Tick / Timeout | the peer's read and ACK, the clock, the peer's 5 s wait | — | E | b3d-2 FJ/FJ-stop ACK facts |
| Present | abstract | — | A | — |
| ParkExpiry | `serve` `now >= plan.deadline` (W/combined.rs:527); the admission deadline, not a separate bound (divergence) | the renderer worker | B | `Collected(Expired)` after it |

## Fallback

| action | Rust | call site | class | observed by |
|---|---|---|---|---|
| Churn / Replace / Revoke / Mismatch | the scene, the grant, the authority and the plan description changing | — | E | the controls' setups |
| Route | SceneChurn → `Refused::Ordinary` (combined_arm.rs:132) → `decline` :154 → `CombinedEntry::Ordinary` :161; `classify_combined_frame` :119, `classify` (combined_scene.rs:434), `recompose` :401 | the arm | R (the Held join and the decline), F (`classifies`, `installs`) | `Joined(Held)` then `Removed(Decline)` |
| Fast | the accept at combined_arm.rs:133/:138 | the arm | R, F | `Removed(Accepted)`; `accepts()` |
| Refuse | combined_arm.rs:109-112/:117-131/:157-159 | the arm | R, F | `Joined(Held)` then `Removed(Decline)`; the terminal by kind and reason |
| ReleaseAck | abstract; the nearest Rust is `flush_combined_release` Ok setting the receipt (W/release.rs:264-267) → Transferred (:223-225); the physical release is `run_worker`'s Release arm (W/service.rs:519) → `frame_slots.release` (:557) | — | A | the Transferred outcome in `Removed`/`Retained` is the receipt, not the ACK |
| LoseRelease | the release mutant's loss (`Mutation = "release"`), not a normal debt; the guard that keeps a refused Release retained is W/release.rs:249-277 | — | M | backend unit rows (MD1/MD2 Full) |
| Install(ok/deferred) | `install_shell_content` (shell_content_native.rs:184) → `set_shell_component_content_on_target` (shell_content.rs:51) → `install_shell_component_content` :159 (rollback :179-195); the fresh mutation's guard `preflight_shell_component_content` :81 | submission.rs:62-66 | F | `installs()` |
| Prepared | `content_prepared` (content_candidates.rs:276) | submission.rs:71 | F | the peer's records (no Prepared on the lean lane) |
| Retry | the one-way `Verdict::Consumed` written by `join` (MT3's guard) | `join` | R | `Joined` with `verdict_after`; a second match is Invariant |

## OwnerTurn

| action | Rust | call site | class | observed by |
|---|---|---|---|---|
| Consume | `take_combined_frame_result` (combined_frame.rs:296) → discovery/combined.rs:59 → W/combined.rs:382 | `collect_one` | R | `Collected` (result → consumed, lease row), its reservation, grant and head checked against `Bound` |
| Settle | `CombinedRow::settle` / `settle_combined_cell` (W/combined.rs:331; :338 is the token ownership check); one call does Settle, Retain and ClearCredit | `collect_one` | R (the edge), documentary (internal order) | `Stepped(in flight → held/failed/unsettled/credit/undelivered)` |
| Retain | `hold_combined_result` (W/release.rs:153); the Retained case is `give_up_combined_held` :178, reached only through `release_combined_held` (W/combined.rs:390 → :395); the row never calls it directly | `collect_one`; `decline`/`walk` | R | `Stepped(→ held)`; `Removed`/`Retained` with a Release outcome |
| ClearCredit | `settle_combined_token` (W/combined.rs:319; the row's retry in `collect_one`'s Credit arm) | `collect_one` | R (the credit edge) | `Stepped(credit → credit)` is a refused attempt, `credit → held/failed` the success |
| DeclineFast | `Verdict::Consumed` then `Refused::Ordinary` | `join`, the arm | R | `Joined(Held)` then `Removed(Decline)` |
| OrdinaryEntry | submission.rs:52/:62-66 | the ordinary path | F | `installs()`, `ordinary_queues()` |
| FlushRelease | `flush_combined_release` (W/release.rs:249) from `poll()` (worker.rs:579), `combined_release_for` (release.rs:219) and the walk's Debt arm | `walk` | R (Full and its later success), documentary (Disconnected/NotFound/Mismatch) | `Retained(DebtFlush, Full)`; `Removed(DebtFlush, Transferred)` or `Retained(DebtFlush, Transferred, failed(Expired))` |
| TryRender | `submit` (worker.rs:480) / `submit_reserved_capture` :516 (Full → WorkerQueueFull :552) from worker_export.rs:174-183 | the ordinary export | B | backend unit rows |
| DrainNoise / PriorLastUse | the owner's other work | — | E | — |
| ProcessRelease | `run_worker`'s Release arm (W/service.rs:519) → `frame_slots.release` (frame_slots.rs:208) | the renderer worker | B | backend unit rows |
| ProcessRender | the Render arm (W/service.rs:321) → `render_frame` :763 → `try_acquire` (frame_slots.rs:184; the occupied guard :187) | the renderer worker | B | backend unit rows |
| CollectDeferred | worker_export.rs:65-84 | the ordinary export | B | backend unit rows |
| LaterOwnerExport | `pending_frame.take()` + `submit` (worker_export.rs:136/:174-183) | the ordinary export | B | backend unit rows |
| Supersede | `requeue`/`replace_pending_frame` (discovery.rs:469-488) | the ordinary export | B | backend unit rows |

## The seven citation corrections (against the first survey)

`retire_native_output_on_target` is native.rs:762 (:789-790 are calls
inside it); `observe_presentation` is content.rs:404 (:426 is its
`content_presented_at` call); the RetirementProtected guard is
combined_accept.rs:129-130 (:125-127 is HeadRenderingContent);
`check_offer` is owned_ring.rs:170 (:182 is its slot-busy and
stale-generation check); `settle_combined_cell` is W/combined.rs:331
(:338 is `reservation.owns(&result.token)`); the `frame_slots.release`
call in the Release arm is W/service.rs:557 (the function is
frame_slots.rs:208); and the row never calls `give_up_combined_held`:
`decline` and the walk call `release_combined_held` (W/combined.rs:390),
which calls it at :395.

`W/` is crates/sophia-backend-live/src/scanout/rendered_scanout/exporter/
worker/. Unprefixed runtime paths are under crates/sophia-runtime/src/
shell_content/ and crates/sophia-runtime/src/shell_transport/files/export/;
session paths under crates/sophia-session/src/.
