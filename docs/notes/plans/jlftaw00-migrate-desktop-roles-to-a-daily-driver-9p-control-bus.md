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

## Implementation and qualification policy (2026-09-27)

niltempus approved completing implementation, client integration and recovery
before broad performance tuning, and using 9P as the experimental daily-driver
path with IPC retained for explicit rollback and benchmark comparison. The
refused t249 result stays recorded; t249/t250 qualification and t255 retirement
remain separate gates. No benchmark thresholds change through this decision.

The [desktop SDK decision](../decisions/gs6l7tuk-publish-native-c-and-rust-desktop-sdks-with-pinned-contracts.md)
promotes [t263's initial shell SDK release](9cd1ie0x-publish-independent-sophia-client-libraries.md)
ahead of t252 application adoption. Merge the reviewed t252 foundation first,
then extract the native C and Rust SDKs in parallel, complete their lifecycle
and file-role support, and move Bemenu/Lom/Provlita to pinned releases. New
releases include t261; its prior source fix was absent from the installed WM.

Codex directs tracking, contract review, integration and build slots, and owns
the C SDK, Bemenu and independent oracle. Claude owns Rust extraction, B6c and
Sophia's Rust dependency integration; separate agents adopt Lom and Provlita
after a tested SDK commit is pinned. Preserve Bemenu's unpushed local commits
and coordinate Provlita's Lom GPU-helper pin. Build at nice 19, jobs 2, with
private targets and explicit quiet windows for measurements.

Source integration: signed master merge `987cfd39` incorporates t252 at
`79a09a302`. Its production source is identical to that gated branch; only
master's corrected verdict text differs. The branch's six classified full-suite
failures remain failures, with their retained baseline evidence below.

t253 requires a real output-role product peer before implementation: the only
current client is the in-repository proof client. t254 retains its t249
dependency until qualification closes or a separate scope decision changes it.
Application frontend and broker/portal work remain outside this implementation.

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

**Rehearsal record (2026-09-26).** Candidate: personal release
`niltempus-81b687e08030f1d7b55e` (Sophia `06efcfda`, Hagia `5af36ac7`, Lom
`ad349869`, Bemenu `7d2d2399`, Narthex `50b9014d`), installed with the new
"Sophia niltempus Desktop (9P WM)" entry (sealed Hagia, `--wm-transport=9p2000.L`)
beside the unchanged current-IPC entry. `tools/rehearse_wm_9p.sh` on tty4, run by
the operator, passed 7/7 phases on both wires, session exit 0: startup ready,
`restart-wm` completed, reload unchanged, a Hagia-rejected profile (view-count 10)
rejected with rollback, the restored profile unchanged, restart after the
rollback completed, logout completed. Evidence:
`development-evidence/t250-rehearsal-niltempus-81b687e08030f1d7b55e-{9p2000.L-20260926T193548Z,current-ipc-20260926T193602Z}`.
The first attempt's post-rollback phases failed on the script's own readiness
race (fixed in `0cd03eab`) and are retained beside them. Rehearsals use a copy
of the release profile so the rejection can be swapped in; binaries are sealed.
**Attended session (2026-09-26).** The operator logged in through the installed
"Sophia niltempus Desktop (9P WM)" entry (session
`00000001790451452784-210136aa`, release commit `06efcfda`, Hagia over
`--wm-transport=9p2000.L`) and used it as the ordinary desktop, including this
development session. Operator report: layout, focus, move/resize and overview
behave normally; a Ctrl+Alt+Shift+R WM restart came back with the layout
preserved. The session log shows the WM ready at epoch 1, one requested restart
to epoch 2 with `preserved_layout=true`, and no degraded or failure records.
Single head per output as configured; multi-output acceptance is not claimed.

**Default selection (2026-09-26).** After the attended session the operator
chose 9P2000.L as the WM default ahead of the t249 release latency verdict. The
personal installer (chezmoi `2675ac0`) now makes the plain "Sophia niltempus
Desktop" entry run Hagia with `--wm-transport=9p2000.L`; "Sophia niltempus
Desktop (current IPC)" is the explicit rollback with the same Hagia, and install
removes the retired "(9P WM)" entry. First release with this default:
`niltempus-e587da65514ead64d34a` (Sophia `cb50f447`, Hagia `5af36ac7`, the
attended session's Hagia binary `464ae2fc`). Only the WM role moves; shell and
output stay on current IPC. The product default in
`WmTransportSelection` is unchanged until t255.

**t249 release verdict (2026-09-27 01:01): refused.** The unchanged
preregistered campaign ran 22:10-01:01 on a quiet machine with the operator
logged out (evidence `t249-release-overnight-48c387dd-r2/VERDICT.md`; an
earlier start aborted on a stale lock before measuring). `budgets_pass=false`,
40/40 pairs refused: all 40 on survivor equality (timing-dependent coalescing
happens on both wires, which makes exact cross-run equality fragile), 21 on
the one-interval p99 (all 20 at 120 Hz, where both wires run at p99 ~22 ms,
plus move 60 cpu pair 4), and 3 relative threshold breaches (one a 60 Hz
stall, two at 120 Hz). Current IPC also has 60 Hz p99 outliers (26.5 ms and
7.6 ms); 1.7-3.1 ms is the typical 60 Hz p99 on both wires. With different
survivor populations and stalls on both wires, the breaches do not establish
transport causality. Codex independently re-verified the hashes and survivor
sequences; the refusal stands (corrections appended to the evidence
VERDICT.md). The refusal stands for this
method; it is not rerun. On 2026-09-27 niltempus chose to retain the experimental
9P daily driver with IPC rollback and comparison. A new declared method must
retain the numeric budgets and 60/120 Hz coverage, report coalescing and stalls,
and distinguish transport costs from shared scheduling/storage delays.

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

**Content host selection (2026-09-27).** The single-shell owner now selects
9P explicitly with `--shell-transport=9p2000.L`; independent components retain
their existing profile selection. GPU content proof parameters require a wire,
and the content conformance host accepts a wire argument. All three use the
existing component file export, admission and content owners. The single-shell
facade's bounded negotiation and reconnect tests pass, as do 90 metadata-shell
tests, the configuration refusals, 12 proof-parameter tests and five proof CLI
tests. Focused all-target/all-feature clippy and layout pass. The content host
also completed an external, device-hidden Lom file exchange through protected
launch, ending with renderer failure and exact lease-backed release. This does
not claim GPU execution or native presentation. Descriptor-only file selection
is refused until that profile has a file contract; the independent descriptor
peer gaps remain open. Evidence is under development-evidence/final-9p.

The C SDK snapshot is now `8decca1d73699d6750c9228ecbf6e27f589d965d`,
including required-capability validation and nonblocking WM wakeups. Its exact
tree/contract check and the generic C SDK production WM export exchange pass.

**Amendment (2026-09-26).** The operator set the end state: all IPC code is
purged and every role runs on 9P files. t251's body rule (transaction ID plus
the unchanged `sophia_shell_v1` payload) is replaced by native file layouts
over wire-neutral typed records, wire-neutral budgets, and a client seamed at
typed values ([amendment 1 and the purge inventory](../../sophia-shell-files.md#amendment-1-2026-09-26-native-bodies-ipc-independent-records)).
It lands before the role families, client and oracle build on the old rule.

**Progress (2026-09-26, protocol/t252-shell-files).** Native records and
codec: typed shell records and value encodings out of IPC (4c46ec24,
f0d01d77, 4fb064a0), native whole `Candidate` (d2ec49db), self-contained KDL
pinned by a conformance test with normative cross-field rules (a25783ba,
f64d670e, bae4ec4a). Transport: shared journal/staging (f5f0dd43), candidates,
pacing and actions over files (59c056d5). Clients: Rust client seamed at
typed values (a753b343), pipelined write-capable 9P client with review-4
fixes (ab1c4243, a8b3f7f1), and the C binding's native 9P file client for the
base kinds by Codex (ba8c617b; `tools/check_shell_c_wire_files.sh`), proven
against the production export without Bemenu adoption or r7 records yet.
Later the same day: the Rust client's native file wire (cac21b94), the B5
protocol layer for launcher, dock and bar kinds (7f1b2f22), the attach epoch
disclosed in `api` (43e4530b) and read by both clients (ef0b9453, 7247f2d5),
revocation flushed so waiting reads answer ESTALE before close (ef0b9453),
and normative KDL corrections from Codex's audit (bae4ec4a, 6bb0c8f2). The
independent Go oracle by Codex (f516ba22; `tools/check_shell_files_oracle.sh`)
judges the production export with 54 named checks across eight exports and
shares no code with Sophia; it found the revocation gap. Its admission is
supplied, so it proves wire and owner interoperability, not supervisor
authentication, native rendering, latency or attended acceptance.
Role families, codec layer: normative rules for the launcher, dock and bar
kinds, traced against the owners (ee5e7f80, 19a21c84, ae60576f), and
independent codecs for all 17 B5 kinds in C (ef50d220) and in the Go oracle
(72eef656) by Codex, written from the KDL alone. The live oracle verdict is
still the 54 base checks; no role-family runtime pass is claimed yet.
**Desktop boundary cleanup (2026-09-27).** The external recipe and Quickshell
probe gates were preserved in niltempus before removing their Sophia copies.
Sophia's CLI now retains only generic launch controls, environment, host check
and exact-vector validation; the wrapper requires explicit arguments and a
prebuilt executable. Named recipes and source builds are gone from that path.
The focused launch/preflight tests, PTY host-refusal and recovery cases,
watchdog and lifecycle checks pass in the device-hidden sandbox; CLI,
conformance and xtask clippy pass. Evidence is under
`development-evidence/final-9p/{retained-launch,host-wrapper-reduced,watchdog-reduced,lifecycle-reduced,cleanup-clippy}.log`.
The product-specific X error tolerance moved with Quickshell. Sophia keeps the
Present-after-destroy and SHAPE wire regressions; no client/GPU claim follows
from those tests. The remaining gated S3–S5 desktop-tool closure is now removed, including the
comparison crate, installed stack and physical runners. Generic evidence readers,
fixtures and launch safety remain. The canonical offline wrapper no longer needs
sibling desktop checkouts; its 32 regressions pass. Niltempus owns the moved
coverage. The combined isolated gate passes in
`development-evidence/final-9p/combined-removal-run6.log`: 5,858 workspace
tests, zero failures, 62 ignored; SDK tests, clippy, layout and retained
verifier controls pass. Device-dependent pixel checks report unavailable,
and no promoted archive corpus was mounted. Earlier failed logs remain:
the sandbox needed private runtime/Go-cache paths; nested SDK and profile
checks needed to honor the selected Cargo target; the shutdown test helper
needed to preserve its requested phase after a budget yield. The helper's
three deterministic controls and the real shutdown case pass without
changing production scheduling or weakening the identity/history assertions.
E1's independent descriptor serve/bar peer remains an explicit gap. This
does not complete t252 or authorize a live change.

Role families, owner handoff (2026-09-27): the launcher, dock and bar
families run over the file wire through the existing owners (a52f93f7);
Session publishes component catalogs and indicators through the typed
transport (e50fc08a, bdf3f12c); a whole candidate gets exactly one outcome
and large objects publish in order under the object cap (3856d101), both
found by Codex's live oracle. The independent Go oracle now passes exactly
96 checks (r6, r7, r8 added; 9583b499) and the C client runs native launcher
and dock sessions against the production export, including a 4096-row
catalog (e8f3c486). Catalog and indicator eligibility is scripted in the
oracle from the normative rules; Session launch and admission policy is not
claimed there.
Full offline gate on the branch with master merged (29c5f77e,
2026-09-27 02:55, device-hidden, jobs 2): 6036 passed, 64 ignored, 6 failed;
fmt, workspace clippy, layout, both C gates and the 96-check oracle pass.
The 6 are not from t252: 5 `sophia-cli` `client_launch_socket` tests trip
their 3-second watchdog only under full-workspace parallel load (3/3 alone),
and `gpu_proof_domain` failed transiently on master and the branch alike
around 02:55-02:58 and passes since (7/7 on master, 3/3 on the branch; a
bisect on it was misled by the transient). The per-record credit rework is
deferred to the socket Limits removal (purge inventory).
Remaining: Bemenu r7 on the C client, B9
measurements under a new declared method (the t249 verdict showed survivor
equality and 120 Hz are unattainable as declared), merge and attended
evidence.

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

Slice 1 (2026-09-26) adds the `sophia_shell_fs_v1` codec
(`sophia_protocol::shell_files`) and a per-component file wire in
`ShellComponentTransport`: one 9P export per admitted epoch, negotiation as a
submitted record through the unchanged `select_negotiation`, the `limits`
object, the journal with its 64-record terminal reserve, per-role byte bounds
and 2000 ms acknowledgement deadline, and allocation requests through the
existing allocation owner. Current IPC remains the only selected transport:
Session selection, the `outputs` object, uploads, candidates, actions and the
catalog, indicator and launcher families come in later slices. Evidence is
runtime tests over a real private socket with supplied protection evidence,
not a protected child or an independent client. The shell export duplicates the
WM file owner's journal and staging algorithms; extracting both into one owner
is recorded debt.

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
