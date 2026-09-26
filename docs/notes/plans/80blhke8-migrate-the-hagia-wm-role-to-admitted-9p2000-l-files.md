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
and full WM lifecycle evidence, bounded read-only snapshot/event text inspection,
reproducible old/new transport measurements, and isolated integration gates.
The inspection command must preserve identities and distinguish Submitted
custody from semantic outcomes; malformed/truncated records and record limits
need controls. Captured-record inspection and admitted live inspection must be
labelled separately. Physical and default-switch acceptance are separate. Task
state and order live only in the queues.

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

The [WM file contract](../../sophia-wm-files.md) records reviewed file semantics,
the binary envelope and complete body layouts. Shared Rust semantic codecs and
the independent Nim corpus now cover those bodies. The export preserves
bounded send failure, a single driver phase admission owner, prefix-preserving
cancellation and immutable opened-snapshot metadata.

Hagia h006 owns its independent Nim client and preserves reducer, projection,
checkpoint, action, layout and overview behavior. Full acceptance covers every
current WM capability, profile rollback, output changes, launch contexts,
source-only repaint, all-head receipt consensus, protected/application capture,
release debt and reconnect with reused numeric identities. Production
Session/backend joins are required; simulated native completion remains labelled.

Compare identical old/new workloads for latency distributions, CPU, allocation,
copied bytes, idle wakes, round trips and queue bounds. Do not infer performance
from encoding size. Before proposing a default switch, the pointer-drag gate is:

- Measure from Session enqueueing each admitted move/resize update to its
  correlated layout settlement, using the same WM, scene, update sequence,
  coalescing policy and build mode for both transports. Record offered, admitted,
  coalesced and settled counts so dropping work cannot improve the result.
- Run at least five alternating pairs of at least 10,000 admitted updates each,
  at 60 Hz and 120 Hz, on the same machine under both idle and a recorded
  repeatable CPU load. Report p50/p95/p99, maximum and every timeout/disconnect.
- In every paired run, files may add at most 1 ms at p95 and 2 ms at p99 over
  current IPC. File p99 must also stay within one offered-update interval
  (16.67 ms at 60 Hz, 8.33 ms at 120 Hz). No timeout, disconnect or unbounded
  queue growth is accepted. These are acceptance budgets, not measured results.

Failure keeps current IPC the default; do not relax a budget after observing a
failure without a separate recorded decision. This control-path gate is not
input-to-photon evidence: attended pointer-drag and physical presentation
acceptance remain additional requirements. Rollback explicitly selects one
transport and uses existing revoke/settle/restart barriers before a fresh owner
is admitted.

Sophia's mandatory tests remain WM-neutral. Hagia policy assertions and real
Hagia lifecycle pairing belong to Hagia's optional `tools/sophia_pairing` gate.
Private Session joins use an isolated pinned Sophia revision plus a recorded,
hashed test-only source overlay; they never claim an unmodified Sophia checkout
or add a production test API. Generic protocol, admission, reducer, replay and
backend tests remain in Sophia. Pairing must list and run each required test,
refusing missing or zero-test runs.

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
232 joined protocol tests. These counts describe that historical scalar
checkpoint, not the current full acceptance result.

Later signed checkpoints added negotiation/profile bodies, independent Nim
codecs, the common Hagia policy loop, protected Session admission and explicit
production transport selection. Frozen normal Hagia has exercised real profile
and catalog admission, Session layout commit and managed timeout, the CPU
production join, accepted session-operation intent, and paired current-IPC/file
behavior and layout controls. Automatic and control restart recovery now
exercise real checkpoint restore and restore-triggered Dirty through both
transports. The [typed-driver investigation](../investigations/uf2wya88-typed-wm-driver-preserves-current-ipc-phase-and-shutdown-ownership.md)
retains exact source and evidence limits; recovery `e5105ff1` is joined as
`65c460ea3`, with its durable report under
`~/.local/state/sophia/development-evidence/t249-hagia-recovery-e5105ff1`.

On September 26 niltempus reaffirmed public role protocol replacement after a
separate investigation suggested administration-only 9P. That restriction does
not govern this approved work. Continue with real profile rejection/rollback,
remaining behavior and presentation-owner joins, reproducible measurements and
an exact-source acceptance runner. Current IPC remains the installed default;
output-role migration, physical acceptance and a default switch remain separate.

## Connections

- [Public-interface direction](../../sophia-9p-control-bus.md).
- [9P application frontend](../../sophia-9p-authority.md), a later role.
- [Current native protocol family](../../sophia-policy-ipc.md), still supported.
- [Namespaces and portals](../../namespaces-and-portals.md), unchanged authority.
- Hagia h006: `docs/notes/plans/i2c2blti-run-the-hagia-wm-role-over-an-independent-9p2000-l-client.md`
  in the separate Hagia repository.
