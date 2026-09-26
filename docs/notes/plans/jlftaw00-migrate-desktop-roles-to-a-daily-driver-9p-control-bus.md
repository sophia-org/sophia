---
id: jlftaw00
date: 2026-09-26
kind: plan
tags: [plan, milestone, security, shell]
---
# Migrate desktop roles to a daily-driver 9P control bus

## Scope and exit

On 2026-09-26 niltempus requested that the path from the working Hagia 9P WM
to a daily desktop control bus be documented and placed in the immediate queue.
This plan sequences the already accepted
[public-role replacement direction](../../sophia-9p-control-bus.md). It does not
change the role authorities or turn documentation work into permission to
install, reload, or take over a live session.

The desktop milestone is an explicitly selected, pinned configuration whose
WM, shell components, output role and administrative commands use 9P2000.L
through their existing semantic owners. Each role must have independent-client,
recovery, performance and applicable physical evidence, plus explicit rollback.
Default selection and retirement of the old protocol happen per accepted role.
The presence of a common codec does not accept another role automatically.

Portal interfaces and a 9P application frontend are subsequent contracts. X11
remains supported, and Engine's typed queues, rendering, device interfaces and
internal transactions remain internal. A desktop-role exit must name any public
interfaces still on old IPC; it cannot claim that every IPC path migrated.
Task state and execution order live in [todo.md](../../../todo.md).

## Starting evidence, September 26

| Area | Established boundary | Remaining acceptance |
| --- | --- | --- |
| Shared core and client | Bounded `.L` server, independent conformance, read-only client, enumeration and flush controls | Reconcile the signed evidence against t247/t248 exits; preserve their existing IDs |
| WM | Independent normal Hagia, production Session joins, checkpoint/restart/profile recovery, and an attended opt-in smoke including overview | Complete t249's evidence and measurements, then qualify the pinned daily configuration |
| Inspection | Separate default-disabled HostDomain export and Session/CLI integration at signed `4a2cb12e0ab49220ab9cbd5f82932b1dbaba0283` | It is read-only observation, not migration of administrative commands |
| Shell | Existing content/descriptor and independently admitted component owners | Define file semantics and migrate Sophia plus independent Lom, Bemenu and Provlita clients; retain Narthex compatibility |
| Output | Existing separately admitted output transport and restart barriers | Define and join its own 9P role; a working WM connection does not exercise it |
| Administration | Existing authorized command owners | Migrate command discovery/submission/outcomes while keeping control permission separate from inspection |

The [WM owner investigation](../investigations/uf2wya88-typed-wm-driver-preserves-current-ipc-phase-and-shutdown-ownership.md)
retains exact paired and attended candidates. The
[inspection investigation](../investigations/pp3pk4dd-read-only-wm-inspection-preserves-host-admission-and-writer-progress.md)
records focused/strict/layout success and the unresolved full-workspace
X-authority lifecycle test failure. Its original first assertion was lost in
the cleanup abort; source-only triage is not proof of an unrelated flake or a
passing baseline. Resolve or explicitly classify that gate before promoting
the complete candidate. Existing t194 owns the broader routing-test work.

## Security and transport rules for every stage

- Share 9P framing and bounded connection machinery. Keep role admission,
  disclosure, state and mutation with their existing owners. Do not tunnel old
  IPC envelopes or introduce a parallel reducer behind the file API.
- Bind an export to verified process/protection-domain evidence and explicit
  role grants. Paths, attach names, UIDs, fids and qids alone grant nothing.
  Preserve per-component and per-output scope; use neither a global all-role
  attach nor observer permission as authority to issue commands.
- Check held handles and pending operations after revocation/replacement.
  Fresh epochs fence stale identities, with bounded queues and no cleanup or
  input-release debt waiting for a slow reader's reply credit.
- Keep byte acceptance, submission custody, semantic commit, presentation and
  source retirement distinct. Flush cannot undo an executed operation. Shell
  storage stays charged until the real consumer releases it.
- Keep blind WM spatial facts separate from metadata-bearing shell grants.
  Preserve explicit direct-GPU permissions, presented-target input, focus
  leases and portal decisions. 9P supplies no new GPU or handle-transfer grant.
- Use explicit transport selection and rollback with one admitted owner for
  each grant. Shell bar, launcher and dock have independent writers; they do
  not inherit the WM's single-writer admission shape. No sniffing, silent
  fallback, duplicate exclusive provider or broadened
  grant. Direct Unix sockets are sufficient; mounting is not a prerequisite.
- Keep mandatory Sophia controls WM-neutral. Client policy/UI assertions stay
  in the owning repository or an optional exact-base pairing overlay. Record
  supplied facts, simulated completions and actual device evidence separately.

Every role exit includes the control-bus contract's five retirement criteria:
wire conformance (malformed/partial/cancel included), equivalent authority and
lifecycle with negative controls, independent clients, predeclared measured
performance, and an explicit compatibility/rollback path. All stages below use
direct sockets; mounted access needs its own unresolved credential/cache/handle
contract. A later shell or admin observer needs a separate disclosure contract;
it cannot add metadata to WM inspection by convenience.

Review the applicable admission findings under t133 for each endpoint, and the
typed topology epochs in t031 for output work. A transport-default change does
not silently decide the bwrap-policy question in t033. Lock/takeover authority
from t034 is outside this migration; any proposed expansion into that authority
must satisfy its own contract first. These references identify security gates,
not permission to implement every candidate task during a transport port.

## Task details

### Foundation and WM development: t247, t248, t249

The [Hagia-first plan](80blhke8-migrate-the-hagia-wm-role-to-admitted-9p2000-l-files.md)
continues to own these exits. Implemented foundations need evidence
reconciliation, not another implementation. Do not close them merely because
this successor plan exists. Finish remaining t249 joins and reproducible
transport measurements, preserving the exact native/simulated distinction.

### t250 — Qualify the WM daily configuration

After t249, assemble a pinned Sophia/Hagia candidate and explicit 9P launcher or
package configuration with a documented current-IPC rollback. Installed entries
must actually select the intended transport; an argument-free old launcher is
not a 9P deployment. Rollback is an explicit relaunch through the existing
revocation/restart owners. Keep shell and output transport identities explicit.
Reuse the existing package owner and
t101 checks; do not create another installer or silently change the login entry.

Run the Hagia-first plan's agreed pointer-drag latency gate and record all
admitted/coalesced/settled counts and failures. That plan owns the numeric
thresholds; this task cannot weaken them after a result. The abandoned ad hoc
CPU comparison is not a substitute for this bounded transport qualification.
Reuse t020's acceptance discipline and named t018 behavior where applicable;
transport acceptance does not close their remaining physical rows. Rehearse
startup/refusal, WM restart, profile rejection/rollback and shutdown
on exact candidates. Attended ordinary-use evidence must cover the selected
desktop's layout/focus, move/resize, overview and recovery without loss of
input or application state. Record topology limits rather than inferring
multi-output acceptance from a single-head trial.

Exit: reproducible candidate/launcher identities, classified build/test gates,
passed latency budgets and named attended evidence, with verified rollback.
This task owns the WM default-selection decision and its separately authorized
deployment/rollback, based on that record. It need not wait for shell migration.
Inspection remains a separate optional permission; it is not required for the
WM to operate.

### t251 — Specify the shell file contract

This is the immediate parallel planning lane while WM acceptance finishes.
Inventory the existing `sophia_shell_v1` content/descriptor paths, component
supervision, metadata delivery, reservations, focus/capture, actions, content
submission and receipts. Map every operation to its current owner and a
versioned file/record contract before implementation.

The [shell file contract](../../sophia-shell-files.md), accepted on 2026-09-26, maps the current
owners and revision skew, per-component uploads and revocation, and the Lom,
Bemenu, Provlita and Narthex acceptance scopes. Upload custody, separate scratch
accounting, role-filtered disclosure and per-component selection are specified.
Snapshot retention, journal and snapshot bounds, the acknowledgement-progress
deadline, per-role disclosure and the performance budgets are decided. The
operator accepted the contract on 2026-09-26, which closes this exit; the
guaranteed allocation-invalidation variant stays an open, non-blocking decision.

Define admission, negotiation, immutable reads, transaction assembly/submit,
outcome correlation, source lifetime, disconnect and stale-handle semantics.
Set numeric payload/queue/storage bounds and workload-specific latency/upload
budgets before measurement. Explicitly decide how content and any OS handles
cross the boundary; ordinary 9P bytes are not an implicit FD or zero-copy path.
Reuse the existing GPU permission and backing-accounting contracts.

Exit: an implementable role contract, operation/owner coverage matrix,
admission and revocation scenarios, performance budgets, and independent client
work scopes for Lom (bar), Bemenu (launcher), Provlita (dock) and the Narthex
descriptor reference. Cover Provlita's per-output reservations, pinned catalog
tiles, authorized launch context and cleanup without adding running-app
tracking, previews or other unrelated UI features. Reuse t039/t043's existing
contract and feed requirements rather than redefining their semantics.
Companion repositories allocate their own task IDs; Sophia does not invent
them here.

### t252 — Join and accept the shell path

After t251, implement a Sophia adapter to the existing shell/component owners
and independent clients. Development need not wait for t250's attended WM
qualification; the daily combined configuration must use its accepted WM
candidate. Start with bounded admission and a minimal
real content round trip, then cover Lom bar, Bemenu launcher and Provlita dock,
each independently admitted. Exercise the retained two-component baseline and
the three-component migration target separately. Provlita is an explicit client
in this path, not an implicit later extra. Preserve the descriptor/reference
route and explicit Narthex rollback. Do not add a desktop component just to
demonstrate the transport.

Require per-role capability parity, metadata non-disclosure, reservation and
focus behavior, exact presented-target input, replacement/revocation, stale
actions, slow-reader isolation, source retirement and reclaimed accounting.
Exercise all three components without one blocking another, including dock
replacement while bar/menu continue and retained dock content retirement after
revocation. Run CPU and native paths separately where applicable; receipt
fixtures do not prove frame
retirement. Compare equal workloads against current IPC using t251's budgets.

Exit: independently built paired clients, actual owner/lifetime joins, negative
controls, measured bounds and exact attended selected-desktop evidence. Reuse
t097/t100/t101/t081 and t104-t108 evidence for unchanged product owners; those
tasks retain their own remaining gaps and are not closed by a new wire. Keep a
role-to-evidence matrix so neither acceptance nor tests are duplicated blindly.
The September 25 two-component acceptance target remains historical evidence;
adding Provlita to this migration plan does not claim t108's three-component
physical exit has already passed.
This task owns shell default selection and its separately authorized rollout
after those gates, independently of the later output migration.

### t253 — Migrate the separate output role

After shell acceptance, specify its bounded bootstrap/topology/candidate/outcome
files and map them to the existing output authority. Implement an adapter and
an independent peer that actually consumes this role; Hagia WM success alone
does not identify such a peer.

Preserve the launch selector, protected assignment, supervised PID changes,
epoch replacement, outstanding-candidate cancellation and topology rollback.
Require old-connection closure, stale-handle refusal, unchanged topology on
replacement, candidate abandonment and release debt controls. Snapshot-only
restart checks remain distinct from topology mutation and native effects.

Exit: explicit 9P output selection, independent peer and production-owner
recovery evidence, predeclared performance bounds, and applicable exact native
topology acceptance. No transport result attests KMS or source retirement.
This task owns output default selection and its separately authorized rollout.

### t254 — Migrate administrative commands

After the WM development exit t249, this lane can run independently of shell
and output migration. Specify a separate
authorized command export for existing discovery, registered actions and
Session operations; retain existing validation and execution owners. Keep
HostDomain control admission independent from inspection and protected roles.
The read-only inspection tree/client must not gain command authority.

Preserve command identities, stale-epoch/action rejection, at-most-once effects,
bounded submit/outcome handling, cancellation semantics and restart recovery.
An accepted operation is not proof of a launched application's success. Port
the ordinary command CLI and use an independent protocol control without
granting arbitrary command execution or inventing new operations.

Exit: equivalent authorized command behavior over 9P, denial/replay/revocation
controls, bounded performance and explicit rollback. Runtime reload or command
surface expansion remains owned by its existing task, including t037; reuse
t036's safe command/restart smoke rather than scheduling an unrelated second one.
This task owns administrative default selection and its separately authorized
rollout after its own gates.

### t255 — Finish per-role default and compatibility retirement

Record the selected transport and exact accepted clients for every desktop
role. Tasks t250, t252, t253 and t254 own their respective default decisions;
this final reconciliation does not delay an earlier accepted role's switch.
Publish compatibility/version windows and a rollback recipe
before removing an old transport. Remove an adapter only when its callers and
independent reference path have migrated and retained tests cover the same
semantics.

Exit: a pinned daily desktop uses 9P for WM, shell, output and administration,
with no hidden old-wire dependency in those selected roles. Publish remaining
public interfaces explicitly. Scoped portal exports are the next design stage,
reusing t046's authority/lifecycle work; the application frontend and
application-owned exports remain separate milestones. This plan does not
promote mounts, a draw compatibility layer, or replacement of X11.

## Coordination

The director owns task IDs, normative contracts, integration and compile slots.
One lane can reconcile WM evidence while another maps the shell contract;
after the contract is fixed, server and independent-client work can proceed in
isolated lanes. Administration can proceed after the WM development exit;
output follows shell acceptance in the chosen queue sequence. These are work
allocations, not a new dependency between the runtime authorities.
Heavy checks remain device-hidden and serially allocated with private targets.
Every handoff names source, binary, evidence, first failures and remaining
limits. No lane writes another repository's queue or expands role authority by
renaming an existing grant.

## Connections

- [Accepted public-interface decision](../decisions/1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
  owns the destination and exclusions.
- [WM file contract](../../sophia-wm-files.md) and
  [inspection contract](../../sophia-wm-inspection.md) own the implemented APIs.
- [Modular component plan](ptil1ejw-modular-native-shell-components-and-independent-launcher-critical-path.md)
  and [Lom/Sophia native plan](1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md)
  own existing shell product and physical gates.
- [Content-shell contract](../../content-shell.md) and
  [native capability map](../../native-desktop-capabilities.md) are the starting
  shell inventory, rather than historical single-client checkpoint prose.
