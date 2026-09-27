---
id: c6z62h49
date: 2026-09-27
kind: investigation
status: investigating
tags: [investigation, shell, 9p]
---
# Native launcher SDK audit exposes timing and generation assumptions

## Question

Can the extracted C SDK implement native launcher lifecycle authority using
only its pinned contract? Its author reported thirteen gaps. A read-only trace
through production owners and existing tests found incorrect assumptions about
permit time, generation ownership and acknowledgement order.

## Evidence

The trace uses Sophia `e4f9abcc` (production code unchanged from the `987cfd39`
integration base). Claude's survey reported all thirteen items; the director
cross-checked grant timing, interaction generation, Accept eligibility, close
settlement and the existing file-wire acknowledgement-before-activation test.
This is source/test inspection, not a new physical or latency run.

- `crates/sophia-session/src/live_session/metadata_shell/content/native_service.rs`
  samples elapsed time at line 108, before servicing incoming content, then
  grants demands at lines 141–146 using that sample. Candidate owner
  `crates/sophia-runtime/src/shell_content/candidates.rs` computes its deadline
  from that time and echoes the lifetime without an issue timestamp. Expired
  permit use propagates Stale and terminates the component.
- That Session service supplies `interaction_generation: 1` at line 118.
  Runtime fixtures may supply other values; those values are not production
  protocol authority. Facts and interaction generations are checked again when
  pending native work is taken for renderer submission; a mismatch there
  propagates an owner error rather than producing a Rejected outcome.
- `crates/sophia-runtime/src/shell_transport/native_launcher/control/activation.rs`
  checks that an Accept receipt is unattempted and within its deadline, without
  requiring it to be unacknowledged. `control.rs` retains acknowledged Accept
  receipts. `crates/sophia-runtime/tests/shell_native_launcher_files.rs` already
  submits an ack before activating the same event.
- `crates/sophia-runtime/src/shell_content/candidates/native_close.rs` cancels
  unsubmitted work but retains submitted renderer obligations. Session's
  `native_service/closing.rs` invalidates allocations after old pixels are gone.

## Finding and resolution

Sophia clarification commit `436fb1ac` changes the KDL comments and lifecycle
document, with no field-layout or runtime change. C SDK `3e34a6d` pins those
inputs. The trace also established:

| Area | Current production behavior |
| --- | --- |
| Catalog | One generation-1 publication per grant; an opening cannot adopt a republished generation. |
| Keyboard activation | Ack order and disposition do not themselves determine eligibility; each Accept permits one attempt. |
| Pointer activation | Uses the Action event ID and visible row slot, current focus/revision and the action ledger. |
| Action kind 3 | Cancellation, with no ack obligation. |
| Input deadlines | Host CLOCK_MONOTONIC; overdue input or held Enter closes the opening with reason 6. |
| Close | Immediately disarms focus/input; allocations, submitted candidates and resources settle separately. |
| Admitted activation | Disarms focus, then Session closes the opening; it does not prove application startup. |
| Bounds and IDs | Demand and candidate IDs increase within their owner; allocation request IDs increase; transactions need only be nonzero. |
| Demand reason | Withdrawal (3) takes priority; 1/2 are dirty or animation work and behave alike. |

Neither receipt-plus-TTL nor enqueue-plus-TTL guarantees permit validity at
server ingest. The server sample can precede ingestion, and transport or
scheduling delay has no protocol bound. A client margin is advisory. A client
also cannot eliminate facts-generation races merely by fetching the newest
announcement. These are current behavior limits, not SDK implementation defects.

## Validation and remaining work

The clarification awaits a second trace review and the updated SDK gates.
Native lifecycle implementation proceeds from pinned normative rules, with no
claim that advisory timing prevents revocation. Add production Session coverage
for its fixed interaction generation; current runtime fixtures alone do not
pin it. The C convenience path remains keyboard-only until pointer activation
has its own integration evidence.

The permit timestamp/failure policy and stale-generation renderer handoff belong
with t262's existing role-outcome consistency decision. This audit does not
promote that planning task or silently change server behavior. t263 and t252
remain open in [the task file](../../../todo.md).

## Connections

- [SDK plan](../plans/9cd1ie0x-publish-independent-sophia-client-libraries.md)
  owns extraction and consumer adoption.
- [Shell file contract](../../sophia-shell-files.md) owns the clarified rules.
- [Extraction checkpoint](../milestones/i7pfnyzy-desktop-sdk-extraction-checkpoint-before-application-adoption.md)
  records tested SDK slices and their remaining limits.
