---
id: 7xqjn8rp
date: 2026-09-12
kind: plan
tags: [plan, security, conformance]
---
# Private native input authority and XTEST adapter

Status: **implementation authorized** by the operator. Work starts from
`b8e7aeaa` in separate runtime and conformance worktrees. This authorization
does not include deployment, native-display runs, or enabling live-seat input.

This plan supersedes the ownership, synchronization, shared-seat admission and
version choices in [qoltxfr5](qoltxfr5-concrete-design-for-admitted-synthetic-input.md).
The [principles contract](../decisions/htm85gg0-admission-ingress-and-provenance-for-synthetic-input.md)
still governs provenance, revocation and physical evidence. The earlier
host-user administrator mode is **deferred**; its delegation gate is reopened.
Neither UID matching nor knowledge of a socket pathname authorizes injection.

## Current integration state

The isolated input branch includes published content commit `22c01aa1` through
reconciliation merge `e48571f1`, preserving the reviewed input commit identities.
Reviewed common-authority and constructor/helper code is retained there; the
later M3 producer/consumer candidates remain unintegrated. XTEST discovery stays
disabled. The mandatory native manifest has 40 obligations, including 20 with no
production implementation evidence.

The current runtime focus is terminal completion: control and untracked input
need real completion signals, and accepted work needs owned error and shutdown
continuations through poison, missing targets and output backpressure. Narrow
admission, failure-slot and tracked-input controls are recorded below; they do
not establish the complete lifecycle. Counting a failure is not settlement.
The ordered executor still owes final guarded validation, authority/XKB state
application, remaining producers and the chosen interval/cleanup budgets. These
are already authorized implementation requirements, not new operator decisions.

## Deliverable and limits

Build a protocol-neutral authority with an internal typed API, followed by an
XTEST **2.1** adapter in a contained, headless Sophia instance. Session issues
authority; adapters translate requests. No public native protocol, libei
transport, ambient authorization client, or live host-admin option is added.
The private host constructs no ambient backend and rejects ambient options.
It never tries a portal, bus or live server when local authorization fails.

The instance has one test seat and one test namespace. Its separation from the
operator's seat does not imply confinement of input effects between namespaces
on a shared seat. Modifiers, cursor motion and WM actions remain seat effects.
Explicitly delegated descriptors are valid capabilities; unrelated inherited
descriptors and connections to a different authority are not.

## Milestones and owners

| Milestone | Runtime work, Claude w9:p4 | Independent work, Codex w9:p6 |
| --- | --- | --- |
| M1 | Repair the default Session build by extracting indicator projection from the native module | Retain the fresh 100-execution default-off core baseline and this scope record |
| M2 | Review the common API for executor integration | Own and repair `sophia-input-authority`, its pool and identity regressions, and independent evidence |
| M3 | Guarded synchronization API, private ordered executor, retained completions and phase-aware delivery recovery | Review races, failure attribution, capacity and test mutations |
| M4 | Expose production Session controller and broker integration with deterministic topology/clock adapters | Build the private Session host and containment runner with fabricated endpoint negatives |
| M5 | Implement all four XTEST 2.1 requests, admission and cancellation | Independent clients in both byte orders with absolute deadlines |
| M6 | Affected Rust tests, default build, warnings and workspace checks | All profiles, retained provenance, honest XTS result, tracking and coordinated integration |

Separate targets and worktrees prevent stale include-file and build-cache
comparisons. No merge precedes coordination of runtime, gate and tracking diffs.
Passing one milestone does not enable discovery or close the full task.

## Native identities, state and capacity

The authority is bound to its instance and seat at construction. Session has
an issuer interface; adapters receive opaque keyboard and pointer submission
handles. A registry binds origin, owning connection and grant, generation and
incarnation. `DeviceId` is a packet lookup key, never proof of authority. Only
the issuer registers physical sources. Execution and thaw both validate the
bound grant, generation, security epoch and publication revision.

Reservation creates queued state, not a hold. Validation and application are
one guarded transaction. Authority application begins reconciliation debt even
if no writer has flushed. The ledger implements first press, duplicate press,
joining an existing hold, stale release, survivor-preserving release and
last-holder release. The last release targets the recorded recipient; focus
changes cannot turn cleanup into a new action. Engine retains repeat policy;
retiring its starter cancels repeat rather than transferring it to a survivor.

Retirement marks preallocated debt; it cannot allocate a new vector or fail
because the attempt queue is full. Native reconciliation and recipient
transport are separate obligations. `Flushed` settles transport, not client
processing or native state. Proven recipient termination settles the recipient
obligation. Missing targets and rejected, failed or timed-out routes retain
debt. Completion identity includes recipient connection generation and hold
incarnation. A newer hold for that recipient waits behind the old clearing
barrier. Tombstones remain until both obligations settle.

Construction verifies these bounds against the advertised domain:

| Resource | Bound |
| --- | --- |
| Active plus retiring grants | 16 |
| Virtual devices per grant | 2 |
| Pending request and retained completion per grant | 1 each |
| Synthetic hold/debt records | 16 × (248 keys + 9 buttons) = 4112 |
| Scheduled or in-flight delivery attempts | 64, separate from retained debt |
| Service interval | 16ms; at most 2ms and 32 event starts |
| Cleanup reservation | 0.5ms and 4 event starts within that allowance |

Outstanding incarnations consume capacity. Reserve debt and completion storage
before effects; refuse a new press if it cannot be settled safely. Physical
source records use separate storage. Button mapping permutations do not by
themselves change domain size; incompatible capacity changes must be refused.
Fair cursors persist across stops. These are ceilings, not latency guarantees.

## Synchronization and execution

The selected acquisition rank is outer X runtime, coordinator transition gate,
common authority, surfaces,
pointer state, frozen input, core subscriptions, clients, XFixes subscriptions,
then X input authority. Acquire only needed locks through guarded APIs. Audit
all integration writers; dependency direction alone does not establish order.
Existing sequential epoch locks are not evidence of a current deadlock.

The private executor retains actual xkbcommon state on its worker thread;
there is no substitute modifier arithmetic. One sequence covers keys, pointer,
repeat, state-only changes and cleanup. Modifier projections are read-only.
Delayed work takes its sequence position when runnable. Under the common and
needed X guards, final execution revalidates authority, resolves current
focus/grabs, applies state and records the actual recipient. All participating
publication and revocation writers use that boundary. A transition makes
synthetic routing unavailable until its matching snapshot is published.

Keymap initialization, socket writes, recovery waits and notification routing
stay outside the critical section. The worker never acquires the outer runtime
lock. Completion cells are reserved before acceptance; eventfd is a wakeup,
not storage. Coalesced or failed wake writes cannot discard a committed result.

A watchdog outside the locks observes 250ms from execution dequeue, including
lock acquisition. Deliberate delay, frozen work and budget waiting are excluded.
A wedge before locking, during execution or after commit fails the owned
private instance as `executor_unavailable`. It is not recipient nonresponse.
There is no ambient fallback, replacement replay or live physical-path switch.

## Delivery recovery

Ordinary tickets retain the existing six-second admitted-age policy. Synthetic
tickets track queued, frozen, writer-waiting, socket-blocked and terminal phases.
The actual input send uses `MSG_DONTWAIT | MSG_NOSIGNAL`, without changing a
shared descriptor's flags. Only an `EAGAIN` followed by a `POLLOUT` wait accrues
recipient blockage. Leaving that phase adds its duration; partial writes never
reset accumulated time. Recovery uses six seconds of accumulated blockage.
It does not subtract internal waits from total age or infer nonresponse from
absence of a flush. Queue waits, output-lock waits, control preemption and
intentional delay cannot make a healthy recipient eligible for termination.

## X adapter contract

The client discovers the extension major through QueryExtension. The proposed
major is 146, subject to a collision check; core opcode 88 remains FreeColors.
GetVersion negotiates down to 2.1 when asked for 2.2. Implement GetVersion,
CompareCursor, FakeInput and real server-grab imperviousness through GrabControl
before advertising the extension. Discovery paths share one admission decision.
An implemented but disabled or unauthorized guessed opcode returns BadAccess.

Core FakeInput accepts event types 2–6 and the 36-byte request. The trailing
2.1 padding is not a device selector. XI events are refused. Motion supports
relative and absolute coordinates, protocol clipping and root None as the
pointer's current screen; a missing motion root gets BadWindow and an existing
non-root window gets BadValue.
Window and explicit cursor lookup retain namespace checks for CompareCursor,
including its special None and CurrentCursor cases.

Delay spans the full CARD32 domain using bounded monotonic state. FakeInput has
no success reply. The next request waits until internal processing completes,
not until enqueue or timer expiry, and not until presentation. Healthy peers
continue. Disconnect and revocation cancel pending work. Write-half-close
finishes pending work and drains buffered later requests before actual EOF.
Paused ingress masks POLLIN, latches RDHUP, and listens for cancellation and the
pollable completion notifier without spinning.

## Containment and evidence

Use a tmpfs root with explicit runtime and artifact mounts, fresh temporary,
run and home directories, private network/PID/user namespaces, private proc
and devices, a new session and no operator terminal. Never bind the host root
read-only: read-only mounts still expose pathname sockets. Close inherited
descriptors except owned streams and explicit delegated capabilities. Validate
the supervised inner entry before creating a host or client; `--inside` alone
is not an activation mechanism.

Tests use fabricated endpoints only. They prove an outside-sandbox pathname
socket is reachable in its authorized context and unreachable inside, an
unrelated inherited connected descriptor is closed, and explicit delegation
works. Poison display, authorization, Wayland, DBUS, XDG, Sophia and Hagia
environment values. Mutations must break the corresponding protections, so a
sandbox that accidentally blocks everything cannot produce vacuous success.

Mandatory evidence includes the 100-case core baseline, native authority and
executor tests, real stalled-reader and containment tests, and XTEST wire
cases in both byte orders. Missing, duplicate, foreign, unexecuted, timed-out
or nonpassing mandatory cases fail. Zero matched Rust tests is not a pass.
Retain exact source, host and harness identities and separate failed attempts.

XTS is a separate dependency. Its adapter uses the same containment boundary
and requires explicit test purposes; an absent checkout or TET executable is
BLOCKED and unrun, never suite success. XTS and synthetic fixtures do not
replace physical acceptance for t077, t060 or t062.

Current physical ingress collapses keyboards and pointers to class DeviceIds.
Per-physical-device identity is tracked separately before any future live-seat
mode. Distinct registered fixture devices can establish ledger and emergency
rules headlessly, but cannot prove independent hardware holds. Protected actions
require sufficient physical holds and a physical trigger; synthetic holds must
neither supply a missing component nor veto an independently sufficient chord.

## Baseline and tracking

At `b8e7aeaa`, the fresh core run in
`/tmp/sophia-native-input-evidence/baseline-permitted` passed 100/100 executions
and 20 reporting tests. The initial run in `baseline` failed because the tool
sandbox denied private socket binding; its evidence is retained separately.
Neither run used an operator display. Implementation evidence will be recorded
after the corresponding milestone runs.

M1's projection extraction landed on the runtime branch as `5e0803c8`.
Independent fixture repair `1112c026` removes ambient application/shortcut
discovery from configuration tests. The same native-feature library suite went
from 387 passed, 20 failed and 13 ignored to 407 passed, zero failed and 13
ignored; evidence is `.artifacts/native-input-fixture-target/evidence`.

M2 candidate `5a85cbca` is **not accepted for integration**. Seven independent
external API tests compiled against an immutable archive and failed their
desired-safety assertions. These are implementation gaps under t093, not
changes to the accepted authority contract:

| Gap | Observed failure |
| --- | --- |
| Key/button indexing | Key 255 and button 7 share a record; releasing the key reports a survivor |
| Source capacity | Source 32 wraps onto source 0; retiring synthetic input removes a physical hold |
| Revocation | A retired capability can immediately execute again |
| Issuer identity | A second authority with the same public seat/instance binding produces an issuer accepted by the first |
| Release authorization | A foreign SourceId releases a local hold at the same index |
| Debt retention | A new press overwrites an uncleared release's record |
| Device bound | A grant allocates a third device despite its two-device limit |

Reproduction source is
`/tmp/sophia-native-input-m2-review/probe/tests/authority_safety.rs`, with
`5a85-assertions.log` alongside the probe directory. The run was 0 passed,
7 failed, exit 101. Repairs must make these assertions pass without weakening
them before executor work relies on the crate. Guarded submission must cover
release, retirement and control mutations as well as press; a public packet
key or copyable binding cannot stand in for that authority.

On candidate `55651225`, the original seven external assertions pass. A second
set, retained in `after-55651225/remaining-safety-v2.log`, passes two controls
and fails ten additional desired-safety assertions: revoked-generation device
allocation, unreclaimed grant slots, foreign physical-source identity, clearing
debt for A blocking synthetic or physical B, obsolete publication reopening a
transition, ordinary release during a transition or after an epoch advance,
heap allocation during revocation, and reuse of a caller-created hold identity
allowing stale settlement. Both B-recipient tests finish native reconciliation
first; only A's transport debt remains. These remain M2 blockers under t093.

Harness review also found that the old hidden core `--child SOCKET` entry
accepted an arbitrary socket path. A fabricated listener received its setup
bytes. Both core and XTEST profiles now use the same supervised containment
entry, and direct child paths are refused. The stronger contained core run in
`/tmp/sophia-native-input-evidence/core-contained` passes all 100 executions;
61 harness regressions also pass. No operator endpoint was contacted.

t093 tracks this implementation, t094 the deferred physical-device prerequisite,
and t057 remains the broader protocol conformance task.


### M2 pool replacement

Claude handed the common crate to Codex after candidate `935c3ac8`. The retained
independent run against that candidate was 17/24 PASS: ordinary synthetic
release still bypassed epoch/publication checks, and five new cases exposed
overwritten recipient debt, a lost earlier barrier, grant reuse while ordinary
release debt remained, stale revocation of a replacement grant, and physical
delivery overtaking an old release to the same recipient. Evidence is
`/tmp/sophia-native-input-m2-review/after-935c3ac8/README.md`. The helper
`both_bits` is not a test and is excluded from the roster.

The replacement reserves a pool record before a first press changes state.
That record survives release and retirement until both settlement obligations
finish. All retained incarnations participate in barrier lookup. Exact source
participation references keep grant and device slots occupied through ordinary
release as well as revoke; a joined source conservatively retains its reference
until the shared incarnation settles. Grant identities include authority and
generation, and source identities include their own incarnation. A stale revoke
cannot address a replacement by its reused numeric slot. Ordinary synthetic
release takes the original execution context; issuer cleanup outlives it.

One previous regression asserted that physical delivery could bypass a pending
release to the same recipient. That assertion was unsafe and is replaced:
physical recognition remains separate, but recipient delivery must obey the
clearing barrier. A later receipt cannot undo a stale release already sent on
the wire. Different recipients may proceed once the earlier native obligation
settles. The corresponding test now makes that settlement explicit.

The common crate currently passes 46 tests and warnings-as-errors Clippy.
These include the original seven, the corrected eighteen-test ledger roster,
seventeen independent pool/reuse tests, and four source/reference regressions.
The pool test fills all 4112 synthetic records, proves refusal before mutation,
checks the independent physical reserve, and settles one record to prove reuse.
Logs are `/tmp/sophia-native-input-evidence/m2-pool-rewrite`. This validates the
common state machine, not production receipt producers, queued completion,
executor ordering, or XTEST. Those remain M3–M6 obligations; discovery stays off.

Canonical `cargo xtask check` must run through `offline_check.py`: clearing
opt-in variables alone does not prevent its automatic render-node probes.
Contained metadata validation on `68bc722d` passed with an empty device
namespace. `.artifacts/offline-input-metadata-68bc722d/report.json` records
`full_check_executed: false`; it is not evidence that the full check ran.

Independent review also ran the seventeen pool assertions successfully and used
an external allocator observer against a frozen candidate copy. Applied
synthetic revocation and physical unplug each allocated zero times and retained
the owed release (2/2 PASS). The external probe is necessary because the crate
forbids unsafe code while Rust's allocator observation API requires it; evidence
is `/tmp/sophia-native-input-m2-review/after-pool-rewrite/provenance.json`.
That record names an uncommitted source hash, not a fabricated commit identity.


### M3 common completion and attempt primitives

The common authority now reserves one completion cell per grant before enqueue.
The original request context remains in that cell. Final execution receives a
restricted `ExecutionPermit` under the common guard; it cannot issue or revoke
a grant. The result is stored before unlocking, with eventfd left as wakeup only.
Completed cells pin retiring grant slots until consumed or abandoned by issuer
cleanup. Repeated execution observes the retained result without replay.
Publication changes cancel pending cells. A security-control epoch change also
revokes old grants; issuing a new context cannot revive an old capability.

A callback can fail after applying authority state. Such a result is explicitly
`FailedAfterApplication`, separate from a pre-application refusal; it retains
both the completion and any cleanup debt. Adapter-only effects must mark that
boundary before mutating their state. This result is not proof of recipient
processing, and `Processed` is not an XTEST success reply.

A separate pool reserves at most 64 attempts over the retained debts. There is
one outstanding attempt per hold and a persistent fair cursor. A failed attempt
leaves debt retryable. Even if another receipt settles both obligation bits, an
older attempt that could still send a release pins the record and the same
recipient's clearing barrier until its actual terminal outcome. Stale attempt
identities cannot settle a retry using the same slot. The runtime still owes
actual receipt producers, cancellation acknowledgement, interval budgets and
socket-phase accounting; these primitives do not establish them.

The full native manifest on `e7531ba0` reports 14 PASS obligations and 19
NORESULT, with 36 exact tests executed; it exits 1 as required. Evidence is
`/tmp/sophia-native-input-evidence/native-m2-e7531ba0`. Unimplemented Session,
writer, executor and notification obligations remain mandatory rather than
being replaced with the passing common-state tests.


Grant issuance now binds an explicit `ConnectionIdentity`, distinct from the
delivery `Recipient`. Reservation retains that binding. Final execution and
completion consumption require the currently proven pair; wrong callers cannot
consume a result or change the original cell. Session still owns the actual
admission/currency proof. The common library checks the binding; it does not
infer it from an X packet, UID, socket path or caller-created numeric identity.

Transport attempts become eligible only after synchronous native reconciliation
has completed. They cannot contain deferred native key-ups: a different
recipient may already have pressed that input while the old recipient's
transport remains blocked. M3 integration must maintain that separation.

The common M3 primitives and binding checks pass 72 tests, warnings-as-errors
Clippy and the layout gate. Evidence is
`/tmp/sophia-native-input-evidence/m3-common-bound-connections`. Independent
completion, attempt-retention and connection-binding reviews found no remaining
blocker in these primitives. This does not close the production executor,
notifier, receipt, lock-participant or Session-currency obligations.


### Integration checkpoint after the shell-content foundation

The private branch was rebased onto `deb3dab0` as `6171c351`; earlier evidence
keeps its original commit and archive identities. The fresh native profile at
`/tmp/sophia-native-input-evidence/native-m3-6171c351` reports **17 PASS and
19 NORESULT** across 36 obligations. Its mapped common tests pass, but the
production executor, Session, writer and XTEST obligations remain open under
t093. A preceding restricted-tool invocation could not send on a fabricated
socket pair; its failure log is retained separately from the permitted run.

Independent coordinator review of `2c3e3a5d` found that a superseded
installation report could satisfy a later transition, and that mismatched
coordinator/common starting revisions turned a publication into revocation.
The desired-safety run was one positive PASS and three FAIL, retained in
`/tmp/sophia-coordinator-review-2c3e3a5d/REVIEW.md`. Transition-counter saturation
was also a source finding, not a practical exhaustion reproduction. Claude's
later repairs remain subject to production integration review; sequencing tests
alone do not prove that grab, pointer and frozen populations were cleared.

Common authority now exposes an issuer-checked `published_revision` read for
coordinator construction under the common guard. It refuses during a pending
transition instead of returning the previous publication as usable. The returned
revision is observation, not authorization or a promise of future currency.
Two regressions cover both transition kinds and an identically bound foreign
issuer. This does not replace final execution validation.

The contained canonical run at `2fc6def5` passed workspace tests, Clippy and
layout, then failed the bounded-xterm orphan regression. Tracing proved that
xterm could exit before its command sampled PPID, leaving the command watching
namespace init and retaining the caller's pipe for the full workload. This is
a probe-lifecycle defect, not an input-runtime or hardware observation.
The repair records the outer launcher's PID and starttime before spawning and
checks that identity, including zombie state. A deterministic regression starts
the command only after reparenting. Retained isolated evidence in
`.artifacts/offline-xterm-fix-6171c351` reports original FAIL in 22.142s, fixed
PASS in 2.166s, and restored-PPID mutant FAIL in 22.197s. No full canonical PASS
is inferred from that targeted repair. Render proofs remain NOT_RUN.


The next independent client review found six faulty behaviors that the original
XTEST cases accepted: globally delayed healthy peers, immediate or mid-delay
input effects, half-close bypassing delay, reversed transitions, and denied
pointer input taking effect despite BadAccess. The corrected cases reject all
six in scripted controls; 20 offline harness tests pass. Evidence is
`/tmp/sophia-xtest-case-review/`. The 20 server cases (40 byte-order executions)
have not run against a completed adapter. CARD32 cancellation and slot reuse
still require native lifecycle evidence; a short socket observation cannot
establish them alone.

Master later integrated the wrapper and probe fix as `d0ee0160` and `c89fb1f8`.
The private branch was rebased onto `c89fb1f8` as `8716a523`, dropping those
duplicates and retaining the new content/client foundation. Wrapper and probe
files match master exactly at that checkpoint. The preceding independent core
run at `1c32f3ab` passed 100/100 wire executions and 75 harness regressions;
that report is not validation of the later content/client tranche.

Coordinator repairs from Claude's `f27cefc0` and `c8971472` are now included for
independent testing. Construction derives common published state; transitions
cannot supersede one another, installation names an opaque coordinator-bound
token, and both identity counters refuse exhaustion. The narrow
`m3_coordinator_state` manifest entry names the 24 actual test attributes, not
helper functions. These tests check declared installation sequencing. Production
clearing, final execution, queued/thawed validation and receipt producers remain
open. The [writer investigation](../investigations/0t8n7mwl-client-writers-re-select-the-key-target-at-write-time.md)
records target re-selection at write time, not live reproduction. Its earlier
cross-client modifier-cache finding is withdrawn: the atomic is allocated per
connection, not shared across clients. The genuinely shared XKB state belongs
to the seat-keyed worker. Ordered private execution must preserve that state
without treating each client's notification cache as seat authority.

The canonical run at `1c32f3ab` passed workspace, Clippy, layout and the repaired
orphan regression, then failed an archive-verifier fixture that assumed a
pre-existing release binary. That fixture synthesizes evidence to test the
verifier; it is not a scanout run. The fix selects the actual built debug binary
(or explicit override) consistently. Its remaining private-fixture prerequisites
include parent history and public signature-verification material. No placeholder
binary, disabled signature guard, or inherited private keyring may satisfy them.
The complete run remains FAIL until those checks execute and pass.


The independent coordinator run on `edc4237e` passes 24 tests and
warnings-as-errors Clippy. Evidence is
`/tmp/sophia-native-input-evidence/coordinator-edc4237e`. Its narrow sequencing
obligation is mapped separately from the still-unimplemented production gates.
Review of the subsequent broker scaffolding `9711c376` found that admission was
returned as a boolean before effects, deferred routes lost publication stamps,
and private mode still had bare-counter application paths. Those paths must join
the final common-guard transaction and retain original identity on every thaw;
the slice is not accepted as a complete authorization boundary. Gated construction
must also prevent an earlier ungated sender from surviving as an alternate ingress.

The repaired archive fixture at `553908a6` now passes in a fresh contained
snapshot with its actual parent and minimal public verification keys. Genuine
signature checks for both commits pass; the debug executable is used only as
hash input, never executed. Evidence and public verification recipe are
`.artifacts/offline-archive-fixture-553908a6/verification-recipe.md`.
This is a verifier-fixture result, not scanout or full canonical acceptance.


The wrapper prerequisite repair `4507c741` is integrated on master as
`cce4b8cd`. It requires explicit public verification material for full checks,
retains parent history, and verifies both signatures in a fresh private keyring.
Twenty wrapper regressions and contained signature/metadata preflight pass.
The full contained attempt at `4507c741` passed workspace, Clippy, layout,
bounded-xterm and the repaired archive fixture, then failed because the Hagia
matcher fixture required absent sibling Hagia/Narthex Git repositories. Its
report is `.artifacts/offline-input-full-4507c741/report.json`, full FAIL and
hardware NOT_RUN. It predates master's later content changes and cannot
establish their acceptance. Separate source review found a Narthex archive
identity omission, now tracked as t095 in the
[verification investigation](../investigations/4cs9nf2q-native-archive-reverification-omits-the-narthex-commit.md).

Review of broker `460297d6` confirms that existing senders observe gate
installation, deferred ordinary routes retain publication stamps, and the bare
epoch advancer refuses in private mode. Check-then-act, StateOnly thaw and all
private writer participation remain open. The extra bare-application guard is
currently unreachable behind the blocked counter writer; its mutation is not
behavioral coverage.

The privileged transition candidate `3c399a00` has new source-confirmed blockers:
its co-held X locks reverse the selected rank; state is cleared before token
validation and for publication-only transitions; failed installation discards
revocation receipts; and receipt delivery still occurs within the caller's
control/common guards. Repairs `e2591ed5` and `2aa65b09` address those paths and bind the coordinator
to the common authority lifetime. Review still requires the broker itself to
match that coordinator, and receipt batches to retain their originating sink;
a valid permit/coordinator pair from another private instance must not clear
this broker or deliver its receipts. These are private candidate findings,
not observations on the operator's session.

Common `617651ae` supplies an issuer-checked opaque authority lifetime identity
and an exclusive `ControlPermit` that remains available after revocation and
while publication is unavailable. An identical public seat binding is not the
same authority. Coordinators must retain and check that identity; the permit
keeps exclusive access through adapter-side control without exposing an input
or grant bypass. Seventy-seven common integration tests and one exclusive-borrow
compile-fail doctest pass, with warnings-as-errors Clippy and layout. Evidence:
`/tmp/sophia-native-input-evidence/common-control-permit`. These primitives do
not establish that production callers participate in the guarded transaction.


The native profile at clean `a8a43271` executes 91 exact tests successfully.
Its overall result remains FAIL: 18 obligations PASS and 19 mandatory
production obligations NORESULT. Evidence is
`/tmp/sophia-native-input-evidence/native-a8a43271/report.json`. The result
includes the narrow common/control tests, not production executor acceptance.
The private setup authentication seam `57bb1dba` separately passes seven real
socket tests covering both byte orders and deliberate credential faults. It
produces verified admission evidence only and awaits coordinated integration;
it implements neither grants nor XTEST execution.


Setup seam `57bb1dba` was approved by the runtime owner and integrated as
`161ec805`. An independent rerun of its seven tests passes; evidence is
`/tmp/sophia-native-input-evidence/setup-161ec805/tests.log`. Its manifest entry
is limited to setup authentication. The default-off core baseline at
`28417f90`, including the then-current master content foundation, passes
100/100 wire executions and 90 harness regressions, with all 14 real isolation
tests executed. Evidence is
`/tmp/sophia-native-input-evidence/core-28417f90/report.json`. Neither result
constitutes XTEST execution or hardware acceptance.

The subsequent control slice at `57fdcfc0` passes independent instance-binding,
transition and receipt checks: 676 X-authority tests PASS, one existing Qt probe
is ignored, and the contained core profile passes 100/100 wire executions.
Warnings-as-errors Clippy, layout and 90 harness regressions pass. Evidence is
`.artifacts/native-control-57fdcfc0-evidence/`. Receipt batches now retain their
origin by allocation identity; a wrong broker returns the batch so the origin
can still deliver it. This closes the reviewed control-slice findings, not
production execution admission.

The proposed executor at `8cb09cb9` was rejected and remains unintegrated.
Independent public-API checks pass three positive controls and fail five desired
safety assertions: foreign-authority execution, release after focus disappears,
release after focus changes, duplicate delivery reporting, and delivery reporting
on a refused press. Evidence is
`.artifacts/synthetic-executor-review-8cb09cb9-v2/`. Production `route_pending`
does not call that helper; its check-then-act gap remains open. Revision
`2caba34c` passes all eight retained cases and a direct-common-under-coordinator
control. A separate helper-under-coordinator call times out at its three-second
watchdog: the identity check reacquires the coordinator from a method receiving
mutable common state. A caller holding common would also invert the selected
rank. Evidence is `.artifacts/synthetic-executor-review-2caba34c/`. Repair
`b5d371c4` captures authority identity in the gate for lock-free checks. All nine
retained tests and the previously hanging re-entry case pass independently;
evidence is `.artifacts/synthetic-executor-review-b5d371c4/`. Public mutable access
to the whole coordinator allowed replacement while the gate retained the old
identity. Repair `cbd94859` restricts that access through a facade: all ten helper
cases and its compile-fail regression pass independently. A compiling mutant that
restores mutable dereferencing makes the regression fail at the intended boundary.
Evidence is `.artifacts/synthetic-executor-review-cbd94859/`. The repaired helper
is integrated into the isolated branch only. Committed focus, actual recipient
routing, ordered XKB state application and StateOnly thaw remain required before
production-path acceptance.

The canonical contained check now passes on exact clean `49d38924`, using the
device-hidden wrapper: command exit 0, 282 Rust result groups with 3,311 passes,
zero failures and 29 ignored tests, and no compiler warning lines. Required
Sophia and explicit sibling commit signatures pass. Evidence is
`.artifacts/offline-input-full-49d38924/report.json` and its adjacent
`execution-summary.json`. This snapshot includes master `4c4af8bd` and excludes
later content commit `9cf6650a`; the result does not certify current master.
Hardware proofs and promoted host archives are explicitly NOT_RUN. Passing the
canonical checks does not discharge the native profile's missing production
obligations or make the private XTEST host operational.

Master subsequently passed the exact-source contained canonical check at
`a65edae8`; evidence is `.artifacts/offline-check-a65edae8/report.json`, again
with hardware and promoted archives NOT_RUN. The input branch is reconciled
onto that root, preserving its verifier repairs and closed t095 record. Neither
root's PASS nor the older input snapshot's PASS certifies their newly combined
input changes without checks on the combined source.

The [production operation inventory](../investigations/w8vt0ueb-production-entry-and-ownership-for-ordered-synthetic-input-execution.md)
now includes writer-side focus application, direct SetInputFocus, core
subscriptions and route-relevant window publication, registration-drop/recovery
cleanup, grab rollback and repeat ownership. These are dependencies of the real
private execution path, not additional work deferred past its acceptance. Only
route-relevant state needs a coherent publication boundary; ordinary property and
clipboard operations need not move into the input executor. Unstamped private
ingress must return an explicit producer refusal without mutating state, inventing
a receipt identity, or terminating healthy-client service.

On exact clean reconciled `ed3ffd9a`, the common and X-authority package suites
pass 765 tests with one existing Qt probe ignored. All-targets Clippy with
warnings denied and the layout check pass. Evidence is
`.artifacts/native-input-integration-ed3ffd9a/`. These checks establish the
integrated helper/facade slice, not production executor or canonical acceptance.
Private construction must avoid exposing raw ingress before admission exists.
Late activation with already accepted untracked raw work may explicitly refuse
activation; it cannot retroactively return an error through a completed
`SyncSender::send` or fabricate a receipt for that work.

The initial ready-stream candidate `c6e4475e` failed independent review. Independent tests of its
exact std-only source pass four controls for FIFO, reserve capacity, sequence
accounting and accepted-payload ownership. A desired-safety assertion fails:
capacity refusal destroys the incoming owned payload before returning, so its
owner cannot retry cleanup or report the rejected operation. Evidence is
`.artifacts/ready-stream-review-c6e4475e/`. Refusal must return the payload;
durable cleanup debt must remain represented when even reserved queue space is
full. These are queue-primitive checks, not execution by the five real producers.

Repairs through `4cdcb253` return the refused payload and validate the configured
reserve. Nine independent std-only tests pass, including a private test fixture
that reaches actual admission exhaustion at `u64::MAX` and checks that the payload
and queue state survive. Evidence is `.artifacts/ready-stream-review-4cdcb253/`.
The repaired primitive is integrated at `7aefe013`; its ten in-tree tests pass,
with evidence in `.artifacts/ready-stream-integration-7aefe013/`. The native manifest
maps these as `m3_ready_stream_primitive`, expressly separate from real concurrent
producer and executor obligations. Nineteen production obligations still have no
implementation evidence; this queue result does not close them.

The non-consuming installer through `0c24e888` passes two independent cases:
refusing a different gate preserves the original gate, queued work and continued
delivery; reinstalling the same gate preserves an in-flight transition and its
eventual completion. Extracted control apply/report bodies are unchanged apart
from whitespace. Evidence is `.artifacts/control-install-review-0c24e888/`.
This verifies an already-gated broker's continuity. First activation after raw
ingress exposure, the private constructor and real producer wiring remain open.

The branch was reconciled onto published content commit `d7654d23` at
`eb8e1078`, preserving root's wrapper and closed t095 record without importing
another copy. Root master was not moved. The root's exact contained canonical
PASS remains evidence for `d7654d23`; it does not certify this combined branch.

Raw-ingress repairs through `42766ddb` pass six independent cases on that exact
source, retained in `.artifacts/raw-ingress-review-42766ddb/`. Taking a raw handle
prevents later activation even after every handle is dropped. Refusal preserves
an already queued event and subsequent ordinary operation. An installed gate
refuses raw access without disturbing a pending transition. These repairs are
integrated as `2c662b64` and `30279f29`. The exposure flag covers queued raw work
because the guarded getter is currently the only export of that channel; an
internal producer bypassing the getter would invalidate that argument. The
private constructor and production executor still require independent evidence.

Exact combined source `a71fde82` passes 779 common/X-authority tests, with one
existing ignored probe and no compiler warnings. All-targets Clippy with warnings
denied and the layout-only check also pass. Evidence is
`.artifacts/native-input-integration-a71fde82-sockets/`. The initial restricted
sandbox run failed on owned Unix-socket operations and is retained separately at
`.artifacts/native-input-integration-a71fde82/`. Neither run was a full canonical
gate or a native/hardware test.

Private-constructor candidate `7d92667e` passes two independent public sender
checks and the missing-raw-method compile-fail check. The actual sender accepts
an open-gate enqueue and returns rejected work during a pending transition,
including when the facade is constructed with that transition already pending.
Evidence is `.artifacts/private-constructor-review-7d92667e/`. The constructor is
integrated at `18621044`; its facade currently provides only construction,
stamped ingress and the existing `route_pending`. Client registration, service
attachment and ordered production execution are not established by this slice.
The preceding `a71fde82` package results do not include this later constructor.

Consumer candidate `df1db847` is not integrated. Its admission pass drains
separate lease, input and control queues in a fixed order, so it does not
establish cross-producer ordering. The test named for ordering across sources
submits two items through one sender and does not check their identities. The
runtime owner reports that replacing the pass with the original broker drain
leaves the tests passing.

Source review also finds ownership loss at all three `admit_runnable` refusal
sites: `try_recv` removes an accepted item, then `.is_err()` drops the
`ReadyAdmissionError` together with its returned payload. There is no retry
storage despite the method comment. Queue refusal must return unaccepted work
to its producer; previously accepted work must retain its genuine completion or
cleanup obligation. The candidate's closed-before-run test covers one stale
stamp check, not atomic execution: `gate.admits` still releases its guard before
routing mutates state.

The next implementation boundary is shared admission by the actual producers.
Position assignment and publication must be one action when work is runnable.
Concurrent admissions may take either order, but an accepted operation must not
be overtaken by one whose admission starts later. Delayed and frozen work takes
its position when it becomes runnable. Tests must exercise alternating real
producer facades, controlled concurrency, exact identities and refusal ownership;
class-tagged entries submitted by a consumer cannot establish those properties.
These blockers remain within highest-priority t093, with discovery disabled.

The private producer facade must also separate policy refusal from queue
saturation: the current `XAuthorityRoutedInputSender::try_send` reports a failed
stamp as `Full`. Its existing recovery admission precedes queue acceptance and
is rolled back if enqueue fails. Shared-stream integration must preserve exact
reservation ownership and rollback on refusal, with no accepted-but-unretained
operation and no cancellation of another request's completion.

Follow-up `496bcad8` checks capacity before receiving, but its unexpected-refusal
branch still drops the payload: `_retained` is a local binding destroyed before
`RegistryPoisoned` returns. Sequence exhaustion is independent of the capacity
check. Follow-up `b72c39e7` distinguishes missing-stamp denial from channel
saturation, but still maps every recovery-admission failure to `Saturated`:
that boolean also covers duplicate delivery identifiers and a poisoned recovery
lock. The facade also continues to expose the ordinary sender with its old
error mapping. Neither follow-up is accepted as the completed private producer
boundary. Rollback tests must establish exact reservation ownership, including
successful retry of the refused operation without disturbing accepted work.

Independent execution confirms the reachable `df1db847` loss without changing
queue configuration. With constructor capacity 1, a lease release, routed input
and ConfigureSurface control are all accepted; input delivery 9001 reaches its
channel, but control transaction 8001 disappears and the next drain runs no
work. The same fixture at capacity 2 delivers the control. One positive passes
and the desired conservation assertion fails; evidence is
`.artifacts/private-ready-loss-df1db847/`. The first fixture compile error is
retained separately and is not counted as bug evidence. This is a headless
internal integration reproduction, not a live-session failure.

The unchanged reproduction passes both cases at `496bcad8`: capacity 1 runs two
operations, then delivers transaction 8001 on the next drain; capacity 2 still
delivers all three in the first pass. Evidence is
`.artifacts/private-ready-loss-496bcad8/`. This verifies the reachable capacity
repair only. Unexpected-refusal ownership, actual producer ordering and atomic
execution remain open, so the staging implementation remains unintegrated.

At `c9b3a674`, source review confirms an actual frontend-owned retained slot and
a private ingress wrapper with no ordinary `try_send` method. This repairs the
local-drop structure and facade escape; it does not establish exhaustion
coverage, progress or complete private error classification. No further staging
polish is required before replacing the staging pass with shared producer
admission. The candidate remains unintegrated.

Sequence exhaustion is permanent for a stream. Direct admission must return an
owned exhaustion refusal before acceptance rather than retrying forever or
reusing sequence identities. Previously accepted work must keep its genuine
completion and cleanup obligations through explicit failure and settlement;
retaining a payload alone does not settle it. The native manifest now makes
production ingress refusal classification and exact reservation rollback an
explicit mandatory obligation, separate from the passing queue primitive and
common completion-cell tests.

Candidate `025d59ba` replaces consumer staging with shared admission used directly
by routed-input and control facades. This establishes the intended structural
location for sequence assignment and publication; it does not yet establish
production execution. Source review identifies three remaining boundary defects:
control submission drops the returned command while mapping an admission error;
producer-held `Arc`s keep admission alive after the frontend/consumer is dropped,
with no closed-consumer state; and `take_next` maps a poisoned queue lock to
`None`, making failure look like an empty queue. These require owned refusal,
consumer-close ordering and explicit unavailable reporting respectively.

The new ordering tests compare positions returned to producers and, in one case,
a total number of executed operations. They do not assert the alternating
operation identities seen by the consumer. Admission order and consumption order
need separate assertions; a consumer regrouping already-numbered operations
must fail. Likewise, completed-send precedence and simultaneous producer
contention are distinct cases. Cleanup remains without a producer facade, and
final generation/publication validation plus authority/XKB application remain
unimplemented. The candidate is unintegrated pending these checks and repairs.

Independent `025d59ba` probes pass real-capacity rollback and duplicate-identity
controls. Constructor capacity 1 accepts two controls and refuses a tracked
input; that input leaves no recovery ticket and can retry with the same delivery
identity once those controls are consumed. A duplicate live identity preserves
the original ticket and work. A desired lifecycle assertion fails: after a
successful delivery, dropping the frontend still leaves an ingress whose next
submit returns `Ok(ReadySequence(2))`, accepting work with no consumer. Evidence
is `.artifacts/private-admission-review-025d59ba/` (two controls PASS, one desired
assertion FAIL). Fixture compile errors are retained separately and are not
behavioral evidence. This confirms the consumer-close blocker rather than
accepting the candidate's complete lifecycle.

Three independent exhaustion tests pass against `025d59ba` with an explicit
`cfg(test)` counter fixture in copied source. Actual private submissions at
`u64::MAX` return their payload without a recovery ticket; repeated attempts do
not reuse a position or leak a ticket; entries accepted at `MAX-2` and `MAX-1`
remain consumable exactly once. Deliberately removing exhaustion rollback makes
all three tests fail. Evidence is
`.artifacts/private-exhaustion-review-025d59ba/`; this is boundary-fixture
coverage, not a naturally exhausted instance or complete execution acceptance.
The fixture's initial compile error is retained separately, and the runtime
mutation was restored byte-for-byte.

A separate poison probe confirms the source finding: after accepting control
transaction 8201, poisoning the shared queue makes `route_pending` return
`Ok(0)` without emitting the control. A healthy empty queue passes its control;
the desired unavailable-error assertion fails. Evidence is
`.artifacts/private-poison-review-025d59ba/`. The result proves failure is hidden
and accepted work is inaccessible through this consumer path, not that the
payload was destroyed or its completion settled. Consumer departure, poison
handling and owned control refusal still block integration.

Follow-up `7e1b0af3` returns refused controls, rejects producers after consumer
close, and reports poisoned reads as errors. An independent six-case replay
passes five controls covering rollback, duplicate identity, post-drop refusal,
healthy emptiness and poison reporting. The remaining desired assertion fails:
tracked delivery 9201 is accepted but not run before frontend drop; its terminal
receipt does not arrive within the fixture's 200 ms bound and its recovery ticket
remains present. Evidence is `.artifacts/private-close-review-7e1b0af3/`.

The source explains that failure. `close` returns queued work in a vector, but
`Drop` binds it to `_stranded` and then destroys it without reporting completion.
Poisoned close returns an empty vector instead of handing failure ownership to a
recovery path. Rejection of later work is therefore verified, while settlement
of already accepted work still blocks integration. Ported poison control
`11ea63aa` does not close this separate obligation.

The new consumer report contains sequence and class, not the delivered operation
identity. It also grows a vector until the queue is empty; concurrently refilling
producers can make that report and service turn exceed the queue's fixed bound.
Consumption needs a bounded service/report contract and assertions tying each
consumed payload to its original delivery or transaction identity. Final
authoritative execution remains separate from this reporting. Exhaustion tests
stay in the copied-source fixture; no production-source setter or layout debt is
requested. None of these runtime follow-ups has been integrated into this branch.

At `830c88c5`, an independent eight-case replay passes six controls: healthy
close now emits a real terminal input receipt, and observing it removes the
recovery ticket. Two desired assertions still fail. Close reports `TargetGone`
for delivery 9201 while its original target and registration remain live; a
truthful authority rejection/cancellation is required instead. Poisoned close
still emits no receipt within 200 ms and leaves the ticket present. Evidence is
`.artifacts/private-settlement-review-830c88c5/`. These are distinct findings:
healthy completion now occurs, its cause is wrong, and poisoned-close ownership
is still unresolved. The source also falls back to client 0 on failed target
lookup and discards completion errors; neither is a substitute for retained
admission and receipt identity.

Control shutdown need not invent an input receipt: existing
`XAuthorityClientControlAck` carries kind, transaction, surface and outcome, with
`AuthorityRejected` available for a genuine authority refusal. Its bounded
capacity and ownership still need to be preserved through close. Every current
`XAuthorityControlCommand` has `transaction()`; the new report's FocusSurface-only
match incorrectly labels the other controls untracked. Lease releases also have
an identity and an owner. Reporting needs to preserve those facts.

The candidate bounds a service call by queue capacity and allocates its report
before running effects, removing the prior unbounded growth. This does not yet
implement the chosen per-interval execution/cleanup budgets. Close still grows
a vector while holding the queue lock, rather than transferring preallocated
cleanup storage. The runtime candidates remain unintegrated; final guarded
execution and accepted control/cleanup settlement remain required.

Independent `5120624b` checks pass live-target input rejection with recovery
settlement and an exact `AuthorityRejected` control acknowledgement when its
channel has room. A third desired assertion fails under ordinary bounded
backpressure: an earlier real frontend close fills a capacity-1 channel with
acknowledgement 9300; another frontend accepts transaction 9301 and closes before
9300 is read. The original acknowledgement survives unchanged, but 9301 never
arrives within 200 ms. Evidence is `.artifacts/private-ack-review-5120624b/`
(two controls PASS, one desired assertion FAIL). Logging the failed `try_send`
still discards an accepted outcome.

The next fix must retain an owned drain/cancellation batch or reserved completion
storage, including failures to report. A queued lease release is a request to
perform settlement, not proof it happened; it must execute through privileged
cleanup, transfer to its durable owner, or be discharged by proved teardown.
No input receipt should be invented for it. Poison handling, unresolved input
and control backpressure remain obligations of that same shutdown owner.

Receipt attribution also needs precise language: `send_input_delivery` sends to
the issuer's receipt channel, not to the X client named in the receipt. Frontend
client IDs start at 1 and do not wrap. The former client-0 fallback was unsupported
attribution/correlation, not demonstrated delivery to an actual client 0. Removing
it does not by itself supply the missing completion owner.

Candidate `059df74e` adds explicit shutdown and retains failed operations inside
an opaque report. Source review confirms that local storage but finds no usable
external settlement path: the report exposes only `owed` and `is_settled`, while
shutdown consumes the frontend without retaining its originating settlement
access in the report. Exporting stamped envelopes would not fix that lifetime
problem and is not requested.

The chosen API direction is an opaque, origin-bound settlement handle with a
bounded retry/progress operation. It must retain access to the owner capable of
settling its pending work, or refer to a durable instance-owned settlement
record. Full acknowledgement channel, shutdown, capacity becoming available,
retry and exact acknowledgement once is the public-API acceptance case. Retry
must not route a batch through another frontend with colliding local identities,
or select an execution recipient early to manufacture completion ownership.

`#[must_use]` is a lint, not a linear ownership guarantee. Dropping the current
report destroys its obligations, and fallback frontend Drop still logs and
then drops its report. Abandoned handles need retained origin-owned cleanup or
an explicit instance failure/teardown path that discharges the obligations.
Poison cannot be reduced to a boolean after losing access to the pending queue.
Preallocated drain storage, bounded cleanup service and truthful completion
remain required. No public envelope API or additional operator decision is
needed for this already-authorized implementation. The candidate is unintegrated.

Candidate `deb873c8` retains the originating registry in `PrivateSettlement`
and exposes bounded retry without an external authority argument. This supplies
the capability missing from the earlier report while keeping operations opaque.
Source review still finds abandonment unsafe under continued backpressure:
`Drop` calls `settle_against(...).len()`, which destroys the returned surviving
operations immediately. Losing their final owner does not establish that their
live acknowledgement receiver disappeared or that instance teardown settled
them. A final best-effort attempt is not a durable ownership transfer.

The repair must transfer abandoned work to a bounded, durable origin-owned
settlement service, or establish a specified terminal instance outcome through
proved teardown. It must not block indefinitely in Drop, retain an unreachable
self-cycle, or treat a log as a completion. That owner must also retain the
poisoned queue and its failure authority, rather than only the unreadable flag.
Independent evidence at `.artifacts/private-settlement-review-deb873c8/` records
two controls PASS and one desired abandonment assertion FAIL (131 filtered;
10.03 seconds). A real prior shutdown fills the acknowledgement channel;
freeing capacity and retrying returns the exact `AuthorityRejected` outcome
once, and a repeated retry answers nothing. Two separate origins both retain
pending work with colliding client/surface IDs; retrying them in reverse order
still reaches only their respective receivers.

The negative keeps acknowledgement 9700 in the capacity-1 channel while dropping
the pending handle for accepted transaction 9701. The original 9700 remains
intact, but 9701 has no outcome within 200 ms. This bounded observation, together
with source-confirmed destruction of the surviving operations and absence of a
post-Drop settlement owner, establishes the remaining ownership blocker. It is
not evidence of recipient failure. No runtime integration or executor acceptance
follows from the two positive controls.

Candidate `a5ba03d1` adds a shared settlement owner and guards against settling
the frontend twice. Source review finds the transfer still fallible after
acceptance: a full owner increments `lost` and destroys the operations that do
not fit. Settlement capacity must instead be reserved before accepting work,
with ownership retained across queued, executing and abandoned states. A new
operation may be refused with its payload; an accepted obligation cannot be
discarded because its subsequent owner has no room. Capacity accounting must
cover multiple instances sharing an owner and repeated shutdowns, not only a
single admission queue.

The claimed poisoned-queue transfer is also absent in this candidate:
`unreadable_queues` stores a count, not the admission queue or its origin-bound
failure authority. Poisoning the settlement owner's own mutex additionally
makes transfer drop its pending vector and makes `drive`, `owed` and `lost`
return zero. The failed-instance owner must retain the actual state, expose a
typed unavailable result, and avoid resuming poisoned execution. A boolean or
counter on a longer-lived object does not supply that ownership. The candidate
remains unintegrated pending these corrections and independent capacity tests.

The independent capacity test at `.artifacts/private-owner-review-a5ba03d1/`
records one positive PASS and one desired conservation assertion FAIL (133
filtered; 9.83 seconds). The public capacity-1 owner successfully retains one
abandoned control and delivers it after a real prefilled acknowledgement is
read. In the negative, one frontend with input capacity 2 accepts controls 9901
and 9902 while real acknowledgement 9900 fills the output channel. Shutdown
reports two owed operations; dropping the handle leaves owner `owed=1, lost=1`.
After reading 9900 and driving three bounded rounds, only 9901 arrives; 9902
is absent and the owner reports `owed=0, lost=1`. This is loss after acceptance,
not an admission refusal. Poison ownership findings above remain source-review
findings rather than newly reproduced behavior.

Candidate `8dab7e26` reserves shared owner credits before ready admission and
retains an actual failed admission queue. The shutdown-only capacity repair
does not yet establish the claimed credit lifetime: `route_pending` releases
the credit as soon as `run_one` returns, while `route_control` can have merely
queued a command to its client writer. Queue consumption and writer enqueue
are not a terminal acknowledgement. Credits must follow pending writer and
frozen work until a real terminal outcome, with an owned continuation when
execution returns an error rather than consuming the operation and stranding
its reservation.

Failed-queue retention introduces a strong-reference cycle:
`PrivateSettlementOwner.inner` owns `failed`, which owns `Arc<SharedAdmission>`,
which owns a `durable` clone pointing to that same `inner`. The retained queue
must instead be an owner-independent storage record without a strong back
reference. Failed-instance storage also needs its own reservation before
exposure: the `failed` vector is unbounded independently of operation credits,
including failures of empty instances, and currently allocates during transfer.

Settlement-owner poison remains explicitly open. Transfer still drops its
input on a poisoned owner; zero-valued status/drive results hide unavailability;
and reservation translates that condition into `Saturated`. These require
preserved ownership and typed failure, not retry-as-capacity or settled/empty
reporting. These are source-review findings; the runtime candidate remains
unintegrated.


Independent `.artifacts/private-credit-review-8dab7e26/` records two controls
PASS and one desired credit-lifetime assertion FAIL (138 filtered; 9.53
seconds). Capacity 1 now refuses the second owned payload before acceptance;
the accepted operation receives its exact outcome once. Two instances share
that limit, and an actual shutdown acknowledgement for the first operation
releases capacity so the second instance's retained payload can be retried.

The execution-path negative routes control 10201 into the client control
channel and inspects that exact queued command without executing a writer or
producing an acknowledgement. The owner already reports `reserved=0` and
admits 10202 at sequence 2. The desired assertion that 10201 still holds its
credit fails. Thus the admission/shutdown controls pass, while the production
consumer still reclaims settlement capacity before the operation is answered.

Candidate `33bfadc8` removes the failed-record strong cycle by retaining the
queue as a leaf, and distinguishes unavailable owner reservation from ordinary
capacity exhaustion. It adds a recovery operation that drains retained failed
queues for cancellation against their originating registry, without resuming
request execution. The known terminal-credit and owner-poison defects remain
open.

Source review finds that failed-instance slots are still not reserved before
frontend exposure. Preallocating the owner's vector does not reserve a slot for
each constructed frontend: `take_failed_instance` returns without retaining the
queue when the vector is full. An empty failed instance can occupy that capacity
without consuming an operation credit, leaving another already-exposed instance
unable to transfer its accepted work on failure. A separate instance reservation
must be acquired before exposure and retained until its failure/settlement
obligations end. Capacity refusal belongs at that earlier boundary, not in Drop.

`recover_failed` also replaces the preallocated failed-record vector with an
empty capacity-0 vector through `mem::take`, then drops the original buffer.
Subsequent failure transfer therefore allocates during cleanup. The storage
must survive and be reused across recovery cycles. These source findings keep
the candidate unintegrated.

Independent `.artifacts/private-failed-review-33bfadc8/` records one positive
PASS and one desired conservation assertion FAIL (139 filtered; 9.68 seconds).
Recovering one poisoned queue emits the exact `AuthorityRejected` outcome once
and sends no command for execution. In the capacity-1 negative, empty failed
instance A occupies the failed-record slot while operation credits remain zero.
Instance B can still be constructed and accept control 11101. B then fails and
closes, but the owner retains only A. Recovery drains empty A and produces no
outcome; B's receipt times out after 200 ms, leaving `failed_instances=0` and
`reserved=1`. The test permits an explicit owned refusal before acceptance;
this candidate instead accepts and loses the failure transfer. Buffer reuse
and the removed cycle remain source-review findings, not additional runtime
tests in this result.

Candidate `d04359fa` adds failure-slot reservation before frontend construction
and retains the instance's failed status so its subsequent Drop does not
release a slot already handed off. The owner releases the separate slot after
clean close or recovery of the retained failed queue. Independent
`.artifacts/private-slot-review-d04359fa/` records three controls PASS, no FAIL
(141 filtered; 9.78 seconds). An empty failed instance prevents a replacement
from being constructed until recovery. A retained failed handle continues to
hold its slot after the consumed frontend drops; handoff and recovery produce
the exact `AuthorityRejected` acknowledgement once, without executing a command.
Repeated recovery and old-instance teardown do not free a newer occupant's slot.
Clean shutdown permits reuse, and dropping its older clean report does not free
the replacement's slot either. This establishes only the bounded failure-slot
lifecycle, not the outstanding terminal-credit or owner-poison paths. The
combined runtime candidate remains unintegrated.

Draining `held.failed` now preserves its backing allocation. Collecting that
drain into another vector still allocates under the owner lock, so only the
specific lost-buffer defect is repaired; allocation-free recovery is not
established. Construction refusal also currently consumes the supplied gate
and both sender handles. Preserve those caller-owned inputs in a returned
configuration or borrow and clone them after reservation, so a refusal permits
retry with the same inputs rather than requiring the caller to reconstruct
capabilities it already supplied.

Source review of `844f57a7` confirms the constructor-input repair:
`PrivateFrontendParts` is returned unchanged with the refusal before it is
destructured, so the caller retains its original gate and both sender handles.
This narrowly closes the reviewed ownership issue. No new independent runtime
test result is claimed for this small API change; the earlier slot-lifetime
evidence remains pinned to `d04359fa`. Terminal-credit integration and owner
poison remain the next runtime work, and the combined candidate remains
unintegrated.

Candidate `14126862` stops releasing a credit immediately after the ready
consumer routes its operation and introduces explicit reclamation. Source
review does not accept the terminal path yet: recovery's `ticket` lookup
returns `None` on a poisoned mutex as well as absence, and reclamation treats
both as completion. A missing public delivery ID also immediately releases
the credit even though the operation can still be pending in a writer or
frozen queue. Completion requires an internal, origin-bound operation record
with typed unavailable/pending/terminal status; no public receipt should be
invented for untracked input.

Control credits need an internal completion signal from the real writer
acknowledgement or terminal cancellation path. This does not require taking
over the external acknowledgement receiver or adding a public protocol. The
server-owned completion registration must exist before acceptance and preserve
the original operation identity through execution and cancellation. Holding
credits indefinitely is not a substitute for that signal.

Shutdown currently drains queued work only; the new `outstanding` identities
are not transferred with their completion access and disappear with the
frontend. Failed execution similarly retains only an identity after consuming
the work. Both need an owned continuation. In addition, the outstanding vector
is sized to one ready queue while its maximum occupancy follows the shared
credit capacity, so repeated consume/refill can allocate when recording work
after effects. These remain source-review findings alongside the explicitly
open owner-poison and allocation work. The candidate stays unintegrated.

Independent `.artifacts/private-terminal-review-14126862/` records one positive
PASS and one desired safety assertion FAIL (143 filtered; 9.58 seconds). The
positive routes tracked input into the actual client channel, supplies a modeled
terminal outcome through the production receipt-recording API, and observes
the receipt it produces. Recording without observation reclaims nothing;
observation then permits exactly one reclamation. This tests recording and
observation semantics, not an actual writer failure.

The negative poisons only the recovery ledger after live delivery 13002 is
routed, with no terminal outcome or receipt. The durable owner and admission
queue remain healthy. `reclaim_settled` nevertheless returns 1 and the owner's
reserved count becomes zero. This confirms that an unavailable ticket lookup
is being mistaken for terminal absence; it is distinct from the separately
acknowledged settlement-owner poison defect. No broader terminal lifecycle or
runtime integration is accepted on the positive control.

Candidate `1ab787ba` separates unavailable recovery lookup from ended delivery,
holds untracked input credits, and carries outstanding identities into the
explicit shutdown handle. Source review confirms these changes but finds that
the new state is not transferred on abandonment: `PrivateSettlement::drop`
handles only the failed queue and pending operations. Its empty-pending early
return destroys outstanding identities, and the nonempty-pending path never
transfers them either. The origin-owned completion records and their already
reserved credits must survive handle Drop and remain actionable through the
durable owner's progress operation. Carrying them into one more temporary
owner does not establish the complete lifetime.

Independent `.artifacts/private-outstanding-review-1ab787ba/` records three
controls PASS and one desired abandonment assertion FAIL (145 filtered; 8.93
seconds). Both prior recording/observation and recovery-poison controls pass;
the unreadable ledger now retains credit. A kept shutdown handle also waits
for a late modeled terminal outcome recorded through the production receipt
API, then reclaims once after the emitted receipt is observed.

In the negative, the handle is dropped before delivery 14002 ends. The same
origin subsequently records its terminal outcome, the emitted receipt is
observed, and the recovery ticket is gone. The durable owner nevertheless
reports `drive=0`, `recover_failed=0`, `owed=0` and `reserved=1`: the completion
identity was destroyed with the handle and its credit is stranded. No socket
writer effect is claimed by these modeled terminal controls. The initial test
overlay name collision is retained separately as a fixture compile failure,
not a runtime result. Control completion, untracked-input completion, owned
execution errors, owner poison and reserved outstanding storage remain open;
the runtime candidate is unintegrated.

Candidate `ae6fc5f2` transfers outstanding identities and their originating
registry to the durable owner before the pending-empty early return, taking
no new credit. Independent
`.artifacts/private-outstanding-review-ae6fc5f2/` records all four controls PASS
(146 filtered; 10.18 seconds): the recording/observation and recovery-poison
pair, late completion through a kept handle, and late completion after handle
abandonment. Driving the owner both before any terminal outcome and after a
recorded-but-unobserved outcome keeps `reserved=1, outstanding=1`. After the
produced receipt is observed, driving yields `reserved=0, outstanding=0`;
another drive does not release again.

This narrowly establishes the reviewed tracked-input abandonment path under a
healthy settlement owner. It does not establish actual socket-writer effects,
control or untracked-input completion, owned execution errors, poisoned-owner
handling, allocation-free cleanup or the production executor. The combined
candidate stays unintegrated. `drive` currently reports only newly settled
pending operations, so its return can be zero while it reclaims an outstanding
credit; callers must not confuse those two forms of progress. This reporting
qualification is separate from the passing credit-lifetime controls.

Source review of `f42e091f` confirms separate `answered` and `reclaimed`
progress counts, with `made_progress` covering either. No new independent test
run is claimed for that reporting change; settlement-owner poison still
returns default progress and remains open.

The control completion mapping needs the following production boundaries.
All eleven current writer acknowledgements call `X11ControlChannels::send_ack`,
but the original completion record must be reserved before private ingress
accepts the command. Routing attaches that existing registration; it cannot
first create it at the client-queue send. `FocusSurface` and `ClearFocus` branch
through `route_focus_control` and `route_authority_control`, bypassing the
proposed ordinary-control construction site, so both routes must carry it.

Acknowledgement publication is conditional: `try_send` success, full output,
and disconnected receiver are distinct outcomes. Full output retains the exact
acknowledgement and completion responsibility rather than replaying a command
whose effect may already have happened. Receiver closure is not proof the X
client disappeared. The hook must preserve those distinctions instead of
marking completion on entry to `send_ack` or its current success return, which
also covers receiver disconnection.

The stale-control helper is reached through the ordinary control-router wrapper;
the current private `run_one` calls the registry directly and bypasses that
wrapper. Its actual errors need owned cancellation/continuation. Completion
must be carried by an opaque server-issued operation registration, not looked
up solely by public client/transaction fields. Bind grant generation where
applicable without requiring a live synthetic grant for privileged issuer
cleanup. Writer stop, queue teardown and failures before acknowledgement also
need ownership paths; two acknowledgement-producing sites alone do not cover
the complete lifecycle. No public wire change or additional operator gate is
needed for this already-authorized internal completion work.

A bounded independent source audit of `f42e091f` confirms further paths that
do not reach `send_ack`. The control writer's stop check can exit with queued
commands, and its intentional client-termination branch returns after answering
only the current command. Errors after dequeue, including runtime/metadata lock
failures and output write/flush failures, can precede the final acknowledgement;
some follow state mutation. Preserve the current operation's execution phase
and separately retain queued operations rather than asserting no effect or
replaying an entire command. Receiver disconnection alone is not being claimed
to discard buffered mpsc work, which normally drains before disconnection is
reported.

Registration Drop currently cleans input recovery and frozen input but has no
control-completion sweep. Worker cleanup also needs a private lifecycle owner:
the dispatch join sequence can return on an input-writer error before stopping
or joining the control writer, and registration can fail after writer creation.
Every started worker and its accepted controls must retain cleanup ownership
across those exits. These are source-level lifecycle findings, not reproduced
timing failures or permission to alter ordinary mode without the planned
private-path separation.

Candidate `aa4898ca` adds production control-writer hooks, per-operation
registrations, retained acknowledgements and writer-exit sealing. The preceding
`f2144b51` adapts two CLI smoke callsites to refusable raw ingress; that is a
source-reviewed compile fix, not a smoke execution. The reported one-off
cross-client observation-order failure remains unresolved evidence, not an
independently established regression or an unconditional suite PASS.

The two named credit/acknowledgement tests construct `X11ControlChannels` and
manually call `begin_applying` and `send_ack_for` with a supplied outcome. They
test those helpers and the registry, not a real writer applying an effect.
Production writer calls do exist in this candidate; independent writer tests
are needed to establish their behavior and distinguish it from helper evidence.

Source review finds these cancellation blockers:

- Shutdown removes Accepted records even for commands already in a client
  writer's queue. `begin_applying` returns no success/refusal and silently does
  nothing for missing or poisoned state, so a writer can subsequently execute
  a command that shutdown has already handed out for cancellation.
- That handoff leaves an outstanding token classified Retired and a pending
  cancellation command owning the same credit. Reclamation can therefore run
  before cancellation acknowledgement publication and settlement can release
  again. Transfer needs one owner, not two terminal-looking copies.
- Focus routing can enqueue FocusOut and mutate the routed focus before the
  writer marks Applying. Cancellation must not classify those already-effectful
  controls as unexecuted. Execution claiming must precede the first effect and
  be atomic with cancellation, with an explicit writer continuation phase.

Independent registry evidence in
`.artifacts/private-completion-registry-aa4898ca/` records two controls PASS
and four desired safety assertions FAIL. A mismatched acknowledgement can
retire another command's token; a contradictory duplicate can overwrite a
retained outcome. Test-only counter-boundary fixtures also produce equal live
tokens and let an old retired token retire a newer command. The global origin
counter wraps by source inspection only. No practical counter exhaustion or
direct X11-client access to these server APIs is claimed.

The positive controls cover foreign-token rejection, phase preservation against
discard and ignored late publication after retirement. A capacity-1 registry
also retains 128 sealed client IDs with zero operations, demonstrating that
seal history is not covered by record capacity. The initial fixture compile
correction is retained separately. Mutation APIs returning unit/zero/default
on poison and the producer collapsing unavailable/sealed refusal into saturation
remain source-review findings. These are unfinished invariant checks and
lifecycle requirements; the candidate remains unintegrated.

Independent actual-writer evidence at
`.artifacts/private-control-writer-aa4898ca/` records two controls PASS and one
desired safety assertion FAIL (161 filtered). These tests start the production
control writer with an actual registered runtime window and owned Unix socket
pair. The normal path changes window geometry, emits `ConfigureNotify` and
permits credit reclamation. The full-channel path performs the effect once,
retains the exact acknowledgement, then republishes after the writer exits and
capacity is freed, without a second effect or release.

The negative holds the surface-window map lock after dequeue and before the
writer's phase transition. Shutdown and settlement retry publish
`AuthorityRejected` for transaction 73001 while the window width is still 40.
Releasing that barrier lets the same writer change width to 80, emit a 32-byte
`ConfigureNotify` event (type 22), and publish `Delivered` for the same
transaction. This establishes actual execution after cancellation and a second
terminal outcome. A successful execution claim must be atomic with cancellation;
the cancellation-winning path must prevent effects, while the execution-winning
path retains Applying responsibility until a truthful outcome is established.

Initial sandbox refusal at socket-pair creation and fixture corrections are
retained separately, not counted as runtime failures. The successful retry used
only owned sockets, no operator endpoint or hardware. These positives establish
real writer wiring but do not accept the cancellation/credit lifecycle or the
combined runtime candidate.

Independent review of the uncommitted repair atop `aa4898ca` used frozen staged
patches `19bfd3d6ed9bee3137c05ff29e37063ad1fe53be4e64260caa93a56776135b8e`
and `24c8227aded6792e2166385823ea585e0338d55dec58f36574bb3d2f963a94d5`.
The X-authority runtime Rust files in those two snapshots compare identical;
the snapshots and fixture changes retain separate provenance. No signed repair
commit or combined runtime acceptance is claimed.

The registry review at `.artifacts/private-completion-registry-staged-19bfd3d6ed9b/`
records four controls PASS and two desired safety assertions FAIL. Checked
operation-counter exhaustion, acknowledgement identity and retained-outcome
validation, and cancellation/foreign/duplicate-start claim checks pass. Bounded
seal eviction nevertheless lets the real producer accept the same dead-client
command it previously refused, and full seal storage can leave a client unsealed
with the writer's Drop ignoring `recorded=false`. These establish false
acceptance, not an untrusted wire exploit or proof that the later route executes.

The producer/shutdown review at
`.artifacts/private-preadmission-staged-19bfd3d6ed9b/` reproduces one safety failure
through real producer and shutdown methods. Holding the existing durable-owner
mutex pauses a producer after registration and before credit reservation or
acceptance. Shutdown takes that still-producer-owned registration. On resumption
the producer receives `ConsumerGone` and its original command, yet the settlement
publishes `AuthorityRejected` for transaction 83001. Reserved completion storage
must remain distinct from accepted responsibility until the admission handoff;
rollback after refusal cannot undo a cancellation that already answered it.
An inherited fixture compile error is retained separately from the successful
runtime reproduction.

The actual-writer review at `.artifacts/private-control-writer-staged-repair/`
on staged patch `24c8227a` has four positive controls and two channel-publication
safety failures. Normal writer effects, exact
acknowledgement retry under Full, execution winning against shutdown, and
cancellation winning before routing with another operation's credit preserved
pass. However `send_ack_for` sends before validating the record and ignores its
refusal: a contradictory acknowledgement reaches the receiver while the original
outcome remains owed, and a retired token emits a second acknowledgement. A
registry rejecting the later bookkeeping cannot retract publication. Validation,
outcome establishment and publication ownership must precede the external send,
including retry. These results leave the candidate unintegrated and disabled;
Applying reconciliation, cancellation edges, budgets and the final guarded input
executor remain outside acceptance.

The follow-through at `.artifacts/private-dead-writer-staged-24c8227a/`
uses an actual writer rather than a synthetic seal alone: the writer applies
configuration and exits on a full acknowledgement channel while its client
registration stays alive. After the original outcome is republished and
unrelated seals evict its seal, the real producer accepts transaction 77002
without a writer to execute it. This rules out client-registration liveness as
the repair. Moreover, `InputRecovery::active(None, client)` is only a
not-known-revoked check: an absent client also passes. A private admission needs
positive ownership of the exact writer lifecycle, with accepted work retained
through a later exit. No change to ordinary recovery semantics is implied.

Follow-up guidance requires a fallible atomic handoff from a reserved completion
to an accepted queue entry, and phase validation before publication: a Reserved
record is still producer-owned and cannot justify an acknowledgement. The
uncommitted implementation and temporary mutations remain Claude's; review uses
frozen snapshots and does not treat an in-progress mutant as a candidate.

## Reconciliation with accepted content at 22c01aa1

Signed merge `e48571f1` brings accepted master `22c01aa1` into the isolated input
branch without moving master or importing Claude's completion candidate. Only
`todo.md` needed manual conflict resolution; t081, t093, t094 and t096 are each
retained once. Both Session test-file changes and the common-authority lockfile
entry survive, and the remaining content files match master exactly.

Workspace compilation reproduced the refusable-input-sender mismatch in two CLI
smoke call sites. The self-contained fix `f2144b51` was independently reviewed
and imported alone as `504ae33f`; no smoke was executed. At that exact source,
workspace all-target compilation, affected clippy and source layout pass.
Common-authority, X-authority and default Session tests record 954 passes and one
ignored; the native-feature Session run records 655 passes and 13 ignored. These
are test executions across feature configurations, not counts of unique cases.
The existing backend `with_accepted_capacity` dead-code warning remains.

Evidence is retained at `.artifacts/input-reconcile-22c01aa1/`, including the
pre-fix compiler failure and overlap checks. The canonical check uses the
device-hidden wrapper and retains its own exact-source report separately; these
targeted results are not a full canonical or hardware claim. The four existing
notebook broken-link reports are unchanged. XTEST stays disabled and M3's later
production lifecycle work remains unintegrated.
