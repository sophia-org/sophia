---
id: vup982br
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, session, shell, lifecycle]
---
# Retained panel pixels can block fresh component admission after disconnect

## Question and boundary

Does the production Session component service settle a disconnected content
grant against the actual backend without losing retained pixels, a neighbor,
or the previous work-area reservation? The source baseline is accepted Sophia
`9ee301e74ef22a065dcb112dc1873c7e458dfec4`. This is a test/fixture checkpoint for
[t100](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md#t100),
not a task closure or a production repair.

The primary fixture calls unchanged `component_service::service_components`.
It uses actual private Unix sockets, negotiation, resource uploads, candidate
submission and feedback, the component registry, Session services and the
backend's retained-content admission, owned queue and custody. The extracted
existing backend `Target` supplies copy/device/flip completion explicitly.
Protected-peer evidence names the fixture process: no protected child is
launched. The initial `PendingPresentation` setup is supplied at the concrete
native submission boundary, bypassing Session's initial `submit_bundle` call;
it is not evidence of that entire preparation path. Projection and subsequent
Session observation, disconnect and settlement are production code.

The doc-hidden bridge is enabled only by the backend `test-support` feature
from a Session dev-dependency. It constructs no native device and does not
expose `NativeCompositionTarget`. Normal CLI native-session release dependencies
exclude the feature. The separate legacy `LiveMetadataShell` compatibility
control exercises recovery while paused and its own deferred settlement owner.

## Established joins

The primary panel has a completed candidate, then one submitted replacement
whose retirement is deferred behind an independently owned neighboring layer.
A second submitted candidate for the same output is correctly refused by the
real store; the first fixture attempt hit this bound and was corrected without
changing production admission.

Disconnect cancels the old grant's exact backend retirement claim while keeping
its source bytes, copied backings and reservation. The current target can issue
a real wire Action before disconnect; the same target cannot issue afterward.
A live neighbor remains connected. An absent runtime retains the Session
revocation obligation; a later production service visit settles it without
another message. Repeated settlement does not repeat the obligation.

A supported three-role profile, with its dock unopened, provides actual budget
headroom for the positive replacement control. The new epoch reuses resource
and candidate number 1. An old completed frame cannot give it a receipt; an old
action cannot issue, while its actually completed current target can. A changed
reservation (two pixels instead of one) appears only after replacement
completion. Neighbor resource upload proceeds independently. Final transfer
drops the actual fixture backend, leaving exactly four bytes owned by one
independent old consumer; its release makes the real accounting quiescent.
Claim cancellation, source release and copied-backing custody are distinct
assertions. Backend-produced or discarded old terminal outcomes are not claimed
as delivered to a disconnected client.

## Reproduced two-role circular wait

The installed-style panel/menu profile does not have the positive control's
headroom. Its panel reserves 40 MiB (8 staging + 16 resident + 16 retiring),
and its menu reserves 24 MiB (4 + 12 + 8). Together they exhaust the unchanged
64 MiB aggregate limit.

The fully opened three-role profile also exhausts that limit (24 + 20 + 20 MiB).
The positive control's unopened dock is therefore a material limit, not evidence
that the fully opened three-role profile can reconnect.

The ignored red
`two_role_panel_reconnect_progresses_after_all_old_native_work_completes`
runs explicitly and fails. It drops the fixture's independent old source lease,
completes all old native work through the deterministic target, and drives four
production retry/collection visits. Each retains the original panel attempt.
The neighbor remains connected. Observed accounting stays at 24 MiB + 4 bytes
reserved, with one retired epoch and one disconnected submitted candidate.

The custody path is:

1. `ShellComponentSession::stop` closes the exact connection. The epoch registry
   revokes admission and moves non-quiescent stores into its retired pool.
2. `settle_revocations` cancels `retained_projection_retirements` for that grant;
   it deliberately does not drop `runtime.shell_content` image leases.
3. Collection releases revoked resources only after their final real consumer
   ends. Completing copies and retaining lease-free history does not remove the
   current shell-content source image.
4. `reserve_attempt` needs a fresh full role allowance: 24 MiB + 4 old bytes +
   40 MiB exceeds 64 MiB. Negotiation cannot start, so no replacement can present
   and replace the retained source.

The panel disconnect path invokes no component removal. The explicit removal
call belongs to native-launcher closing. Topology replacement, neighbor exit,
or final backend shutdown is not automatic panel reconnect progress. This is
a demonstrated retained-pixels/admission circular wait in the tested owner join,
not merely a test-owned consumer refusing to release. No quota or production
semantics have been changed to hide it.

## Repair choices for review, not implementation

Reserved reconnect headroom would partition admitted role allowances so a
bounded predecessor and its successor fit together. The registry still owns
the aggregate accounting; Session retains source and work-area ownership until
replacement presentation. A meaningful guarantee must budget the maximum
allowed retained predecessor, not just an arbitrary spare four bytes. This
reduces steady-state per-role capacity and needs a reviewed admission policy.
That budget must include every retained staging, resident, retiring and backing
category and all overlapping retired epochs; resident capacity alone is not a
bound on the full residual.

Alternatively, a fresh grant could negotiate validated limits from the remaining
aggregate budget. The new peer must honor its actual advertised limits, with a
bounded minimum useful grant and explicit refusal when it cannot fit. This
preserves existing peers' limits but does not guarantee progress under every
saturation case. It must not inherit an old grant, renegotiate a live connection,
borrow unaccounted credit, or mint a completion to collect old storage.
Every `ContentLimits` relation must still hold. Subtracting a residual from
individual limit fields heuristically is not an admission policy.

Both choices must preserve exact epoch rejection, independent neighbor service,
finite retained state, old reservations until actual replacement, and real source
retirement. Raising aggregate quotas or forcibly freeing retained bytes is not
an option. The director owns the next repair decision and gate allocation.

## Fresh-grant budget design on accepted 69e16358

Signed `4d8d7966` is the design/control checkpoint, without production changes. The
director's [bounded admission brief](../../../validation/specula/shell-reconnect-budget-modeling-brief.md)
defines the joint review. No production semantics, queues or live session have
changed. The prototype selector is confined to
`sophia-runtime/tests/shell_content_reconnect_budget.rs`; eventual acceptance
must call the production selector instead of retaining a parallel algorithm.

### Inventory and ownership

Active epochs reserve `S+R+T` source bytes and `R+T` backing credit.
Resource Begin charges staging, reserved resident and backing; reserved resident
overlaps the transfer's staging allocation and is not another source allocation.
End moves staging into resident without increasing backing. Explicit Retire
moves resident into retiring. On disconnect, `revoke` aborts every open transfer,
returning staging, reserved-resident and the corresponding backing credit before
the epoch enters the retired inventory. Remaining leased resources occupy both
resident and retiring classes: `P = resident + retiring`, equal to remaining
resource backing credit under the current implementation. Collection removes
only resources whose last real consumer has ended. No accounting update claims
that a submitted native frame completed.

Source holders include installed `(output, layer)` shell images, retained
projection frames, candidate references and queued/rendering native clones.
The successor uses the same Session-assigned layer; replacement releases old
source references per output, while independent consumers can retain them.
Old work-area and input transitions still depend on actual presentation, not on
successful negotiation. Native head targets and swapchain/copied buffers have
separate owners and bounds. Registry backing credit is one credit per resource,
not one per copy/head and not measured VRAM; this design does not charge those
head buffers to the 64 MiB content cap. Source lease release and copied backing
retirement remain separate assertions in the joined controls.

### Exact proposed selection

The runtime pure API is proposed as
`select_reconnect_limits(nominal: &ContentLimits, budget: ContentReconnectBudget)
-> Result<ContentLimits, ContentStoreError>`.
The registry supplies read-only current source/backing totals and same-profile
retired totals after real collection; the snapshot is not an independent ledger.
The Session connection owner chooses the nominal role profile, calls selection,
then calls the existing serialized reservation transaction without an intervening
peer operation. Transport publishes only the successfully reserved limits.

Keep `M = max_resource_bytes = 4 MiB`, all non-byte fields, grant identity and
`limits_generation=1` unchanged. The useful floor is `S=M, R=2M, T=M`: one full
staging resource, two resident resources and one retiring resource. For nominal
`S0/R0/T0`, `Q=S0+R0+T0`, own-profile retired source `P`, global source reservation
`U` and backing reservation `V`, use checked subtraction:

```text
A = floor4(min(Q-P, 64 MiB-U))
B = floor4(min(R0+T0, 64 MiB-V))
require A >= 4M and B >= 3M
R = floor4(min(R0, A-2M, B-M))
S = floor4(min(S0, A-R-M))
T = floor4(min(T0, A-R-S, B-R))
require S >= M, R >= 2M, T >= M
validate the complete ContentLimits; reserve using unchanged registry checks
```

`floor4` rounds down to a multiple of four bytes. Cold profiles remain identical.
Four retained bytes reduce the two-role bar from `8/16/16 MiB` to
`8 MiB / 16 MiB / (16 MiB-4)`. Source and backing checks remain independent even
though current resource accounting implies backing cannot exceed source.
Every malformed limit, arithmetic underflow, useful-floor failure or epoch
capacity refusal leaves the registry watermark and transport allowance unchanged.
Session's already-minted attempt numbers remain burned as before.

### Partition proof, repeated history and simultaneous reconnects

Within the component registry, `add` refuses duplicate roles. Bar, launcher and
dock map uniquely to Legacy, NativeLauncher and PersistentCatalog. Registration
closes at the first reservation attempt, so `has_dock` and the role set are
immutable. Nominal envelopes total at most 64 MiB: two-role `40+24`, dock profile
`24+20+20`, and smaller subsets. The component registry is privately owned by
`ShellComponentConnections`; its sole production admission path is
`reserve_attempt`. Separate legacy registries keep their existing behavior.

For each profile, the invariant is `active reservation + all retired P <= Q`.
Admission uses at most `Q-P`; revoke substitutes at most the old `R+T` for its
larger reservation; collection only decreases P. This induction includes every
failed tightened successor. Q always comes from nominal role limits, never the
last granted limits, so no additional allowance history is needed. An inactive
profile's global source headroom is at least its `Q-P`: another profile cannot
consume its envelope by reconnecting first. Both global checks remain defensive
guards. All reconnect orders preserve the invariant without holding a second
per-slot reservation. Three active epochs and sixteen total active/retired
epochs remain the existing metadata bounds, including zero-byte candidates.

### Refusal, diagnostics and client limits

The useful floor is not universal output capacity. Lom can upload a replacement
for one maximum-size resident image at `R=8 MiB`. Multiple outputs require
`sum(image bytes)+max(image bytes) <= R`; larger resident workloads can stop
updating. A retiring burst beyond T can hit Lom's five-second response timeout.
An image larger than M was already unsupported. Grants stay fixed for that
connection: no second Limits record, forced recycle or active-grant expansion.
Independent source review found that the live launcher sizes images from M and
its two-slot C upload owner returns Busy at staging/resident/retiring bounds,
retrying within existing deadlines. Executable behavior is not established by
that source review.

Refusal remains real when `Q-P < 16 MiB`: two-role bar P above 24 MiB, two-role
launcher above 8 MiB, dock-profile bar above 8 MiB and dock/launcher above 4 MiB.
Other global or epoch constraints also refuse. If only displayed predecessor
pixels hold this residual, it can remain blocked because only a successor could
replace them. No timeout frees that storage. Genuine independent consumer
release can restore capacity; these cases must have explicit positive controls.
Existing scheduling tries at most one slot per visit, rotates slots and backs
off from one second to sixty seconds. Each refusal ends that attempt; retries
bound the rate, not the number of attempts or time to recovery. No new scheduler
or collection-triggered wakeup is proposed.

The actual current error path is `ContentStoreError::Budget` through
`ShellTransportError::ContentStore` and `ComponentConnectionError::Transport`,
stringified by `ShellComponentProcesses::start` before reaching the service.
`Transport(ContentStore(Budget))` matches no `component_start_cause` branch,
therefore reduces to `cause=other`. The repair will add the exact safe
`content_budget` cause via `StartCause::ALL`, which the existing diagnostic
reducer already consults. Numeric evidence must be emitted at the reservation
owner before stringification, not parsed back out of free text.

Proposed records use the existing `sophia_shell_component schema=1` family:
`status=admission_refused cause=content_budget` or `status=admitted_reduced`,
with slot, role, exact fresh connection/content epochs, nominal/source/backing
capacity, own retired bytes/epochs, global available bytes/backing, required
floor bytes/backing and granted S/R/T for success. Add only those status/field
tokens to the reducer; bound byte values by 64 MiB, epoch counts by sixteen,
slot by two and identities by u64. Emit once per reservation outcome under the
existing retry rate, not each idle service visit. These are diagnostics, not
new wire records or mutable budget ownership.

The design executable has seven passing controls: nominal and full-validation
constraints (including protocol-valid but one-resource resident refusal),
unaligned headroom without rounding up, real revocation of staging with both
remaining source classes, both two-role orders, all six fully opened three-role
orders, sixteen repeated reduced retained epochs and atomic count refusal, and
28 MiB of actual pinned resources refusing until a real lease release lowers
the residual to 24 MiB. It uses unchanged registry admission but supplies
per-profile totals to a test-only selector; Session integration is not proven by
these cases. Focused execution and strict Clippy pass; logs are in
`sophia-borders/.artifacts/t100-budget/pure-final.log` and `pure-clippy.log`.

### Production repair checkpoint

The runtime now owns `select_reconnect_limits`, its shared
`content_reconnect_allowance` calculation and the measured registry projection.
The prototype selector was removed from the control executable. Checked
headroom that would underflow is represented as zero useful allowance, causing
floor refusal; it never wraps into credit. Both selection and diagnostics use
the same computed available/required values. Session invokes this only at
component reservation, then publishes through the unchanged transport owner.
Legacy registries and all cold role profiles retain their prior behavior.

`admission_refused` carries `budget_constraint=bytes`, `epochs`, or `reservation`:
an epoch reason requires the measured count to reach the existing capacity;
an otherwise unexplained reservation Budget is not labeled as epoch exhaustion.
The record includes active and total epoch capacities. One slot can occupy the
shared sixteen-slot inventory and block another slot even when byte envelopes
fit; the byte partition is not epoch fairness. The existing disconnect clears
transport and calls exact `epochs.disconnect(store_grant)` before fallible
endpoint release. An ignored endpoint-release error does not leave the old
registry grant active; no new compensation or lifecycle state was added.

The original two-role progress control is enabled and passes. The replacement
test now covers the full two-role profile and a fully negotiated three-role
profile, preserving old work-area bands until simulated replacement completion.
Repeated Session successors keep exact old leases, and a real 24 MiB plus
four-byte predecessor refuses until an independent four-MiB consumer releases.
These retain the original current-process peer evidence, supplied initial
pending-presentation and simulated native-completion limits. Eight focused
Session joins pass. A separate actual reservation test captures host output,
then checks its reduction: fifteen reduced grants, one count refusal, no idle
record, and actual source release reopening a slot with the refused attempt's
identity still burned. Literal-only reducer tests are supplemental.

Independent client controls are supplied by signed `aad0e262` and corrected
`3f839a38`. Their durable bundle is
`~/.local/state/sophia/development-evidence/t100-reduced-limits-3f839a38`;
its SHA256SUMS digest is
`9fbde2640df8584aabfb477c6aa8e2e23b8ccef0d9cc1f3fd602a187957bde9a`.
Four Rust cases exercise real generic transport/client welcome and resource
admission, and C decoder/upload cases exercise staging/resident/retiring pacing.
A refused Begin does emit status 3; the corrected resident-bound negative fails
in 0.08 seconds with status 3/reason 2. The earlier timeout explanation is
withdrawn, with the original log retained.

Lom `ad34986` pins protocol/client `2e569301`: the limits codec and validation/
layout code compared is identical, not the whole protocol or client crates;
catalog, candidate, action and visibility drift exists. Bemenu `7d2d2399`
vendors `c2ff3fcd`; forty of forty-two C vendor files match, with `fields.h` and
`shell_wire.h` differing in catalog vocabulary, not limits/upload behavior.
Those precise comparisons support the paired library controls. They are not
Lom/bemenu executable runs, physical acceptance or evidence for arbitrary
multi-output workloads.

## Evidence and remaining exits

Worktree logs are in `sophia-borders/.artifacts/t100/`. `joined-6.log` records
four passing controls and the explicitly ignored known red;
`two-role-progress-red.log` records its assertion failure and accounting.
`negative-controls.json` records exact original/mutated source hashes and exits.
Both compiled negatives fail at the production claim-cancellation assertion:
skipping the service settlement calls, and substituting the successor grant.
The native-release feature graph is retained separately.

The disk cache at `sophia-t027/.artifacts/t026-target` is allocated exclusively
to this lane during these checks. Its pathname does not identify the source:
the retained compiler paths name `sophia-borders`, and the executed test binary
is checksummed in the evidence. No concurrent writer used this target.
The new default feature combination exposed two existing libinput-dependent
runtime-tick tests without their feature guards. Their precise guards were
added without enabling libinput in the bridge; the failed build log is retained.
The workspace check similarly exposed an EGL-only allocator test under GBM
without EGL; only that test and its imports gained the existing EGL guard.

Final owner verification records 1,753 passing tests, zero failures and 40
ignored across the backend/session all-features suites. The focused join run
has four passes and one ignored known red; running that red explicitly still
fails the reconnect-progress assertion. Default and all-features test builds,
strict affected all-target Clippy, the GBM-without-EGL renderer check, workspace
all-target check, formatting, metadata and layout pass. `identities.json` records
the executed native-session binary hash and exclusive cache/source paths.
The first parallel fixture run exposed a timestamp-based temporary-path
collision; a process-local atomic counter now gives each fixture a unique path.
That failed log, the feature-build failures and the compiled negatives remain
alongside the passing reruns. None is counted as accepted reconnect progress.

### Integration gate: diagnostic child lifetime

The first default workspace gate on signed `1c780ccd` stopped at the CLI
`scanout_records_reach_capture_independently_of_console_logging` test. Its
capture child exited successfully and all seven expected records matched;
the subsequent `recording=stopped` assertion failed. The fixture removed its
store during unwinding, so its final health and storage-error counts cannot be
reconstructed. The original failure remains in
`sophia/.artifacts/t100-1c780ccd-qaeeapr4/workspace.log`.

`Capture::drop` intentionally waits at most 500 ms to preserve recovery latency;
it does not promise synchronization before returning. The child previously
exited immediately afterward, potentially ending the still-syncing worker.
The capture, storage and tracing-layer sources are unchanged and have no new
feature branch. Three independent direct-child pins of the original binary
passed in 0.14–0.17 seconds with seven records, two discarded records, zero
storage errors and stopped health. Their report is in the gate's
`diagnostic-child/report.json` (binary SHA256
`32826ff68b6a4dbd15a9a00a3fdfa35152ba4b6b8a1226ebfdffe65530ac05ea`).
These support an intermittent fixture-lifetime assumption; they do not recover
the original child's missing final state.

The fixture now keeps the child alive after drop and polls for the actual exact
stopped-health line for at most five seconds. Failure reports the last health
or read error, path, PID and elapsed time. The production 500 ms bound and APIs
are unchanged; the parent still checks exact records and health accounting.
The corrected exact CLI test passes (one test, 0.15 seconds); strict Clippy for
that integration target, formatting, diff and layout checks pass. Worktree logs
are `diagnostic-fixture.log`, `diagnostic-clippy.log` and
`diagnostic-layout.log` under `.artifacts/t100/`. These focused checks preceded
the fresh full default workspace gate recorded below.

## Joined checkpoint gate on 2026-09-25

The signed test checkpoint `1c780ccd245870d1fd79f792267799da7f9a5a60` and
fixture-only follow-up `d5dfbe71cedb62141aad446680adfafe35bdfa4b` were reviewed
for integration. On the exact latter candidate, the fresh main-tree default
workspace suite passed 4,481 tests with zero failures and 37 ignored. Formatting,
whitespace, metadata and layout checks also passed. The device-hidden wrapper
cleared inherited live-session variables, used private runtime and temporary
directories, and kept compilation at two jobs with reduced priority. The source
identity and clean tree matched before and after the gate.

The passing report and logs are checksummed in
`~/.local/state/sophia/development-evidence/t100-d5dfbe71-main`. The failed
first main gate and retained direct-child observations remain separately in
`~/.local/state/sophia/development-evidence/t100-1c780ccd-main-red`. The owning
worktree's broader native checks, compiled negative controls, feature graph and
rebase equality proof retain their own `t100-1c780ccd` bundle; the diagnostic
follow-up retains its `t100-d5dfbe71` bundle.

This integrates deterministic test coverage and the investigation, not a
reconnect repair or t100 closure. The explicit progress red remains unresolved.
No new cross-repository family or physical acceptance is claimed; production
behavior and the paired WM/shell wire are unchanged. No installation or live
reload accompanied the gate.

The evidence is headless development coverage. It does not establish physical
KMS/driver completion, protected launch, installed two-role recovery, successful
VT resume, all topology/scale/revocation transitions, a complete Session/WM/Lom
causal workload, real resource residency/latency, or attended normal exit. Those
remaining [t100 exits](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md#t100)
and the t069/t097 prerequisites remain open.
