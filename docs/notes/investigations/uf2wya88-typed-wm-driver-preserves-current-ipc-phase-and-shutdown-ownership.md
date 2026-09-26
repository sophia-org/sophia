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

## Connections

The accepted [9P interface decision](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
motivates the semantic seam while preserving admission and ownership boundaries.
The director owns task tracking and the subsequent WM file contract; this note
does not close t248 or authorize role integration.
