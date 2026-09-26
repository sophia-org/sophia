---
id: uf2wya88
date: 2026-09-25
kind: investigation
status: implemented
tags: [investigation]
---
# Typed WM driver preserves current IPC phase and shutdown ownership

## Question

Can t248 expose semantic WM commands and results to a future file adapter
without moving policy authority or duplicating the current driver phases?

## Evidence

Signed source `45e61551622a3b5c1d84c36dea4e9668dbf30581` on
`session/t248-wm-adapter` extracts five Session files from accepted `611507b5`.
Logs are retained in `sophia-borders/.artifacts/t248-wm-adapter`.
All compilation and tests used the device-hidden isolation wrapper, nice19,
two jobs and the exclusively allocated disk target
`sophia-t027/.artifacts/t026-target`. Compiler paths identify sophia-borders.

## Finding and resolution

The private `PolicyAdapter` accepts semantic commands and yields decoded
configuration, dirty, projection and session-operation results. Profile
admission carries neutral identity and transaction correlation. The current
IPC adapter retains negotiation, profile handoff, transfer assembly and codecs.
Existing constructors and supervised endpoints remain unchanged.

The driver still owns command order, one-slot owner queues, response deadlines,
capability checks and shutdown. `LivePublicPolicyState` remains the only policy
reducer and settlement owner. Output-role IPC is unchanged. A malformed completed
projection is represented separately so moving its decode does not replace an
earlier phase refusal with a decode error.

The six scripted controls exercise the actual worker and driver, including
profile refusal before Negotiated, cycle/outcome/operation/receipt ordering,
transfer-phase refusals, rejected outcomes, malformed-message phase precedence,
and shutdown while the owner event queue is full. Scripted messages are supplied
semantic values, not proof of wire decoding, protected admission, a valid
reducer proposal or native completion. The existing two real IPC profile tests
and existing queue shutdown control also pass.

## Validation and remaining work

Focused worker controls: 9 passed. Full native-session lib: 620 passed,
0 failed, 18 ignored. Strict Session native-session all-target Clippy passed;
format and diff checks passed. The initial cached layout command also returned
success, but the freshly rebuilt t249 xtask later flagged the production-file
`#[cfg(test)]` mount as inline tests. That earlier result is insufficient for
layout acceptance. Matching the existing shutdown fixture, the attribute now
lives inside the external test-support file; no test body or ledger changed.
Fresh-root layout then passed; both logs are retained in the t249 evidence.
No additional worker suite was run for that attribute-only relocation before
releasing the serial build slot. These are affected-owner checks, not a full
workspace or paired Hagia acceptance claim.

The first focused run was 8 passed and 1 failed: the new refusal fixture observed
the adapter's disconnect trace just before the worker dropped its event sender,
then incorrectly required immediate channel closure. Both terminal assertions
now wait at most two seconds for actual channel disconnection. The failed log
`focused-initial-fixture-race.log` is preserved beside the passing `focused.log`;
no production behavior changed for that fixture repair.

The 9P adapter, independent record codec, staging/submit admission and paired
Hagia integration remain subsequent work. No live, device or physical run was
performed. The file-contract review requires opened-handle snapshot metadata,
bounded send pressure, no fragment-driven phase changes and cancellation that
preserves already acknowledged staging bytes. Those rules do not change this
independently reviewable current-IPC extraction.

## September 26 extraction-exit reconciliation

The t248 exit was reviewed against integrated Sophia
`11d6deef96902e49249c496aa569206818fa87af`. The retained extraction at
`45e61551622a3b5c1d84c36dea4e9668dbf30581` and follow-up `95b39662`
satisfy that scoped exit: six real-driver semantic-adapter controls, two real
IPC profile controls and the full-event-queue shutdown control pass (9/0).
The native Session library passed 620/0 with 18 ignored; strict Clippy passed.
These are the logs under `sophia-borders/.artifacts/t248-wm-adapter`, not new
runs on the integrated revision.

The initial cached layout result is not reused as acceptance. The external
test-mount correction `7567c745` and fresh
`sophia-borders/.artifacts/t249-neutral-records/layout-final.log` supply that
check. Later `permit-focused.log` (10/0) and
`t249-profile-neutral/{protocol,runtime}.log` (7/0 and 21/0 with one ignored)
retain the driver wait/Stop and neutral profile identity/handoff controls.
No implementation gap remains in the behavior-preserving extraction.
This finding does not close t249's file-role, measurement or lifecycle exit.
The [separate integrated qualification](pp3pk4dd-read-only-wm-inspection-preserves-host-admission-and-writer-progress.md#september-26-qualification-follow-up)
records the later X-authority harness repair and workspace gate.

## Driver hooks for the file-owner checkpoint

The later t249 driver-only checkpoint passes a non-cloneable receive permit at
each existing wait site: configuration, idle dirty, projection (with the
existing before/after-transfer dirty distinction), and committed session
operation. Current IPC deliberately ignores that hint and retains its original
decode/refusal precedence. A file owner will consume it only after successful
complete-candidate custody; staging fragments and accepted-submit replays cannot
spend a second permission. No export phase machine is introduced.

An optional adapter Stop handle is captured before worker spawn. Stop and Drop
wake it independently of command queue space, before joining the producer;
current IPC supplies no hook and keeps its existing socket-bound cleanup.
The external control blocks the actual driver's synchronous send on a bounded
simulated credit wait, fills the command queue, then verifies both explicit
Stop and Drop wake it promptly and disconnect once. It does not claim an actual
9P socket/journal yet. The existing full-event-queue shutdown tests remain.

Focused worker checks: ten passed, including real IPC profile controls. Strict
native Session all-target Clippy passed after a nested-if lint correction.
Logs are `permit-focused.log` and `permit-clippy.log` under the t249 worktree
evidence directory. Profile handoff, public policy reducer, output-role IPC and
default transport selection are unchanged.

## Supplied-stream file custody checkpoint

The t249 file owner adopts a Unix stream already admitted by its caller. It
does not select a transport, spawn Hagia or infer authority from attach names.
One continuing Session-owned Qid allocator is passed explicitly; two epochs
receive disjoint paths, opened transaction and snapshot metadata name their
specific object, and exhaustion refuses before changing the allocator. One
attach consumes the admitted epoch; version reset does not renew it.

The export holds staged bytes, one accepted candidate, one transient driver
permit and a bounded event journal. No-permit submission refuses before row
decoding; the named counter control proves this. Successful complete submit
reserves a whole Submitted record before transferring custody, while duplicate
accepted submit bypasses decoding and leaves the next permit unspent. ACK
releases transport bytes only. Snapshot handles keep immutable bytes and
metadata while newer publication replaces the current snapshot. No second
driver phase or reducer is introduced.

The adopted-stream reactor owns mutation on one thread. A full journal services
real 9P ACK traffic while the caller retains its borrowed in-flight command.
Stop wakes the core poll independently of event credit; a peer that never ACKs
reaches the fixed four-second send bound. Staging keeps its first-write
twelve-second deadline despite retries. Limits are 64 event records / 1 MiB,
one staged candidate / 1 MiB, one pinned snapshot plus the current object,
and the core's explicit message, fid, pending-request and output bounds.

Device-hidden focused worker checks pass 22/22: twelve custody controls and
ten retained adapter/shutdown/current-IPC controls. Strict native Session
all-target Clippy passes. Evidence is in
`sophia-borders/.artifacts/t249-file-owner`: `focused-reviewed.log`,
`clippy-initial.log`, and `layout.log`. The first compile's private re-export
failure and the first real-array fixture's disabled-chrome/nonzero-width
failure remain separately in `focused-initial.log` and `focused-arrays.log`.
Both were corrected; neither is a physical observation.

The new array control calls the actual complete configuration decoder and
neutral row owner with the selected capability ceiling: unnegotiated chrome
refuses, then an admitted configuration succeeds with the same unspent permit.
Other scalar payloads and Submitted bodies are explicitly test-codec values.
The permit control obtains a real driver-issued permit but stops its scripted
peer afterward; it is not a complete driver-to-file startup proof. Actual socket
controls cover ACK/Stop transport custody, not protected admission, Hagia,
profile handoff, proposal settlement, presentation receipts or physical output.
The complete role adapter and its launch constructor remain subsequent joins;
default IPC, output-role transport and LivePublicPolicyState stay unchanged.

## Neutral profile records and existing handoff I/O

The profile extraction names the passive records `PolicyProfileIdentity`,
`PolicyProfileOutcome`, `PolicyProfileCommand` and `PolicyProfileCompletion`.
The old `WmV1Profile*` exports remain aliases of those same types. Constructors,
outcome codes, error values and generated legacy frame codecs are unchanged.
A new control compares all six typed codecs with the pre-existing generated
golden frames and checks invalid-identity error precedence through both names.

`PolicyProfileHandoffIo` supplies typed send-effect and receive-completion
operations to the extracted existing execution loop. The existing reducer
still owns phase, exact transaction/epoch/generation/digest correlation and
rollback. The shared helper neither retries input nor invents automatic rollback;
it returns the rejected candidate model from a single step, as before. Current
IPC delegates to that helper without changing socket reads, framing, deadline
behavior or refusal precedence. This helper uses the existing Linux transport
error type and is gated with that platform; the passive records and pure
reducer remain platform-independent.

In particular, the existing four-second admission socket timeout is a timeout
on socket operations, not an absolute four-second complete-frame deadline:
`receive_frame` can perform multiple reads. The extraction does not silently
harden that behavior. The file transport's separately specified absolute send
deadline remains a different contract.

Focused evidence in `.artifacts/t249-profile-neutral` comprises seven protocol
checks and twenty-one runtime checks with one pre-existing ignored case. These
include the shared helper's ordering/error controls, the pure reducer, and
current IPC prepare/activate/rollback, rejection and out-of-phase controls.
The runtime target also contains optional Hagia fixtures; the target's passing
total alone does not assert that an independent Hagia binary ran. No file
profile bodies, startup transport selection, capability negotiation or replay
policy change is part of this extraction.

The separate read-only replay audit found one unbounded per-epoch
`PolicyConnectionState.used_transactions` set shared by IPC projections,
configuration, dirty controls and session operations. File submission IDs
cannot stand in for those transactions. Hagia `97ed593e` allocates one increasing
client transaction counter for configuration/projection/operation (and its old
Dirty envelope); profile completions instead echo server commands. This
supports a separately declared bounded file-only domain watermark, with gaps
allowed and no domain transaction assigned to Dirty. That proposed follow-up
does not alter the legacy set. Full runtime begin/append/finish capability,
aggregate and presentation checks remain distinct from the less strict direct
legacy codec characterization recorded in the neutral-array investigation.

## File-only semantic transaction watermark

The supplied-stream file owner now retains two independent increasing values:
the accepted submission ID and the last accepted semantic transaction. The
latter is taken from a validated Configuration, Projection or SessionOperation
event. Dirty does not have a semantic transaction, and server profile
transactions are outside this watermark. Gaps are allowed. A fresh admitted
epoch starts new watermarks while retaining the explicitly supplied logical
filesystem Qid allocator. Legacy IPC's transaction set is unchanged.

A domain transaction at or below the watermark returns EALREADY with staging
intact. Both watermarks advance only after the complete Submitted record has
reserved journal custody. Decode, capability, absent/wrong permit and journal
credit refusals consume neither ID. Exact retained submit retries still take
the earlier duplicate path without decoding, another permit or delivery.
Transport ACK and a later semantic rejection do not make a consumed ID reusable.

Evidence is retained in `.artifacts/t249-file-replay`. The focused restored run
passes 26 controls, including four new replay controls. Disabling the replay
guard in a compiled negative makes three of those four fail; the source is
restored before the passing rerun. The prior focused run of 25 controls is
retained separately, before the explicit rejected-outcome case was added.
Strict native-session all-target Clippy, the rebuilt worktree layout gate,
format and diff checks also pass. All compile/test runs used the exclusive
disk target, two jobs, nice 19 and the device-hidden wrapper.
These controls exercise actual file custody with driver-issued permits and a
supplied typed candidate decoder. The rejected-outcome control supplies a real
encoded outcome to the journal; it does not claim a reducer actually rejected
a proposal. Array/scalar semantics remain covered by their independent codec
controls. No startup adapter, capability selection, launch constructor or
independent Hagia pairing is established by this checkpoint.

## Shared capability selection

`select_policy_capabilities(offered, ceiling, profile_activation)` owns the
existing supported set and its two dependency reductions: presentation actions
require surface instances, and output launch context requires launch origin.
It is pure. Peer authentication, connection mutation, one-time negotiation and
required-mask refusal remain caller responsibilities. Profile activation is
available only when the caller supplies the existing profile admission mode.

Current IPC calls this function at the old selection site, after the same
connected/negotiated/revision checks and state writes. Its transport still masks
the Hello with its ceiling before that call. No revision, wire, error-order or
legacy replay change accompanies the extraction. The next file admission owner
must check missing required bits after both reductions, rather than assigning
that decision to the body codec.

Focused evidence in `.artifacts/t249-capabilities` passes three new pure/order
controls, fifteen existing IPC controls, and thirteen transport controls with
one pre-existing ignored case. The transport total includes optional external
client fixtures and does not alone establish an independent Hagia run. The new
controls cover every individual bit, both dependency combinations in offer and
ceiling, profile-mode gating, and revision/refusal state order.
Strict runtime all-target Clippy, rebuilt worktree layout, format and diff
checks pass on the same source; the exclusive target and isolation are unchanged.

## Private supplied-stream startup

The next bounded join is `FileStartup`, not a complete `PolicyAdapter` or a
production endpoint. Its input stream and epoch are already admitted by the
caller. The logical filesystem Qid allocator and immutable mechanism ceiling
are also supplied. Nothing here authenticates a process, selects a launch
transport or migrates the separately admitted output-role channel.

The existing driver supplies one non-Clone admission token. Consuming it
derives a single Negotiate receive permit and permission to derive a completion
permit from an exact existing profile-reducer Send effect. Runtime permits
accept neither offers nor profile completions. The file owner has no profile
phase enum. Shared full-value codecs decode candidates; the owner supplies the
selected capabilities, checks the admitted epoch and retains submission
custody. Required capabilities are checked after the shared selection function
has applied both dependency reductions. Selection binds once within the supplied
ceiling and remains readable from that canonical owner; Limits stays immutable.

File Negotiated is journaled before optional prepare/activate exchange. The
existing `PolicyProfileHandoffIo` executor and reducer retain exact command,
transaction, generation, digest and outcome correlation. A wrong header epoch
is refused before custody, as is a wrong completion kind; both preserve staged
bytes. Under the admitted epoch and exact completion kind, a wrong transaction,
generation or digest reaches the reducer and is terminal. Every fatal startup
return revokes the file owner before dropping the adopted stream. Stop wakes
the same reactor during offer/completion waits and journal-credit waits.

Offer and each completion have the existing twelve-second driver response
budget as an absolute deadline, not a per-read timeout; fragments and ACKs
cannot renew it. Event sends have the shared four-second absolute budget.
Journal encoders receive the real owner-issued sequence/epoch header, and only
complete records consume journal custody. Shared API constants govern the
64-record/1-MiB journal and twelve-second candidate assembly bound.

The private helper deliberately does not implement runtime `send(Cycle)`.
Snapshot bytes, a new Qid and the matching Cycle event still need one atomic
reservation/publication join before a complete adapter can be claimed. Internal
driver Negotiated remains after the entire `admit` call; current IPC ignores
the new private token and retains its existing behavior.

Evidence in `.artifacts/t249-file-startup` includes eight new controls plus
the twenty-six retained driver, IPC, custody and replay controls. The raw-wire
Rust peer negotiates the actual adopted .L stream, retries accepted custody,
observes file Negotiated before exact prepare/activate exchange, and checks
wrong-epoch and wrong-kind retained staging separately from terminal wrong-tx
reducer rejection. Stop during offer/profile receives closes the adopted
stream. Other controls pin immutable Limits and one-time selected-set binding,
runtime-permit exclusion of startup messages, and unnegotiated extension
refusal in the typed codec. This is neither protected-peer authentication nor
an independent Hagia executable run; full runtime adapter, snapshot/Cycle
publication and supervisor reconnect acceptance remain subsequent work.

Two compiled negatives remove the missing-required check after dependency
selection, or decode projections with all capabilities instead of the selected
set. Each fails its named control; both source files are restored for the
passing rerun. Initial logs retain two fixture setup failures (a nonexistent
submit encoder assumption, and changing a fixture's outer epoch without its
embedded launch-origin identities) and a strict test-expression lint. None is
reported as a production failure or discarded as acceptance evidence.
The final focused run passes 34/0, strict native-session all-target Clippy
passes, and the freshly rebuilt worktree layout gate, format and diff checks
pass. Builds use the exclusive disk target, two jobs, nice 19 and the same
device-hidden wrapper. No physical, device or production endpoint run occurred.

## Complete private adapter on a supplied stream

The next checkpoint implements every `PolicyAdapter` command through the
private `NinePPolicyAdapter`, retaining the existing driver as phase owner and
the existing profile reducer as handoff owner. It introduces no launch selector,
protected admission, default transport change, output-role migration or second
semantic reducer. Outcomes and receipts in these controls are scripted owner
inputs, not proof of Engine settlement or native presentation.

Snapshot and Cycle publication share a borrowing journal reservation. Validation,
event credit, encoding, sequence/tail exhaustion, cancellation and the fixed
send deadline precede Qid allocation. Qid allocation is the last fallible step;
snapshot replacement and journal commit then occur without a reactor turn.
Dropping a reservation changes neither journal nor old snapshot. A failed
allocation spends no identity. Existing opened snapshots keep their immutable
bytes and metadata.

The adapter temporarily clones the typed scene, actions, classifications,
launch origins and request before event-credit checking. This is additional
memory beside the current snapshot and at most one older opened pin. Only
after event credit does it encode a fresh snapshot candidate, bounded to the
one-MiB file-object limit; it does not retain per-event snapshot copies or add
another command queue. This is not a claim of zero transient typed copying.

All Stop handles share one transport-local flag and wake the same reactor.
Cancellation is checked before and after encoding. Stop racing after the final
check may linearize after publication; no retroactive undo or wall-clock
guarantee that publication cannot follow `stop()` returning is claimed. Later
operations refuse, and worker cleanup revokes and drops the supplied stream.
The actual `expect_session_operation` value is encoded with its capability
check; the file owner neither infers nor owns the resulting driver phase.

Evidence is retained in `.artifacts/t249-complete-adapter`. Named compiled
controls remove the operation flag, the post-encoding cancellation check, or
move Qid allocation before validation. Each fails its intended assertion;
restored source is checked again. The first focused run retained a deadline
diagnostic mismatch (`Errno(110)` instead of the existing deadline text), and
strict Clippy retained unused wrappers and duplicate fixture-module imports.
The deadline text is preserved, unused wrappers removed, and tests now share
one existing semantic fixture module.

The final restored focused run passes 41 controls, fails none and explicitly
ignores the two external-peer controls. Strict native-session all-target
Session Clippy, fresh worktree-root layout, format and diff checks pass. The
exclusive disk cache, device-hidden wrapper, two jobs and nice 19 are retained.
No broader owner suite or production endpoint is claimed by this checkpoint.

Two external peer controls remain explicitly ignored until a prebuilt Nim
executable and exact SHA256 are supplied. They require a fresh evidence case
directory, bound accept/startup/exit waits, retain child logs and start/end
binary hashes, and kill/reap the child on failure within the stated fixture
bounds. The test socket is supplied admission, not production authentication;
artifact hashes are not TOCTOU-free launch authority. Fixed profile identity is
epoch 9, generation 3 and digest `[7;32]`, with prepare/activate transactions
40/41. Cycle uses snapshot/request transactions 100/101, request 55, scene 7
and policy 3. The bridge asserts independently decoded proposal transaction 11,
output 1, focus `(3,1)` and its one state-generation-8 placement at `(0,0,100,100)`.
Scripted operation and receipt completion does not establish physical or
independent application acceptance. No independent executable pass is claimed
by the ordinary focused suite.

The independent frozen Hagia peer at `586b9a4dce3d766127b575720718339cb6ac6071`
was subsequently run against signed adapter `99601a30`: both explicit startup
and cycle tests pass. Its candidate writes use 17-byte fragments, journal reads
23 bytes and immutable-object reads 37 bytes. The original paired bundle
`t249-peer-99601a30-586b9a4` remains preserved under the development-evidence
directory. A test-only follow-up measures `SO_PEERCRED` on the accepted socket
and requires its PID to equal the spawned child PID. Identity records explicitly
name path hashes before and after execution, not descriptor-pinned execution;
the hash-then-exec window remains. This socket credential check is not a
substitute for the production protection-domain admission owner.

The file protocol has no `ProjectionPending` fragment event: Dirty remains
allowed under the existing driver permit until the complete projection obtains
custody. File fragments neither change driver phase nor renew its response
deadline. A closed adapter's `selected_capabilities() == 0` is its unavailable
sentinel, not evidence of a successfully negotiated empty capability set.

## Pending protected endpoint, before production launch selection

The next private constructor accepts an existing `PolicyRoleEndpoint` only
for the WM role. It takes a `ProcessSupervisor` reference, requires that
supervisor's retained protection evidence to name its peer PID and SpatialPolicy,
and calls the existing endpoint authorization method. The evidence is a
supervisor assertion produced by its actual protected launch path, not an
independent namespace inventory. The endpoint still owns exact socket UID/PID
credential matching. No runtime public API or production start site changes.

Admission polls that endpoint with one absolute twelve-second accept budget.
No-client polls use at most two milliseconds between cancellation checks;
wrong-peer/UID and other endpoint errors terminate this attempt rather than
renewing the deadline or silently retrying. Once accepted, the same typed file
and profile-handshake owner runs. Its offer and each profile completion retain
their separate existing absolute response budgets.

One cancellation state exists before accept and continues through reactor
installation. Stop sets its atomic flag, takes a wake clone under a short lock,
and wakes outside the lock. Installation publishes its wake and then checks
that same flag. No polling or socket IO holds that lock. Close and Drop revoke
and drop the reactor before releasing the actual endpoint peer and removing
the endpoint through its existing Drop. Logical Qids are supplied; this
constructor neither resets nor derives them from a socket or an epoch.

External controls launch the current Rust test executable through the real
`ProcessSupervisor` protection-domain path and reach the existing worker's
Negotiated event. They distinguish a missing launch record, wrong endpoint
role, a real protected metadata-broker record lacking SpatialPolicy, a wrong
UID and an unrelated connecting PID. Stop controls cover before/during accept,
after credential acceptance but before adoption, before/after wake installation,
and forced overlapping wake registration. The after-accept control composes the
actual endpoint accept, reactor adoption refusal and endpoint Drop; it checks
that only the endpoint disappears, a separate marker survives, and a fresh
bind has no active peer. It is not yet an automatic production restart test.
Fresh protected connections also receive the same supplied Qid allocator;
fresh epoch owners cannot reuse earlier allocated identities.

The child marker directory is an explicit test-only writable grant. It confirms
the child finished mounting its fixture and, for the success case, drained its
Negotiated ACK before the parent stops the worker. No output/GUI/device access
is added. This is a protected Rust child authentication control, not the later
frozen independent Nim protected pair or native semantic-settlement evidence.

Evidence lives in `.artifacts/t249-protected-endpoint`. Initial retained failures
were an old fixture moving a field out of the newly dropping startup owner,
a wrong-role fixture rejected by the supervisor before reaching the endpoint,
and a child final-ACK race caused by immediate fixture teardown. The fixture now
borrows the field, uses a genuinely launched metadata-broker role for the
endpoint negative, and observes bounded ready/negotiated markers. Compiled
negatives admit an unprotected PID or omit the post-registration Stop check;
each must fail its named control before restored-source acceptance.

The restored focused suite passes 51 controls, fails none and ignores three
explicit fixtures (the two supplied Nim runs and the protected Rust child entry
invoked by its parent). Both compiled negatives fail their intended guard.
Strict native-session all-target Session Clippy, worktree-root layout, format
and diff checks pass on the restored source. Device-hidden execution retains
nice 19, two jobs and the exclusive disk target. The compile slot is released
after all processes are collected; production endpoint selection remains a
separate reviewed checkpoint.

## Explicit production transport selection

The next checkpoint adds `--wm-transport=current-ipc|9p2000.L` to the existing
public WM Session path. Current IPC remains the default. This choice does not
add an interface or reducer, and attach cannot change it. A private factory
binds and starts the selected transport at initial launch, automatic replacement
and controlled restart. File transport requires the supervisor's protected
launch evidence; the existing IPC authorization and profile constructor order
remain intact. File diagnostics name `sophia_wm_fs_v1`.

The launch specification includes only the selected `SOPHIA_WM_SOCKET` or
`SOPHIA_WM_9P_SOCKET` key. The existing protected launch's `--clearenv` excludes
an inherited counterpart. Profile/checkpoint grants and the separate output
socket remain unchanged. The native-retirement capability ceiling applies
before file Limits are exposed. There is no reconnect fallback or default
change. One Qid allocator moves from prepared launch into the live Session and
is cloned into each worker; replacing a socket or connection epoch does not
replace that allocator.

The new external control invokes the actual prepare/activate startup, automatic
restart and control restart methods. A genuinely supervised protected Rust
child negotiates and completes a scripted profile exchange. It checks each
command against a parent-pinned expected generation/digest written before
launch, and validates the separately staged profile fragment using that key.
It retains the connection until the parent replaces it. Epochs 1, 2 and 3 each
reach this exchange and expose increasing Limits Qids. No Hagia semantics,
configuration promotion, projection settlement or native completion is claimed.
The fixture explicitly has `output_service=None`; separate existing output
service controls cover the pause/ReplaceSupervisedPid barrier, not a combined
native WM/output restart. Profile rollback coverage checks the existing reload
owner's retained selection and exact launch specification, not a protected
replacement process during profile rollback.

Evidence is retained under `.artifacts/t249-selection`. The first entry fixture
closed after Limits and therefore failed the real initial profile-admission
wait. After adding the exchange, a non-atomic marker write exposed an empty
file to the parent; atomic rename fixes that fixture race. Strict Clippy then
required boxing the larger IPC variant of the private factory enum. All three
failed logs remain. A compiled negative resetting Qids in every new worker
fails the actual restart control at `second > first`; the restored source is
rerun. These are headless, device-hidden controls on the exclusive disk target,
with nice 19 and two jobs. Broad integration gates and protected independent
Hagia pairing remain later checkpoints owned by the director.

Final focused results are 54 worker/selection controls passed, none failed and
four explicit fixtures ignored; configuration controls 49/0; existing output
service controls 7/0. The ignored entries are two separately pinned Nim supplied
peers and two protected Rust child entrypoints invoked by their parent controls.
Strict native-session all-target Clippy for Session and CLI, worktree layout,
format and diff checks pass. Layout first rejected test-only helper placement
and a configuration test file crossing its limit; helpers now reside in
external test support and transport configuration controls have their own
module. The failed layout log is retained without a debt-ledger adjustment.

## Combined file WM and output-role restart control

The test-only follow-up uses the actual initial, automatic and control restart
entrypoints with the same protected child holding the WM file stream and the
existing output IPC stream. `native_scanout=true` is only a fixture bootstrap
selector: a deterministic topology, empty capability list and no startup
candidate are supplied to `from_started_public_config`. No native target or
device is opened, and no frame, presentation receipt or retirement is produced.

The child completes the existing scripted file profile exchange, negotiates
output IPC, and checks the exact supplied topology. A third fixture witness
socket in the existing checkpoint directory corroborates its identity: parent
`SO_PEERCRED` must match the supervisor's peer PID and launch evidence. This
witness adds no role authority; the actual role endpoints enforce their own
credentials. The parent services `poll_output_authority`, so connection and
assignee events go through the existing owner rather than a test reducer.
Across both replacements the control requires distinct actual child PIDs,
WM epochs 1/2/3, increasing Qids and output epochs, and unchanged topology.

This snapshot-only join does not establish the exact pause-before-spawn order.
The existing service has a synchronous private pause acknowledgement, but no
snapshot-only public observation distinguishes a missing pause from a later
successful PID replacement. The earlier standalone pause primitive control
remains separate evidence. Outstanding candidate abandonment, topology apply
and rollback are not part of this fixture.

The compiled negative removes only automatic restart's `ReplaceSupervisedPid`
command. The real service stays paused, the replacement child's output read
hits its four-second bound, and the parent refuses after its five-second
dual-role handshake bound. Restored source is rerun; the negative establishes
the PID-replacement/resume obligation, not pause ordering. Evidence, including
the changed source hash and failure log, is in `.artifacts/t249-combined-output`.
The restored combined control passes 1/0 with its separately invoked child
entry ignored, and retained selection controls pass 3/0 with their child entry
ignored. Strict native-session all-target Session Clippy, layout, format and
diff checks pass. Production files remain byte-identical to the parent
selection checkpoint; only external test support and this note change.

## Protected normal Hagia startup (Phase A)

The next test-only checkpoint launches the frozen normal Hagia executable from
source `7455c3edd713770ed43630d0989073d2f14ba623`, not the scripted socket peer.
The required path is
`/home/niltempus/dev/hagia-overview-fix/.artifacts/h006-endpoint/hagia-7455c3e`,
SHA256 `0419e09e224676c4d925438f80b22df9532c1653ec339507637edbe01ea52f5f`.
The explicitly ignored fixture requires that path, hash and a fresh evidence
parent through `SOPHIA_HAGIA_FILE_BIN`, `SOPHIA_HAGIA_FILE_SHA256` and
`SOPHIA_HAGIA_FILE_EVIDENCE`. Selecting the test without inputs fails, rather
than reporting a skipped prerequisite as acceptance.

The fixture uses the protected production factory and actual staged profile
activation. `poll_public_request` receives real configuration, while
`settle_desktop_reload` performs the existing idle-input catalog publication.
Both configured and transport-ready must become true within the bounded wait;
the fixture does not set these fields or send scripted profile completions.
It verifies the independently retained profile key, epoch 1, catalog generation
1, exact explicit environment set and required profile flag. The native mode
is false, output bootstrap is absent, no overview actions are admitted, and
selected capabilities remain within the immutable supplied ceiling across
subsequent polls. No Cycle is issued and no layout checkpoint is written.

The first run passed 1/0 in 0.46 seconds, admitting 175 catalog actions and
selected capabilities 253951 under ceiling 262143. Its supervisor/peer PIDs
were 299/300 within the isolated test environment. That identity record is
the actual supervisor's protected-launch evidence, not an independent namespace
inventory. Binary path hashes before and after match; the hash-then-exec
window remains, so this is not descriptor-pinned execution.

Existing worker Stop and supervisor request/poll termination own cleanup. The
successful run reaped the child in the measured five milliseconds; Hagia
reported connection reset after fixture shutdown. This is not a claim of exit
code zero or a hard total reap bound under a stuck kernel. The fixture's
normal-path wait is bounded, while the existing supervisor Drop limitation
remains unchanged. No production repair was needed.

Evidence is in `.artifacts/t249-hagia-startup`, including the selected
missing-input refusal, source/binary identities and startup output. Strict
native-session all-target Session Clippy, layout, format and diff checks pass.
This proves real protected Hagia admission/configuration/catalog, not Session
layout settlement, surface-transaction prepare/apply, native completion, or
presentation receipts. Phase B remains separate.

## Phase B: real Hagia layout settlement and failed-action continuation

The separate `session/t249-hagia-layout-settlement` checkpoint reuses Phase A's
frozen normal Hagia, protected factory, profile and catalog owners. Both new
cases pass unmodified Hagia proposals through `PersistentLiveLayout::stage`.
Each asserts a nonempty resize obligation and a pending result. Supplied initial
facts comprise an already managed surface, safe retained CPU image and route;
the fixture does not demonstrate their original admission or native retirement.

The committed path services actual emitted frontend controls through the
Session queue with supplied correlated Delivered acknowledgements, then calls
the existing layout acknowledgement method. These are simulated X-authority
ACKs, not an actual X client or native presentation receipts. ACKs alone leave
readiness false and the retained layer unchanged. Matching supplied CPU facts
then make readiness true, followed by production prepare, resolve and apply.
The reducer successor and real Hagia checkpoint advance. A fresh proposal with
increasing request and domain transaction identities comes from the same child
and epoch. That proposal proves continuation, not a second committed layout;
cleanup may terminate its outstanding turn.

The timeout case first commits a baseline and boundedly waits for Hagia's fresh
atomic checkpoint, rather than treating Session Ready as peer consumption. It
pins bytes and device/inode from the same opened file. The scene admits operation
slot 1 before the fixture selects its real catalog action. The action is queued,
then canonical work-area facts advance through the existing owner before
dispatch, causing Hagia itself to compute a different size. The owner's relayout
is queued behind the action; no queue or proposal is rewritten. Exact frontend
ACKs are supplied but resized pixels are withheld. The pending layout remains
unready and `public.prepared` remains absent: timeout does not call prepare.
`force_pending_timeout` advances the deadline to now, followed by real expire
and apply. This exercises expiry logic, not a measured 250 ms timeout. Retained
layout and committed reducer state stay unchanged, with no session operation or
physical action. A later correlated proposal from the same child proves failed
outcome consumption; checkpoint bytes and device/inode remain unchanged after
that proposal. The next cause may be existing timeout rearm or work-area work.

The first two runs are retained fixture failures, not production defects:
initial retained admission was absent, so CPU observations were correctly
excluded, and initial SetPresentationState acknowledgements had not been
supplied. The corrected run and exact restored run each pass all three A/B
controls (3/0; restored 1.39 seconds). The narrow compiled P1 negative removes
only the Committed conjunction at semantic command construction. The real
Hagia reports “Sophia's session-operation expectation disagrees with the
projection,” and the next-proposal loop fails its same-child admission guard
(exit 101, 0.29 seconds). This is the intended guard failure, not a setup timeout.
The production `commit.rs` blob is restored byte-exact with its SHA256 verified.

Logs, per-case identities, both fixture reds, negative source hash and restored
proof are retained in `.artifacts/t249-hagia-layout`. Strict native-session
all-target Session Clippy passes. These controls prove Session layout/reducer
settlement through real Hagia's file loop. They do not execute HeadlessEngine
surface prepare/apply, output-role bootstrap, native completion or presentation
receipts. Existing stuck-kernel cleanup and path-hash execution limits remain.

### F1 correction: coherent historical admission and managed rollback

Independent review found that checkpoint `7236b478` called the Engine admission
table's `mark_managed` without completing Session's mirrored transition. Its
planning/unmanaged sets and layout-epoch admission state were inconsistent.
The retained earlier logs establish real Hagia outcome/continuation and the P1
negative, but their timeout took admission fencing; they do not establish
ordinary managed resize rollback. Source and the original durable bundle remain
preserved, with that limit superseding the broader wording above.

The separate test-only correction supplies a historical candidate (transaction
700, surface 3/1, CPU buffer 700), arms retirement through the existing admission
owner and calls production `complete_admission_retirement`. It does not claim
the original native completion was observed. The fixture now asserts Managed
in both admission tables, absence from planning/unmanaged sets, and absence of
pending target/recovery extent before each case. After managed expiry it asserts
the same invariants, an outstanding rollback and exactly one actual queued
ConfigureSurface routed to the retained surface/client and old geometry. No
pending flags or returned proposal are edited to obtain these results.

Q1 remains explicit: the rollback ConfigureSurface is issued but not answered.
The fixture asserts `rollback_pending=true` at expiry and supplies neither a
rollback ACK nor rollback pixels before the next Hagia proposal. Continuation
therefore occurs with rollback still outstanding; this is not completed rollback
or application recovery. The first proposal's cause is unspecified: the supplied
historical transition may itself queue relayout. Its actual resize obligation,
pending stage and correlated settlement are asserted independently of that cause.
The signed correction is `7f7bd745`; this qualification changes no tested source.

The corrected A/B controls pass 3/0. The P1 compiled negative and restored
controls are repeated against this coherent baseline. Evidence remains separate
under `.artifacts/t249-hagia-f1`; the initial fixture failures remain in the
earlier bundle. A direct `rustfmt --check --edition 2024` on the two touched
support files failed, despite the earlier workspace format pass: cargo fmt did
not visit those included test mounts. The direct failure is retained; only
`policy_hagia_layout.rs` and `policy_hagia_session.rs` were directly formatted,
and the direct check now passes. No existing production include was formatted.

### Current IPC and files: exact settlement parity

The test-only `session/t249-hagia-wire-parity` follow-up parameterizes the
protected normal-Hagia fixture by the explicit transport selection. The original
file-only entry remains a compatible wrapper. Each case checks the exact selected
socket environment, absence of its counterpart and output socket, binary hashes,
protected launch evidence, profile admission, catalog and bounded cleanup.

The same committed held resize and failed session-action timeout run through
both production transport factories with the frozen normal `7455c3e` executable.
The test compares selected capabilities, the complete accepted configuration,
typed proposals and their settlement identities, actual layout outcomes, retained
layers, rollback command shape and pending state, and checkpoint bytes. Both
cases start at epoch one without checkpoint restoration, so no identity or byte
normalization is applied. Future restore cases must account for the intentional
legacy Dirty transaction allocation; the file Dirty has no domain transaction.
Proposal causes are recorded for triage but are not included in equality.

The new comparison and retained A/B controls pass 4/0 (3.16 seconds), including
four fresh parity children. Both transports produce checkpoint SHA256
`91f3b477708f3e7bdc6749e9d05fb1205c55c16fddfc7b4e6d21136febb7fe8f`.
Strict native-session all-target Session Clippy, a newly compiled worktree xtask
layout check, direct formatting of both support files, and diff checks pass.
The first launcher invocation failed before compilation because an archived
isolation script lacked execute permission; invoking it explicitly through Bash
resolved that harness issue. Its failure log is retained with both green runs in
`.artifacts/t249-parity`.

This is actual Session layout settlement parity for two workloads. Historical
admission, CPU observations and frontend ACKs remain supplied. The managed
rollback is issued and remains unanswered during continuation. There is no
HeadlessEngine surface apply, full WM behavior matrix, performance comparison,
native retirement or output-role parity claim.

## Plain CPU production join after Session settlement

The separate `session/t249-hagia-cpu-join` fixture reuses the corrected baseline
and frozen normal Hagia, then passes actual CPU bytes and the actual Session
owner update into the existing backend CPU production cycle. Initial authority
transaction 700 supplies buffer 700 at previous surface generation 0; transaction
701 supplies resized buffer 701 at previous surface generation 1. Engine surface
generations become 1 and 2. Buffer content versions are independently 11 and 23;
they are not derived from Engine counters. The supplied Session baseline is
explicitly compared with the backend baseline established by the first real CPU
cycle (geometry, source and surface generation). This correspondence is not proof
of the original application admission.

Owner order follows `owner_loop/authority.rs`: `layout.projected_batch`, then
`presentation.rs::production_authority_batch`; `authority_production.rs` applies
the actual layout epoch result and calls `run_cpu_production_cycle`. The fixture
calls these existing methods in that order, rather than executing the whole
owner-loop macros. The backend reaches `ProductionSessionDriver::run_cycle`,
authority intake commit, and Engine surface prepare/apply. Source content,
readiness and previous generations remain unchanged through projection; only
geometry is projected by its production owner. No test compensation repairs a
refused generation or manufactures a WM result.

Pending retention is narrowly an empty CPU cycle with no competing content.
Admission-group CPU handles feed residency; layout epoch commit/abort calls own
the Present scheduler, not arbitrary CPU batches. These fixtures create no
Present and assert no staged/rejected Present count. The successful case observes
real supplied resized bytes, reaches real readiness/prepare/resolve/apply in the
same turn, then feeds that owner update and converted batch into CPU production.
Backend and output committed geometry, surface generation and source are checked,
as are raw registry bytes and their independent content version. Registry bytes
alone are source-selection evidence. Composition is required for the successor
pixel assertion: a bounded 16x16 interior sample (256 pixels) matches XRGB8888
RGB using the renderer's B,G,R,X byte interpretation. Old-only pixels are checked
where nonempty; the retained run records zero because the target covers the old
rectangle. No exposed region is invented.

The timeout case withholds new pixels, consumes the actual timeout update through
epoch abort and CPU production, and compares complete retained composed-frame
bytes/format/size/stride plus committed geometry/source/generation before and
after. It does not assert that an overlapping failed target rectangle is empty.
Actual WM update transaction/request identities are retained. Same-child next
proposal and unchanged checkpoint checks remain; rollback is issued but unanswered.
There is no cross-owner atomic rollback claim: Session settles its WM result
before this backend production turn. Native retirement, receipts, GPU execution,
actual X-client ACKs and original historical admission remain outside this proof.

The first compile failure used a nonexistent fixture `XResourceId::from_raw`
constructor; it is retained and corrected to `new(3, 1)`. The first runnable and
final owner-order-aligned runs pass 6/0, including two CPU cases, retained A/B and
the two-workload IPC/files parity case. Final duration is 4.33 seconds. Evidence
is separate under `.artifacts/t249-hagia-cpu`; no production or decoder change
was needed. Strict native-session all-target Session Clippy, layout, direct
rustfmt of the included support files and diff checks pass.

## Canonical behavior corpus parity across both transports

Signed `be449f389be6447c4f4eb292fcf27b4b92b36486`, joined as
`5b6a4ad00f360d93c56387bc89335e6b9eb9af8c`, adds a test-only eleven-scenario
comparison through the configured protected worker and frozen normal Hagia
`7455c3e`. The fixture takes the worker before driving it, so the shared public
Session state no longer polls it. A test-owned canonical reducer supplies the
conformance host's decisions. No production owner or API changed.

The pair passes 1/0 in 1.02 seconds: full decoded proposals and outcomes match
exactly, selection remains `0x3dfff`, and both observation digests are
`7ee814817923d62c8c6c0f53b9c7753b927fca9b50ca80f9889b8eb316fe3653`.
Each subsequent proposal proves continuation after the preceding outcome.
ReadyForCycle alone establishes driver send completion, not Hagia's consumption
of the final outcome. This is behavior parity, not checkpoint or layout/native
settlement parity. The invalid-discard case judges a deliberately invalid copy
inside the test reducer, as the original host does; Hagia's wire proposal is
unchanged.

The retained first run failed on current IPC: it included configured session
actions in canonical snapshots without the corresponding operation slots.
The corrected fixture uses the host-exact empty snapshot action catalog and
asserts empty canonical operation catalogs; configuration admission stays real.
Strict native Session all-target Clippy, layout and direct support formatting
pass. The five retained opt-in tests were listed, not rerun. The first layout
command incorrectly passed `-j 2` to xtask; the corrected invocation sets
`CARGO_BUILD_JOBS=2` and passes.

The independently verified durable bundle is
`~/.local/state/sophia/development-evidence/t249-hagia-corpus-be449f38`,
manifest `8e34f49956c17d9c0553a26f287c0045607a4ff88938d6f571c8179b2258b44f`.
The author additionally corrected P1 layout provenance: its valid exit 0 came
from the fallback command at the wrapper's eight-job setting, above the agreed
two-job limit. No fresh P1 run is claimed by this disclosure; its original
evidence is unchanged.

## Normal Hagia committed operation: accepted intent, no executor

The test-only `policy_hagia_operation.rs` child uses the same frozen normal
Hagia `7455c3e` (SHA256
`0419e09e224676c4d925438f80b22df9532c1653ec339507637edbe01ea52f5f`)
and protected file-WM fixture. It establishes the corrected managed historical
baseline and commits a real held resize with supplied CPU facts and correlated
frontend ACKs. These remain supplied inputs, not original application admission
or actual X-client/native completion evidence.

The admitted slot-one action then produces an unchanged projection through
Hagia's real loop. Its actual immediate `stage` result reaches
`apply_commit_result`; staged projection custody is consumed, no operation is
pending yet, and the expected slot remains one. The driver subsequently delivers
the real operation to the existing public owner, which validates epoch, token,
target and slot before returning its unchanged operation proposal. That proposal
also yields an immediate real layout result. Applying it returns the exact
`LaunchApplication { application: SessionApplicationId(1) }`, target `None`,
transaction tuple and sends the existing accepted-intent outcome. The tuple stays
local: the fixture never calls the committed-action queue, executor or launcher.
Application execution and success remain unproven.

The restored run distinguishes activation serial 5, projection request ID 2,
projection domain transaction 3 and operation domain transaction 4. The operation
request ID equals the activation serial. Numeric inequality between activation
and projection request is a fixture discrimination assertion, not a production
identity rule. An atomic checkpoint replacement is observed before operation
settlement. After the actual accepted-intent outcome, the same protected child
answers request 3 with domain transaction 5; checkpoint bytes and inode identity
stay unchanged through that operation. This continuation is a fresh proposal,
not a second committed layout.

The first run is retained under `.artifacts/t249-hagia-operation`: seven controls
passed and the new case failed an incorrect `physical_action.is_none()` fixture
assertion after projection commit. `wm/commit.rs` intentionally returns
`Some(action)` for this committed, non-scripted Action; the production
`owner_loop/session_control.rs` logs that diagnostic marker, separately from
queueing `session_action`. The corrected assertion checks the exact marker.
The operation branch still requires `physical_action == None`. No production
change was needed. The first run interleaved eight cases; the restored run uses
`--test-threads=1` and passes eight controls in 5.97 seconds, including retained
A/B, CPU, layout parity and the joined behavior corpus.

Strict native-session all-target Session Clippy passes. Layout passes with an
xtask freshly compiled from this worktree in a dedicated disk target beneath
the exclusive t027 cache (jobs 2, nice 19, device-hidden). Direct support-file
rustfmt and diff checks pass. No compiled mutation was requested for this slice.

This fixture omits `preflight_staged_presentation`: presentation capabilities
are absent and there is no publication, for which the production preflight
returns true. It does not exercise presentation preflight or retirement. Other
limits remain: supervisor protection evidence is not namespace inventory;
path hashes before/after are not descriptor-pinned execution; terminal peer
reset during cleanup is not a clean Hagia exit claim; cleanup does not establish
an absolute stuck-kernel reap bound. There is no physical input, native receipt,
whole-owner-loop or application-execution claim.

## WM-neutral test ownership, September 26

At niltempus's correction, the new real-Hagia Session fixture family moves to
Hagia's optional `tools/sophia_pairing` source overlay. Its action vocabulary,
camera/checkpoint assertions, frozen binary pin and supplied historical facts
are Hagia-owned. Copies were hashed before removing Sophia's mounts and files;
the earlier signed checkpoints and evidence remain intact. Sophia's generic
P1, driver, protocol, replay and owner controls remain. The optional runner must
identify the exact Sophia base plus its hashed overlay, and reject incompatible
private test context. It does not add a public Session API. Older interoperability
fixtures used by Hagia's existing gate are a separate coordinated relocation;
this checkpoint does not claim their removal.

The allocator observer moved too: it had no generic caller after unmounting
Hagia recovery. It will be injected only by the optional overlay. The backend
mirror target is instead generic: existing mirrored tests and a feature-gated
cross-crate fixture share the production install/custody/completion owners with
simulated device/copy/flip inputs. Normal dependency features exclude
`test-support`; no Hagia policy enters that target.

Focused validation: 32 mirrored-target checks and 51 worker checks pass; three
worker fixtures remain explicitly ignored. Strict backend `test-support`
all-target and native Session all-target Clippy pass, normal backend compilation
passes, and fresh xtask layout and formatting pass. Logs are under
`sophia-overview/.artifacts/t249-{presentation,neutrality}`. Retained initial
failures cover missing test-support reexports, an unused observer after
relocation and formatting. A preliminary feature-omitting zero-test run is not
evidence. The new Hagia presentation join remains uncompiled at this checkpoint;
source review alone is not pairing evidence.

## External pairing and the remaining legacy fixtures

Hagia checkpoint `2294385dfb194187f45d5233311d2fe120e8686a` owns the optional
Rust pairing runner and the external fixture family. Its restored `run-4`
passes twelve individually listed owner cases, protocol checks, strict native
Session, fresh xtask layout and direct fixture formatting. The test executable
is selected once, hashed, and invoked directly for every case. Source evidence
names Sophia `0319356db` plus a recorded overlay, never an unmodified checkout.
Hagia's `aoivl2yn` note and durable `h006-pairing-2294385` bundle retain earlier
strict/format failures and the exact source and executable identities.

This includes the first real profile replacement/rejection pair: the frozen
normal executable accepts view-count 8, rejects 10, and existing Session reload
owners restore the prior immutable profile at a fresh epoch. The presentation
case derives receipts from production retired-frame owners after simulated
device/copy/flip completion. It runs both wires through the same assertions,
without asserting full observation equality. Presented is a no-op in Hagia's
model; ordered delivery does not prove a receipt-dependent model change.

Signed `8bc5da5aa` and `6991101b4` complete the coordinated relocation of the
older real-Hagia fixtures. Runtime pointer-focus/presentation, protected profile
admission, Hagia launch-origin, targeted-click and partial-projection fixtures
now live in Hagia's optional legacy overlay. Generic X-origin and production
owner controls remain. No production API or authority behavior changed.

The neutral-tree listing removes exactly eight runtime cases and six Session
cases, with three name-only generic renames. The independent-target listing
supersedes an invalid first comparison that reused a stale shared-target binary.
Restored runtime tests pass 6/0 and native Session tests 667/0 with 21 ignored;
strict runtime/Session, layout and format checks pass. The first native Session
run had three reconnect failures (664/3/21); repeated focused and full runs did
not reproduce them. These remain intermittent and unresolved, not established
as pre-existing or disproved by reruns. Logs and the initial listing error are
retained in `sophia-legacy/.artifacts/neutral-validation`. Direct formatting debt
in the existing desktop reload mount has the same twelve hunks at base and head
and was left untouched. The external overlay's combined legacy acceptance is a
separate gate; source composition and neutral-tree tests do not establish it.

## Captured-record inspection

`fcc3ff91` (joined as `f9f54aa6e`) adds the WM-neutral `wm_file_inspect` protocol
example and external tests, without library, Session or decoder changes. It
validates one bounded Snapshot or a contiguous captured event window with the
public codecs before emitting text. Supplied epoch/capability values are
unauthenticated context; it does not replay phase, ACK, attach or infer capture
completeness. Submitted custody is labelled separately from reported outcomes.

Eight focused controls, strict protocol all-target Clippy, the example build,
fresh xtask layout and formatting pass. CLI runs on real codec-generated
Snapshot/events pass; a malformed suffix returns nonzero with empty stdout.
The first fixture run mismatched launch-origin epoch 2 and header 3 (five passed,
two failed); only the supplied fixture epoch was corrected. The eighth control
pins the absence of negotiation/phase inference. The durable
`t249-wm-inspect-fcc3ff91` bundle retains the red, corrected source, encoded
captures, runnable tool and checksums. This closes the captured-text tool slice,
not admitted live or mounted inspection.

## First attended opt-in WM file session

On 2026-09-26 niltempus authorized a separate trial and logged out of the daily
desktop. Production release Sophia `81e9826a6` and Hagia `1f24fbc` ran with
`--wm-transport=9p2000.L`, native scanout and the existing shell artifacts on
tty4/display `:90`. Running executable hashes matched the staged candidates;
Hagia received only the 9P WM endpoint plus the separate current-IPC output
endpoint. No acceptance overlay, installation or default-transport change was
involved. The actual policy-fragment and generated-argument preflights passed.

The operator reported normal behavior including overview. Normal logout ended
the 246,261 ms run with exit status 0, no emergency, `wm_degraded=false`, no
native frame in flight and no native cleanup pending. All trial processes were
collected. Greetd returned successfully through the existing safe-text-baseline
path after exact tty7 termios restoration differed; this was not an exact-state
restore pass. The diagnostic capture suppressed 42,456 records under its budget
and reported zero storage errors, so it cannot establish complete event coverage.

This establishes a first attended live smoke result. It does not complete the
physical behavior checklist, pointer-latency/default-switch gates or resolve
the earlier intermittent reconnect tests. Reported lower CPU use remains an
observation: the five-second Sophia sample was 22.6% of one core, without a
matched IPC workload. Detailed custody and evidence are in Hagia's `aoivl2yn`
note and the private 47-file `h006-live-9p-20260926` bundle, whose verified
`SHA256SUMS` digest is
`f32b1f39850b672c8bf5512ca335fd158706807d8297308307878c6c8917cc7e`.

## Connections

The accepted [9P interface decision](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
motivates the semantic seam while preserving admission and ownership boundaries.
The director owns task tracking and the subsequent WM file contract; this note
does not close t248 or authorize role integration.
