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
