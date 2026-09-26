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

## Connections

The accepted [9P interface decision](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
motivates the semantic seam while preserving admission and ownership boundaries.
The director owns task tracking and the subsequent WM file contract; this note
does not close t248 or authorize role integration.
