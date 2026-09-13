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
