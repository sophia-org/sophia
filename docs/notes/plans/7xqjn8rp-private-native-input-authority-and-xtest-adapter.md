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
| M2 | Add `sophia-input-authority`, depending only on protocol and std; test native transitions before an adapter exists | Define required obligations and strict execution evidence |
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

The selected acquisition rank is outer X runtime, common authority, surfaces,
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

t093 tracks this implementation, t094 the deferred physical-device prerequisite,
and t057 remains the broader protocol conformance task.
