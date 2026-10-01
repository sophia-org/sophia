# Role fixture sequences for t252 B7/B8

Prepared against `026af08c9`, with layouts from the file KDL and lifecycle
rules through `ae60576f8`; runtime implementation starts from `39433c631`.
The [approved coverage plan](ROLE-COVERAGE.md) fixes the verdict at
**96 names: 54 existing plus the 42 below**. This document adds no passing checks.

## Shared fixture rules

Keep the eight base fixtures and add `bar`, `launcher`, `dock`,
`launcher-permit-revoked` and `dock-permit-revoked`: thirteen isolated exports.
The last two are fatal controls aggregated into their family's candidate-custody
check. Every required subassertion must finish before that named check can pass.
Failure, an absent fixture or an unrecognized phase fails the scenario; it never
reduces the expected count. Additional connections for profile refusal controls
can be added later with an explicit assertion list, without weakening these exits.

Each fixture supplies admission, a distinct connection epoch, bounded content
limits and a deterministic owner clock. Clients learn the epoch only by reading
the complete `api` file. Control messages request owner actions or wait for their
completion; they do not give the oracle encoded records, expected byte strings,
negotiated values or verdicts. The oracle decodes every observed object/event
using its independent KDL validators.

Use output `(2,1)`, allocation `(1,1)`, facts generation 3 and scale generation 5
as fixture facts, with a 64-by-32 allocation inside a 128-by-64 output at scale
1/1 (within the coverage budget). Upload a small resource
through the real file slots before submitting candidates. The ordinary catalog
contains two available entries in slots 1 and 2 and an unavailable slot 3.
Slot 4096 is absent. Use distinct transaction IDs and increasing submission IDs;
each accepted submit must have one matching Submitted before its semantic event.

For every absence assertion:

1. Drain and acknowledge expected events, retaining their exact sequence/offset.
2. Wait for the named owner operation to finish through the control barrier.
3. Queue a tail events read, then a root getattr on the same 9P connection.
4. After the getattr reply, require that the read has not replied; flush it and
   require Rflush before reusing its tag.

Prepared and Presented are separate owner phases. The harness must never present
automatically while a no-focus-before-presented assertion is in progress. Owner
errors revoke the component epoch as Session does. Fatal cases must deliver
ESTALE to an already pending read before clean EOF, with no semantic outcome.
No sleep duration is evidence that an event cannot occur.

## r6: bar / indicators, checks 55–64

Offer revision 6 with bits 0, 7, 9 and 10 (`0x681`), omitting ordinary discrete
input bit 8. The component profile grants those bits plus reservation bit 1
(`0x683`). The API role is `bar`. This fixture exercises the documented ordinary
input-disabled indicator path; it makes no linked-action admission claim.

| ID / check | Request and required observation |
| --- | --- |
| 55 `r6/profile` | Read api, submit Negotiate, observe custody then Negotiated with revision 6, exact grant and matching epoch; read Limits. |
| 56 `r6/indicators` | Publish generation 3 with an active output, one status and two indicator entries, one activatable and one action=0; pin/read object kind 4 and validate all fields. |
| 57 `r6/announcement` | Require ObjectPublished kind 4 naming generation 3 and the opened object's qid. |
| 58 `r6/qid` | Republish changed focus/status bytes at the same generation 3. Require a different qid and a new announcement still naming generation 3. |
| 59 `r6/second-pin` | While holding the first indicators pin, lopen of a second fid must return EBUSY; clunk the unopened fid. |
| 60 `r6/old-pin` | Re-read the original pin after both same-generation and generation-4 republication; bytes and getattr identity stay unchanged. |
| 61 `r6/fresh-generation` | Clunk the old pin, reopen/read the current snapshot and require generation 4 plus the latest announced qid and changed rows. |
| 62 `r6/activation-custody` | Submit IndicatorActivate for the current nonzero-action entry with event 11 while the owner is held; observe exact Submitted before allowing admission. |
| 63 `r6/activation-echo` | Release owner; require IndicatorActivationOutcome echoing transaction, epoch, snapshot generation and event 11, with Accepted/0. |
| 64 `r6/stale-activation` | Replay event 11 under a fresh submission/transaction: custody then Stale/0. Separately submit an older snapshot generation with event 12: custody then Stale/0. Assert neither enters downstream admission. |

## r7: native launcher, checks 65–84

Offer exactly revision 7 and mask `0x9a0` (bits 5, 7, 8, 11), API role
`launcher`. Publish the plain catalog with no identities. Open one launcher at
state revision 1. Successful publication and owner completion are control
barriers, never replacements for observing events through the file export.

| ID / check | Request and required observation |
| --- | --- |
| 65 `r7/profile` | API and Negotiate select revision 7 and exactly `0x9a0`, matching epoch and published Limits. |
| 66 `r7/catalog` | Read announced Catalog; require identities_present=0, expected slots/availability and empty identities, with KDL validation. |
| 67 `r7/opening` | Owner opens launcher; require NativeOpening with current grant/output/catalog and initial state revision 1. |
| 68 `r7/allocation` | Submit NativeAllocationRequest tied to opening; custody precedes a correlated granted AllocationResult with parentless geometry and zero reservation extent. |
| 69 `r7/permit` | Upload resource, submit FrameDemand and require custody then a granted permit for the exact output/allocation. |
| 70 `r7/candidate-custody` | Run the candidate controls below, including the isolated fatal fixture. Then submit a valid whole NativeCandidate naming the current opening/catalog/state, selected slot 1, and a fresh permit; observe custody while owner service is held. |
| 71 `r7/prepared` | Release candidate service and preparation only; require CandidateOutcome Prepared for that candidate and no earlier semantic outcome. |
| 72 `r7/no-focus-before-presented` | At the prepared-only barrier, the events tail stays pending. No NativeFocus may be minted before presentation. |
| 73 `r7/presented` | Release presentation; require Presented after Prepared with nonzero presentation epoch. |
| 74 `r7/focus-binding` | Require NativeFocus matching the grant/opening/output/allocation/catalog/candidate/presentation/interaction/state, with a nonzero focus lease. |
| 75 `r7/text-input` | Owner issues semantic Text input; require the current binding, nonempty exact text, increasing event ID and state_revision greater than binding revision. |
| 76 `r7/input-ack` | Submit matching NativeInputAck, observe custody, wait for owner receipt retirement and prove absence of a semantic reply. |
| 77 `r7/query-disarms` | Submit pointer-cause NativeActivate against the old focus binding and its old revision; require Stale/1 because the query edit advanced current state. This avoids conflating the test with a missing Accept receipt. |
| 78 `r7/repaint-focus` | Demand another permit and submit a new candidate at the edited state revision. Prepare/present it, observe the old lease's revocation if superseded, and require a new matching focus lease. |
| 79 `r7/accept-input` | Owner issues Accept kind 17 with empty text; require state_revision equal to the new binding revision and an outstanding receipt. |
| 80 `r7/activation-custody` | Before acknowledging Accept, submit keyboard NativeActivate naming that event and the presented selected slot. Hold admission until matching Submitted is observed. |
| 81 `r7/activation-outcome` | Release admission; require exact NativeActivationOutcome echo, Admitted/0, and one launch-queue admission. Then acknowledge Accept. No application is started. |
| 82 `r7/stale-input-ack` | Resubmit the already retired Text receipt under a fresh transaction/submission; observe custody, owner consumption and no semantic event or new admission. |
| 83 `r7/focus-revoked` | Close the opening with an explicit reason; require NativeFocusRevoked for the current lease with that same reason. |
| 84 `r7/closed` | Require NativeClosed naming the same opening/grant/reason after revocation; the owner has no live focus for it. |

Accept ordering follows the current contract: activation requires an outstanding
unacknowledged Accept receipt. Journal ack and NativeInputAck are different
operations. A journal ack can release event retention without retiring the
semantic receipt; NativeInputAck must follow activation in this fixture.

## r8: persistent catalog / dock, checks 85–96

Offer exactly revision 8 and mask `0x11a2` (bits 1, 5, 7, 8, 12), API role
`dock`. Publish identities such as `registered:term` and `desktop:editor`.

| ID / check | Request and required observation |
| --- | --- |
| 85 `r8/profile` | API and Negotiate select revision 8 and exactly `0x11a2`, matching epoch and published Limits. |
| 86 `r8/catalog-identities` | Pin Catalog generation 3; require identities_present=1 and every row's expected nonempty identity, label and unique slot. |
| 87 `r8/catalog-old-pin` | Republish generation 4 with changed text; the old pin's bytes/qid remain unchanged, and a second concurrent pin returns EBUSY. |
| 88 `r8/catalog-fresh-generation` | Clunk/reopen and require generation 4, changed content and a fresh qid matching ObjectPublished. Publish/read a maximum-row catalog at generation 5 to exercise reads well beyond 1 KiB, then publish/read the original compact slot set at generation 6. Later candidate/activation requests use generation 6, where slot 4096 is again absent. |
| 89 `r8/allocation` | Submit panel AllocationRequest; observe custody and correlated granted geometry/reservation within Limits. |
| 90 `r8/permit` | Upload resource, request a frame and require a matching granted permit. |
| 91 `r8/candidate-custody` | Run candidate controls below and the isolated fatal fixture; then submit a valid CatalogCandidate using current generation and observe exact custody before owner service. |
| 92 `r8/presented` | Release owner preparation and presentation; require Prepared then Presented, with the candidate/output/presentation identities preserved. |
| 93 `r8/activation-custody` | Have the real action owner issue Action for a Presented target; submit CatalogActivate wrapping that exact action and current catalog generation. Observe custody while activation admission is held. |
| 94 `r8/activation-echo` | Release owner; require CatalogActivationOutcome with exact wrapped action/catalog echo and Admitted/0, plus one queue admission. Settle its ledger receipt. |
| 95 `r8/stale-generation` | Obtain a fresh issued action and change only the activation's catalog generation to 3; require custody then Stale/0, without queue admission. |
| 96 `r8/stale-slot` | Obtain a fresh issued action and alter only action_id to absent slot 4096; exact ledger matching fails first, so require Stale/0, without queue admission. This does not claim the later absent-slot Unauthorized/0 path was reached. |

## Candidate controls within checks 70 and 91

Hold candidate-owner service while testing submit errors. For NativeCandidate,
construct byte-valid records with zero surfaces, zero placements, mismatched
target/row counts, duplicate displayed slots and a selection outside its rows.
For CatalogCandidate, use an otherwise valid surface with role 3. Each must
return EINVAL and leave the journal empty, including no Submitted. Bypass only
the oracle's value validator when deliberately constructing these controls;
the production export remains unchanged.

Then run owner rejections, each with a fresh valid permit and increasing
candidate generation. Wait for custody before allowing owner service:

- Stale catalog generation for both families; stale opening and state revision
  for native: Rejected kind 3, reason 1.
- Missing or unavailable displayed slot/target action: Rejected/reason 3.
- Duplicate target triple; overlapping rectangles with distinct triples; a
  target outside its surface: Rejected/reason 3, each varied independently.
- Catalog zero-surface and zero-placement bodies: custody then Rejected/reason 3.

After each rejection, drain/ack it and prove absence of any later Prepared,
Presented or NativeFocus through the owner-completion barrier. Preserve the
valid fixture state for the subsequent positive candidate.

The separate fatal fixtures use an otherwise valid candidate and a missing
permit. Observe Submitted while service is held, queue a tail read, barrier,
then release owner service. Require that pending read to answer ESTALE, followed
by clean EOF, with no CandidateOutcome. Disconnect/reclaim these fixtures even
when the positive family fixture fails later.

## C session path and harness work after handoff

Add role-aware initialization while retaining the base initializer as a bar
wrapper. Validate the API role and the exact r7/r8 offers/grants. Catalog reads
need caller-owned storage up to 4 MiB, indicators up to 32 KiB; retain the small
inline base buffer. Reject insufficient capacity before copying, respect msize
and iounit, and keep borrowed object views valid until the next fetch. No hidden
allocation or frame translation.

The C launcher peer follows checks 65–84's positive sequence and the stale input
ack, query-disarm and byte/value controls. The dock peer follows checks 85–96,
including a maximum-row catalog, stale generation and changed-action rejection.
Both keep an events read outstanding while submitting, consume events before
reusing their storage, ack retention explicitly, and end with quiescent owner
accounting. Use new runtime test/support files and existing public owner APIs;
report missing owner seams to the director instead of editing production Rust.

Extend the Go producer's names and independent Rust verdict parser together.
The final footer requires status=pass, checks=96, failed=0 and successful child
exit. Add parser controls for a missing role check, duplicate role name and a
54-check footer. Preserve bounded child waits, captured output and cleanup;
adjust the current 90-second runtime deadline only with measured justification.
All code gates use private outputs, the caller's priority and parallelism, and
run outside 00:45–04:00.
