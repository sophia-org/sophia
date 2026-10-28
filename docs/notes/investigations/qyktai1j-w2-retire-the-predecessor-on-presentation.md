---
id: qyktai1j
date: 2026-10-27
kind: investigation
status: investigating
tags: [investigation, performance, shell-files, protocol]
---
# W2: retire the predecessor on presentation

## Question

Each bar frame ends with a separate `ResourceRetire` record for the previous
frame's resource, after the successor's `Presented` outcome. The client then
blocks until `ResourceReleased` arrives (`niltempus` `tools/gpu-bar/dma/src/main.rs:853-866`,
pin `748c325`). Can the candidate itself carry an explicit, negotiated intent to
retire one named predecessor once the candidate is presented, without
changing when anything is released? This is a design for authority review
only. It has no code and makes no CPU claim. It follows W1's shelving
(`516tpvf5`).

## Today's path, from source

- **The record.** `ContentResourceRetire { grant, resource: {id, generation} }` (`sophia-shell-protocol` `shell/content/records.rs:84`, kind 261). Admission is `service_content_resources` (`sophia-runtime` `shell_transport/content_resources.rs:13-38`): it runs `epochs.collect()`, then the direction and grant checks, then `store.retire`.
- **`ContentResourceStore::retire`** (`shell_content/resources.rs:459-482`):
  - store or grant check, giving Revoked or Stale;
  - the exact `{id, generation}` must be accepted, else Stale;
  - a resource already retiring is Stale;
  - `retiring + bytes > max_retiring_bytes` is Budget.
  - On success it marks the resource retiring on the retire's transaction, moves resident bytes to retiring, and runs `collect`.
- **Credits.** Each transfer reserves two response credits at begin: one for the status and one for the release (`resources.rs:240,254,323,502`). A retire asks for one extra credit only if it could produce an error status (`additional_response_credit`, `resources.rs:101-113`).
- **Release.** `collect` (`resources.rs:484-527`) queues `ResourceReleased` only for a resource that is `retiring && !release_pending && releasable()`. `releasable()` (`:65-71`) requires exclusive custody of the pixels, which means no candidate, backend or native lease clone remains, plus capture eviction when that feature is built. The terminal record is journaled by `drain_file_output`, and only then does `confirm_isb_release` free an ISB backing (`shell_transport.rs:495-555`).
- **Presentation.** The session polls `runtime.shell_content_presentation_epoch` in `observe_presentation` (`sophia-session` `metadata_shell/content.rs:334-398`), then calls `content_presented` (`shell_transport/content_candidates.rs:299-338`). That consumes the outcome's credit, flushes and runs `epochs.collect()`. The store does not record which predecessor was on screen.
- **Not presented.** A candidate that is rejected, superseded, timed out or renderer-failed, or whose store is revoked, drops only its lease clones (`shell_content/candidates.rs:324-349`, `:654-668`, `:800-827`, `:853-896`). No resource is ever retired implicitly.
- **Negotiation.** Hello and Welcome carry a revision range and required capabilities. Unknown required bits are refused, and no optional or advisory field exists (`shell_transport/negotiation_policy.rs`). `ContentCandidateEnd` ends with a reserved `u32` that must be zero (`shell/encoding/content/fields.rs:472-496`). Precedent exists for revision-gated kinds and bodies (`shell_files/codec.rs:268-285`, `shell_files/revision.rs`).

## Proposed design

**Negotiation.** W2 is a new capability bit, valid only at a new revision. The client requires it and the server grants it explicitly, so neither side infers it from a predecessor. Without it, every byte and rule is unchanged.

**Intent.** At that revision, `ContentCandidateEnd` carries an optional `retire_on_present`:
- `retire_transaction`: a valid transaction ID minted by the client, as it mints the retire's transaction today;
- `grant`;
- `resource {id, generation}`.

The reserved `u32` becomes a flags word: bit 0 means the intent follows, and any other bit is still refused. At most one intent is allowed per candidate.

**Admission, at End, before the candidate is accepted.** The intent is checked with the same rules that `retire` checks today, and a failure rejects the candidate with a new rejection reason. It never silently drops the intent. The rules are:
- the grant equals the candidate's;
- the resource is accepted at that exact generation;
- it is not retiring and not already named by another admitted intent;
- the candidate itself does not lease it, so a candidate cannot retire what it draws;
- `retire_transaction` is valid and not in use.

**Budget.** The retiring-bytes budget is reserved at admission (`retiring_reserved`), so presentation can never fail on Budget. The release credit is the one the transfer already reserved, and no error status can follow admission, so no new credit is needed.

**Raised only by `Presented`.** In `content_presented`, the store applies the intent exactly as an admitted `retire` would: it marks the resource retiring on `retire_transaction`, moves its reserved bytes into retiring, and then lets the existing `collect` run. This is a retire *request*. Release still needs `releasable()`, so nothing is freed while a native, backend or image lease remains. `ResourceReleased` keeps today's record, transaction and order.

**Every other outcome discards the intent and returns the reservation.** That covers Rejected, Superseded, Timeout, renderer failure and revocation. The resource stays accepted, and the client still owns it: it can retire it explicitly or use it again. A superseding candidate can carry its own intent.

**Conflicts.**
- An explicit `ResourceRetire` for a resource named by an admitted intent is Stale, as for a resource already retiring, and uses today's extra-credit rule.
- A candidate on another output that tries to lease a resource named by an admitted intent is refused. A W2 client must therefore name only a predecessor that no other output still needs. This is the same client obligation as an explicit retire today.

**Replay and custody.** The intent travels inside the candidate's existing once-only custody. An identical replay (EALREADY) does not admit it twice. Revocation clears intents together with the store (`resources.rs:572-582`).

## What it would save, and what it would not

- Saving: one client record per frame, `ResourceRetire`, with its submit, `Submitted` and covering ACK. These are predictions for the counters, not a measured or general bound on owner wakes.
- Unchanged: when the release happens. Release still waits for lease uniqueness, and the client still has to wait for `ResourceReleased` if it needs the slot back before it renders again. W2 removes a request, not the release latency.

## Open items for root

1. The native-retirement point where scanout and retained-image leases drop is not traced (`sophia-renderer-live` `native_scanout/renderer_images.rs`). A W2 prototype must show that presentation does not run ahead of a lease that the predecessor's native frame still holds. If it did, the request would only wait longer, which is not unsafe, but the proof is owed.
2. Revision placement. Candidate-class records are decoded at r9 with image and r8 without, even under r10 (`export/submission.rs:262`). The new field needs an explicit revision path rather than inheriting that rule.
3. Should a failed intent reject the candidate, as proposed, or accept the candidate and report the intent failure separately? The proposal rejects, so that a client never sees a presented candidate whose retirement silently did not happen.

No edits, builds, tests or native runs follow until root reviews this design.

## Amendment after root review of 0086382a5

Root answered open item 3: reject the whole candidate. This section supersedes the proposal above where they differ. It is still design only: there is no implementation and no native GO. Paths are relative to the crate roots named above. The client is `niltempus` `tools/gpu-bar/dma/src/main.rs` (`BAR`).

### Corrections

1. **Timing.** The claim that W2 leaves release unchanged is withdrawn. The retire request moves from the client's next `ResourceRetire` (after `Presented` reaches it) to the server's own `Presented`. Release can therefore come *earlier*, by one client round trip. That is a behaviour change root must accept, not a no-op.
   - From the moment `End` admits an intent, every new lease of the named resource is denied, on every output, including the candidate's own. Today `lease` (`shell_content/resources.rs:438-456`) checks only revoked, grant, accepted and `retiring`. A W2 store adds "named by an admitted intent" to that list.
2. **Budget: reserve, commit, cancel.** The retiring-bytes and response capacity is reserved at `End` admission, committed once at `Presented`, and cancelled on every other outcome.
   - The reservation is keyed by the owner `(store grant, candidate transaction)`, so it is released exactly once.
   - Supersession goes through `grant_permit` (`shell_content/candidates.rs:327-350`). That path removes the old pending candidate at `:337` and adjusts `response_credits` at `:338`, so it must cancel the old intent's reservation there. Today it does no `collect` and no retire; with W2 that stays true, and the resource stays accepted and resident.
   - Rejection, timeout (`:867`), renderer failure (`:805`) and revocation (`:890`) cancel the same way.
3. **The extra error-response credit is reserved before dequeue.**
   - Today `service_content_resources` only peeks the inbound record, checks capacity, and then takes it (`shell_transport/content_resources.rs:158-174`). That is a capacity check, not a reservation.
   - Today's zero-credit retire also has a hole. When the resource is accepted and not retiring, a Budget or Revoked refusal still sends an unreserved `ResourceStatus`, which can fail as `ContentQueueSaturated` (`content_resources.rs:64-81,143`).
   - W2 does not copy that pattern. The candidate's rejection outcome uses the candidate's own reserved outcome credit. Any further status the intent can produce is reserved before the record is dequeued. When no credit is free, the record stays queued, as `Ok(None)` does today.
4. **Revocation.** Revoking the store drops every admitted intent and its reservation together with the store.
   - A resource whose intent was admitted but never committed is released with today's revoked-store rule, which uses the original transfer transaction (`resources.rs:329`). It never uses an uncommitted `retire_transaction`.
   - A committed intent is an ordinary retire, so it is released on `retire_transaction` (`resources.rs:477,493,503`).
5. **Transaction namespace.**
   - No registry of in-use content transactions exists (`resources.rs:167` checks only `is_valid`), and the bar reuses fixed IDs 5, 6 and 7 every frame (`BAR:735,812,855`).
   - W2 adds no unbounded map. `retire_transaction` is checked against the candidate's own transaction and the admitted intents only. There is at most one intent per candidate and one pending candidate per permit, so that set is bounded by outstanding permits.
   - The bar matches `ResourceReleased` on grant, resource and reason, and discards the transaction (`BAR:246,860-865`). Reuse after release therefore stays legal, as it is today.
6. **Once-only commit at `Presented`, keeping `Presented` before `ResourceReleased`.**
   - Today `content_presented` (`shell_transport/content_candidates.rs:299-338`) does three things in order: it removes the candidate and pushes outcome kind 2 into the candidate store's events (`candidates.rs:763-798`); it flushes those events into the single output FIFO (`:334`); it runs `epochs.collect()` (`:336`).
   - A flush failure returns before `collect`, and the event stays queued for a later flush. A resource flush (`content_resources.rs:26,63`) could then put `ResourceReleased` ahead of `Presented`.
   - So the commit is **not** done in `presented`. The intent travels on the `Presented` event itself. It is applied, marking the resource retiring and moving reserved bytes into retiring, in the same step that transfers that event to the output (`flush_content_candidate_events`, `:397-419`, at `take_event`).
   - `take_event` happens once per event, which makes the commit once-only. A later `collect` can then queue `ResourceReleased` only behind `Presented` in the FIFO, which is drained strictly from the front (`shell_transport.rs:500,538-541`).

### Root's questions

**(1) Lease trace.** A lease is a plain `Arc` clone, and uniqueness is checked at `shell_content/resources/backing.rs:95-104`.

| Holder | Where it drops |
|---|---|
| Candidate | `presented` (`candidates.rs:771`), renderer failure (`:805`), supersession (`:337`), timeout (`:867`), revocation (`:890`) |
| Bundle | The end of `submit_bundle` |
| Session projection | Moved into the frame and then the backend |
| Backend `shell_content` | Its predecessor for the same output and layer drops when the successor is set (`sophia-backend-live` `compositor_graphics/shell_content.rs:147`) |
| Lowered CPU layer | The scanout worker, at the end of the `Mixed` arm of `render_frame_in_slot` (`scanout/rendered_scanout/exporter/worker/service.rs:621-691`), unless the frame is `Deferred` (`:498`) |
| Capture snapshots | Evicted only through `retire_capture_imports` (`shell_content/resources/capture.rs:130-152`) |

Scanout buffers hold composed pixels, not leases.

Nothing announces the last drop. When it happens on the worker thread, release waits for the next `collect` trigger, which in practice is the next bar visit.

For the bar, `P`'s backend clone has already dropped when `S` is admitted, and `P`'s render finished earlier. So the `collect` that follows the W2 commit releases at once. A prototype must still show this for the worker-thread drop (old item 1).

Not traced: `removal.rs`, and whether worker completion wakes the owner. A drop notification is out of scope for W2; it would be a separate design.

**(2) Revisions, decoders and SDK encoders.**
- Candidate bodies are decoded revision-blind at r8 (`sophia-shell-protocol` `shell_files/payload.rs:43-53`, `candidates.rs:133-147`). Only the header revision is 9 or 8 (`export/submission.rs:262-268`).
- The SDK encodes candidates and every single record in the r8 envelope (`sophia-shell-client` `files.rs:521-532,670-680`); only `ImageBegin` is r10.
- Revisions 1 to 10 are taken. The next free revision is 11, which needs `check_revision` widened in both places and `kind_at_revision` extended. The next free shell capability bit is 16.
- A W2 field therefore needs three things:
  - a revision-aware `End` decode, so the flags word means anything only at the new revision;
  - an SDK encoder that emits the new envelope only when the capability is granted;
  - a check that raising the candidate decode revision does not also enable the r9 and r10 gates (image kinds, ResourceStatus reason 13, Submitted `candidate_kind`).
- The revision is chosen only after root reviews this map. "r11, bit 16" is the candidate, not a decision.

**(3) A failed intent rejects the whole candidate.** The candidate and its intent are admitted, or rolled back, together. That covers the candidate's leases, its outcome credit, the intent's reservation and its lease denial.

The saving prediction is unchanged: one client record per frame (`ResourceRetire`, its submit, `Submitted` and covering ACK), as counters to read, not a bound on wakes.
