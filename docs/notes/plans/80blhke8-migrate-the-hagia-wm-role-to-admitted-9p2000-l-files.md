---
id: 80blhke8
date: 2026-09-25
kind: plan
tags: [plan, milestone]
---
# Migrate the Hagia WM role to admitted 9P2000.L files

## Scope and exit

On 2026-09-25 niltempus approved Hagia's WM role as the first end-to-end 9P
milestone, chose compact binary runtime records with derived text inspection,
and instructed implementation. This supersedes the implementation pause in
the earlier architectural discussion; it does not authorize installation,
reload, device access or attended acceptance. The accepted direction remains
[ADR 1uoozfl8](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md).

Deliver an opt-in independent Hagia connection over direct Unix-socket
9P2000.L. Its WM startup/profile handoff, configuration/catalog, snapshots,
proposals, outcomes, session-operation intents and presentation receipts must
reach the existing owners. The separately admitted output role stays on current
IPC, including its bootstrap and restart barriers. X11, shells and application
protocols do not migrate in this milestone. Current IPC remains the default.

The shared core has no Sophia domain types. Session-issued admission remains
required; a pathname, claimed UID, attach name, fid or qid grants no authority.
LivePublicPolicyState remains the sole policy/reducer/presentation/restart owner.
The new path never sends existing IPC frames through a filesystem proxy.

The first development exit is signed paired candidates, independent protocol
and full WM lifecycle evidence, reproducible old/new transport measurements,
and isolated integration gates. Physical and default-switch acceptance are
separate. Task state and order live only in the queues.

## Task details

### t247

Claude (w9:pF) owns the bounded shared `.L` codec and per-connection driver in
`sophia-9p`, its static test export and independent conformance. The existing
`sophia-9p-authority` application scaffold stays inert. The core must enforce
exact version negotiation, framing/msize, tag/fid/queue bounds, open modes,
partial walks, cancellation ordering and teardown. It consumes an explicit
authorization hook on every operation, including retained handles.

First prove version/attach/walk/open/read/write/metadata/clunk/flush against a
pinned third-party Go client. A separately written negative prober covers
malformed framing, counts, strings, duplicate live tags/fids, invalid opens,
flush races, blocked reads, exhaustion and disconnect. Retain compiled negative
controls. This is direct socket evidence; no host mount or VM is part of t247.

### t248

Codex w9:pN owns the behavior-preserving private typed WM adapter boundary.
Keep the existing one-slot command/event queues, phase ordering, deadlines,
profile rejection before negotiation and shutdown behavior in the current
driver. Move current wire/transfer handling behind the current-IPC adapter.
Neutral profile identity carries epoch, generation, digest and exact correlation.
The existing constructors and output service remain compatible.

Prove current IPC through real sockets and profile controls, plus driver
transition/queue-shutdown controls through the private semantic adapter. No new
policy, rendering, application or public runtime authority is admitted here.

### t249

The director (w9:pP) owns the WM file/binary contract, protocol fixtures, paired
Hagia integration, queue and main-gate allocation. w9:pN implements the Session
WM export after t247/t248 interfaces and this contract are reviewed. Contract
definition and independent codec work may proceed while those prerequisites
are being built; joined production acceptance depends on both.

The file interface uses bounded per-attach staging and explicit submission,
generation-pinned snapshot reads and correlated ordered results. Partial writes
never affect the scene. Repeating a submitted transaction cannot repeat its
effect; result retention and acknowledged transaction retirement are bounded.
Cancellation never implies undo of committed work. Revocation and swallowed
release debt do not wait for 9P reply credit. New epochs refuse old handles and
actions. Share semantic record validation without routing through the old
socket, frame decoder or another reducer.

The [WM file contract](../../sophia-wm-files.md) records reviewed file semantics
and the binary envelope. Body layouts remain explicitly unfrozen until their
shared semantic codec owner and independent Nim corpus are ready. Review
already fixed bounded send failure, a single driver phase admission owner,
prefix-preserving cancellation and immutable opened-snapshot metadata.

Hagia h006 owns its independent Nim client and preserves reducer, projection,
checkpoint, action, layout and overview behavior. Full acceptance covers every
current WM capability, profile rollback, output changes, launch contexts,
source-only repaint, all-head receipt consensus, protected/application capture,
release debt and reconnect with reused numeric identities. Production
Session/backend joins are required; simulated native completion remains labelled.

Compare identical old/new workloads for latency distributions, CPU, allocation,
copied bytes, idle wakes and queue bounds. Do not invent a performance result
from encoding size. Default-switch/retirement thresholds require their own
review before changing defaults. Rollback explicitly selects one transport and
uses existing revoke/settle/restart barriers before a fresh owner is admitted.

### Working arrangement

All lanes preserve prior worktrees, branches and evidence. Signed checkpoints
are shared through herdr, heavy jobs are device-hidden and serial (nice 19,
jobs 2, exclusive disk targets), and only the director allocates main gates.
No source edits occur in main. Queue IDs are reserved centrally; other open
tasks are neither closed nor silently retargeted by this work.

## Development checkpoints

The development join includes t247's signed `b8ac5b91`/`ee2b7601` core and
independent Go oracle, t248's signed `45e61551`/`95b39662` adapter extraction,
and t249's signed `4406008f`/`d4d88621` neutral record conversion. The external
test mount correction `7567c745` is joined separately; its note retracts the
cached layout result as sufficient evidence. These are branch checkpoints,
not main integration or WM migration acceptance.

Hagia's independent client review correction is signed `7501b21`: 24 focused
controls, six compiled guard mutations and an actual Nim/static-Rust pairing
with five server mutations. Its note and durable h006 bundle retain exact
binaries. No old WM constructor selects it yet.

The binary envelope has nine focused controls, explicit object/event/candidate
class checking, bounded borrowed sections, and fixed submit/ack decoding. The
joined protocol suite passes 200 tests; strict protocol all-target Clippy,
formatting and a freshly built local xtask layout pass. Four compiled envelope
mutations fail class, ordering and reserved-field controls; the restored suite
passes. Evidence is under `sophia-overview/.artifacts/t249-envelope`.
The [complete-array checkpoint](../investigations/ir7e7zcv-complete-wm-file-arrays-share-neutral-validation-and-keep-submission-identities-separate.md)
adds typed Snapshot, Projection and Configuration bodies with shared neutral
validation, exact identities and selected capability refusal. Nine focused
controls and three compiled negative controls cover the file boundary; the
joined protocol suite passes 210 tests. Its scalar follow-up defines typed cycle,
dirty/session-operation, outcome and receipt bodies through the shared scalar
owner; ten focused controls and three compiled negative controls pass, with
232 joined protocol tests. Negotiation/profile bodies, independent Nim file
corpus and the complete Session/Hagia integration are still incomplete. Existing WM
semantics and output transport remain current IPC.

## Connections

- [Public-interface direction](../../sophia-9p-control-bus.md).
- [9P application frontend](../../sophia-9p-authority.md), a later role.
- [Current native protocol family](../../sophia-policy-ipc.md), still supported.
- [Namespaces and portals](../../namespaces-and-portals.md), unchanged authority.
- Hagia h006: `docs/notes/plans/i2c2blti-run-the-hagia-wm-role-over-an-independent-9p2000-l-client.md`
  in the separate Hagia repository.
