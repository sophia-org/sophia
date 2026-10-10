---
id: jsschoen
date: 2026-10-09
kind: plan
tags: [plan, milestone, 9p, namespaces, portals]
---
# Converge public roles on one 9P core

## Scope and exit

This plan owns the implementation rows that follow from the
[accepted one-core decision](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md).
The milestone ends when every public role is an export on the `sophia-9p`
core, the legacy socket envelope has no production caller, the application
authority scaffold attaches through the same core, identity can be proven at
attach, each connection's tree is derived from its admission context by a
recipe, and one portal kind has been executed as a bind. Engine, the X
authority and the existing role file contracts are unchanged throughout.

Architecture approval and the dependency plan below do not promote every row
at once. Rows enter as parallel work where a driver and a measurable exit
exist and as candidates where they wait on a gate. Task state and execution
order live in [todo.md](../../../todo.md).
Each row's exit below is the acceptance claim; physical acceptance, where a
row needs it, is a separate claim with its own evidence. The measurement rule
of the [public-interface design](../../sophia-9p-control-bus.md#performance-is-an-acceptance-question)
applies to every retirement: nothing legacy is removed until its replacement
is measured against it for the same workload.

The existing rows this plan depends on keep their own notes. t249, t250 and
t252 qualify the live WM and shell roles. t254 migrates administration. t255
retires the legacy envelope and its default transport selections once the
rows here have landed. t133 reconciles the admission review with production.
t142 proves confined groups on separate listeners. t275 designs the recipe.
t256 audits portability and t257 adds status files.

One row here is critical-lane work by niltempus's decision of 2026-10-09: the
9P-side input injection contract, t320, because the application authority's
input contract, the driver tier and agent-driven acceptance all depend on it
and none can be designed around it later.

## First deliverable and dependency order

niltempus approved the reviewed direction on 2026-10-09 and requested the
critical path. The first usable deliverable is a generic observer CLI that
reads bounded status and captures one explicitly authorized output, including
accelerated content, with the output and presentation generation attached.
The observer cannot inject input. No native 9P application frontend, mounted
filesystem, recording stream or new desktop UI is required for this milestone.

| Step | Existing scope | Exit that unlocks the next step |
| --- | --- | --- |
| 1. Finish output publication | t310 | Same-topology owner replacement settles the realization; equal authority snapshots need no republication, changed capabilities advance the epoch and publish. Both paths have failing-without-fix CPU regressions. Reconcile the remaining workspace-affinity/policy exits in t310's plan; do not repeat accepted cable survival as a substitute. |
| 2. Establish admission and reach | t133, t317, t142; recipe design t275 | One immutable principal/namespace per connection; no forged attach, cross-identity second attach or inherited-fid authority change. Two confined groups cannot reach the trusted socket or one another; explicit CLIPBOARD and PRIMARY transfers retain their t142 controls. Launch custody is proven separately from authentication. |
| 3. Compose the observer and portal foundation | t318, t315; bounded status slice of t257 | Separate role endpoints and discovery, independent grants, a retained-fid revocation seam, and truthful available/waiting/recovering status. t315 requires the t323 readiness gate and preserves current X recipient execution. The foundation needs no future native-9P clipboard client. |
| 4. Add one output capture | t319 | Renderer-owned immutable snapshot, bounded bytes/deadline, declared crop and cursor semantics, and a generic CLI. Known CPU and accelerated pixels match; lock, revoke, disconnect, output loss/replacement and slow readers fail closed without leaking or retargeting a frame. |
| 5. Qualify the observer on the desktop | t319 acceptance, then t045 only within its own scope | Full isolated repository gate on the signed candidate, capture-on/off resource and latency measurements, then a matched release and attended capture/revocation check. Record the exact source, profile and artifact. Only the observer is enabled for this milestone. |

The dependency spine is admission review → authenticated attach and group
proof → recipe/composition and portal foundation → capture → qualification.
Under the October 10 sequence, t323 is the default first task, followed by
t133/t275 design and t142 implementation before t317/t318. These rows are
sequenced after the readiness gate; the earlier permission for parallel design
does not move their implementation ahead of it. Read-only prerequisite audits
may inform t323. None authorizes exposing capture before admission is ready.
The existing WM/shell acceptance and performance exits remain unchanged. The
[October 10 sequencing decision](../milestones/dueqd6r0-separate-shared-core-readiness-from-full-wm-qualification.md)
replaces the full t249 prerequisite for administration, broker and portal
migration with t323. It schedules the role-facing foundation before the rest
of t249; it does not waive either role acceptance or retirement measurements.

The output-publication repair has failing-without-fix controls and a full gate.
The separately promoted t322 VT custody repair has also passed its attended
zero-output VT return and evidence integration. t310 and Hagia h018 completed
[occupied-output migration and return acceptance](cmoaia7z-complete-adaptive-output-policy-and-gpu-admission-t310.md#final-workspace-affinity-acceptance-2026-10-10)
on release `niltempus-d59a72c27a32a2c05e27`; this discharges step 1 without
repeating accepted cable survival.

On October 10 niltempus authorized the
[t249 evidence audit](../investigations/5rzn1zue-wm-qualification-after-source-retirement-and-attended-output-recovery.md)
alongside the [t133 admission boundary](esnqxpqw-pidfd-and-namespace-admission-optimizations.md)
and [t275 recipe boundary](../investigations/kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md#proposed-recipe-boundary-2026-10-10).
Those drafts used the immutable-connection rule and did not themselves promote
t317/t318/t319 implementation or bypass the then-current t249 prerequisite.
The later approved sequence below promotes t317/t318 behind their explicit
gates and replaces that broad prerequisite with t323. t257 can supply a
narrow output-availability/status slice without waiting for text status in
every role; t319 requires that slice, not completion of the whole t257 row.

Administration and broker migration (t254/t314), the neutral authority records
and application scaffold (t321/t316), later clipboard/INCR/Xdnd/URI/prompt
executors (t046), and portability/plain 9P (t256) remain separate work. t320's
injection design retains its approved place, but implementing or enabling a
driver is not a screenshot prerequisite. t255 still requires accepted exports
and measurements for every legacy production caller before retirement.

t307 stays open and parked by niltempus's decision: no QEMU, private Mesa or
VM qualification gates this path. Its [re-entry conditions](../investigations/r2m9cx6v-static-retained-dma-buf-images-can-sample-black-before-hotplug.md#t307)
are a physical retained-image failure or a new QEMU/VM-testing goal. The
accepted release 222 and its evidence remain the baseline. Future live checks
test the changed feature; frozen failed runs are never relabeled.

## Task details

## t323

Qualify the bounded shared-core readiness gate before migrating more roles.
This is a subset of t249's obligations, not its completion. niltempus approved
the split and its implementation on 2026-10-10. Root owns the cross-repository
evidence integration; task state and execution order remain in `todo.md`.

First map each obligation to its production owner, existing test, retained
candidate/run and remaining join. Source inspection alone is not a passing run.
Reuse compatible evidence after an impact review; rerun affected tests when
code or fixture bytes change. Generic wire/export tests belong in Sophia,
Hagia policy joins in Hagia, and their runner in external integration tooling.
The eighteen-case `t249-repaint-02` run is the regression baseline, not proof of
the missing credit and identity joins.

The gate has these measurable exits:

| Obligation | Required proof |
| --- | --- |
| Request lifetime | Partial submission has no effect; accepted replay cannot repeat an effect; cancellation preserves already committed work. Exercise both reply/flush orderings and bounded blocked-read cleanup through the shared core and the WM export where its semantics differ. |
| Record and snapshot custody | Malformed or truncated records never reach semantic delivery. Opened snapshots retain generation-pinned bytes and immutable metadata. Credit refusal spends no snapshot identity or partial event; one driver phase owner issues receive permits. Map the existing codec, atomic-cycle, snapshot and driver controls before adding tests. |
| Capacity and progress | Requests, reply bytes, staging, journal/results and acknowledgements remain bounded. A peer withholding reads or ACKs cannot make Session's local revocation or swallowed-release handling wait for credit; overflow or deadline failure is bounded and fails closed. Require a production Session join with the credit genuinely exhausted, not only a supplied receipt or a source assertion. |
| Reconnect and identity | New epochs reject retained old handles, transactions and presentation actions. Reuse a surface's numeric index with a new generation and prove old input/action identity cannot target the replacement. Correlate the first answer or refusal; a timeout or later disconnect cannot stand in for it. |
| Regression and evidence | Run affected core/export checks and all eighteen baseline Hagia cases on the final paired candidate. New behavioral assertions require discriminating controls with recorded first failures. Preserve signed pins, manifests, bounded jobs, source restoration and teardown health; no physical or performance claim follows. |

The [October 10 readiness evidence](../investigations/5rzn1zue-wm-qualification-after-source-retirement-and-attended-output-recovery.md#t323-shared-owner-evidence-reconciliation)
satisfies this exit at Sophia `a8c5fdf29`, Hagia `b4d5702` and runner `6592846`:
the isolated generic gate is bound to the signed source, the ACK-credit join
and reused-surface control discriminate, all nineteen paired cases pass, and
the opt-in SDK export tests pass. Shared wire flush ordering and reply-byte
bounds remain qualified at their common owner; the Session join specifically
exhausts journal record credit. Withholding reads reaching that same journal
bound is source inference, not a second executed Session capacity claim.
The detailed record retains the supplied facts and all remaining t249 limits.

9P flush, fid clunk, policy cancellation and grant revocation retain their
separate contracts. A wire reply is not semantic settlement. Test protocol
invariants once at the shared owner, then test each role-specific connection;
do not multiply equivalent wire tests across every WM capability. A simulated
device completion may replace an external fact, but must not replace the
production owner whose behavior is being qualified.

On completion, t323 releases t254/t314/t315 from the broad t249 dependency.
It also permits t317 after t133. The approved foundation sequence is t133
admission review, t275 recipe design, t142 confined-group proof, t317 attach
identity, then t318 namespace composition. t317/t318 are promoted with those
dependencies; existing custody checks stay until replacement controls pass.
t319 remains gated by t315/t318 and its capture acceptance; neither input
driving nor a native 9P application frontend is admitted by this split.

Return to t249 after t318's exit, including its t317 and t142 prerequisites,
is validated on a signed candidate and the affected Hagia regression passes.
Do not wait for t255: it depends on t250, which still depends on full t249.
Complete remaining WM joins in groups: backend admission/presentation and
launch; output lifecycle; input/capture/reload. The
[coverage reconciliation](../investigations/5rzn1zue-wm-qualification-after-source-retirement-and-attended-output-recovery.md#fourteen-case-coverage-reconciliation-2026-10-10)
and its dated successors retain every requirement. Software-Present admission,
the native service tail and automatic repaint scheduling remain t249 gaps.

Before any latency campaign, review complete offered/admitted/replaced/settled
accounting and a same-workload comparator. The t249 budgets, failed historical
campaign and whole-release rollback remain unchanged. An unavailable valid
comparator leaves qualification open; it is not permission to replace the
relative gate with a transport microbenchmark. Final closure requires the
full coverage matrix, inspection/export controls, signed isolated gates and
reproducible measurements, followed separately by t250 physical acceptance.

## t314

Serve the metadata broker files export on the core from the designed
[broker file contract](../../sophia-broker-files.md). The protected broker
process is the export's single admitted client, with its own admission and
bounded custody as the
[custody decision](../decisions/xa78u03g-keep-broker-and-portal-file-authority-and-custody-separate.md)
requires. Exit: the broker reaches every production path it reaches today
through files alone; the legacy broker socket has no production caller; a
compiled negative control proves a foreign connection cannot attach to the
export; latency, CPU and copied bytes are measured against the current
envelope for the same descriptor workload before the socket is removed under
t255.

## t315

Serve the portal files export on the core from the designed
[portal file contract](../../sophia-portal-files.md) with the bind executor
model. The portal decision owner serves request, grant and revocation over
files and never executes. Execution belongs to the recipient's frontend: a 9P
recipient receives a bind of one object into its tree for the grant's
lifetime and its open fid answers ESTALE after revocation; an X recipient
receives a translation in the X authority. Exit: no request record carries a
payload, descriptor, raw object ID or unbounded string; denied, stale, foreign
and revoked requests fail closed with compiled controls; a grant dies with
source generation, deadline, disconnect, lock or broker restart. Prove the
generic bind/revoke seam with a device-free recipient and preserve every
currently implemented X clipboard/PRIMARY executor outcome through the new
files path. No legacy production behavior may disappear during migration.

This is the portal foundation. It does not require clipboard text through a
future 9P application frontend. t319 supplies the first real new 9P transfer;
t046 owns later clipboard and other executor gaps, with explicit end-to-end
proof for each recipient path as that frontend becomes available. t255 may
retire the old envelope only after all its existing production callers have
accepted replacements, regardless of this split.

## t316

Rebuild the `sophia-9p-authority` scaffold as an export on the core. Delete
its decode and encode modules, which duplicate `crates/sophia-9p/src/wire.rs`,
remove the FUSE service claim from its crate header, and make it attach
through the `Export` trait with an admission-derived root, targeting the
protocol authority contract named under t321. Exit: the crate
builds with no protocol codec of its own; its tree is served by the core in a
device-free test; the
[application frontend design](../../sophia-9p-authority.md) drops the FUSE
and separate-codec text; no application API, content format or input
contract is added.

## t321

Name the protocol authority contract once. The records the session exchanges
with the X authority today are the contract under X names: observed
transaction batches and CPU buffer updates, allocation preferences and
metadata candidates, routed input with its origin, output update and
presentation outcomes, pointer grab anchors and responses, dma-buf import
formats, service commands and the injection policy. Move and rename them into
`sophia-protocol` as protocol-neutral records, make the X authority their
first implementor with no behavior change, and make the rebuilt application
authority under t316 target them. Exit: the session's run loop names no
X-specific type for any of these exchanges; every existing X gate passes
unchanged, including the all-profile gate; the application frontend document
describes the contract as the single seam into Engine; no trait is added and
no third-party path is opened in process.

## t317

Prove identity at attach. The core accepts an afid, runs the factotum
conversation, and the admission policy derives the `ClientAdmissionContext`
from that identity together with the supervisor's launch custody record for a
child it spawned. The first successful attach fixes the principal and
namespace; later attaches must prove that same identity and cannot widen its
admission or rebase retained fids. Different identities use separate connections.
Peer checks become optional only after equivalent admission and custody
controls pass. X11 admission is unchanged. Exit: an attach without a valid afid on a
role that requires one is refused; a forged uname confers nothing; an attach
for a second identity is refused without changing existing fids; a client
the supervisor did not launch cannot acquire custody; lock-file admission no
longer hard-requires `SO_PEERPIDFD`; the admission threat model in the
[admission investigation](../investigations/1pv291te-namespace-and-client-admission-security-gaps.md)
is reconciled with the result. Depends on t133 and t323.

## t318

Implement the namespace recipe and composition layer: a per-identity socket
directory with a discovery file, derived from the admission context and role
grants, with role trees staying one per endpoint and binds landing in the
granting role's tree, following the design t275 produces from the
[namespace investigation](../investigations/kcfh2hdg-adopting-the-plan-9-namespace-model-in-sophia.md).
The recipe is declarative and sealed at attach; clients cannot rebind their
own view. Exit: two private clients with overlapping names see different
trees; an explicitly shared service has one identity in both; a rebound
service changes the view without changing retained handles' authority; a
revoked transfer makes its fid answer ESTALE; visibility, service identity
and permission are recorded as three separate results; the shared and
confined session profiles are expressed as two recipes, and the shipped
observer recipe for agents and operator tools binds only status, inspection
and capture grants, with input injection absent. Operator and driver recipes
do not inherit observer grants; deliberate composition is explicit policy.
Depends on t275, t142 and t317.

The [monitor continuity contract](cmoaia7z-complete-adaptive-output-policy-and-gpu-admission-t310.md#persistent-services-and-replaceable-display-attachments)
adds a lifetime control: a display's replacement generation cannot silently
retarget an existing output-bound handle. Role connection identity and physical
generation are independent. Pinned read-only inspection bytes may remain
historical; operations requiring a live output must reject stale identity.

## t319

Deliver capture as the first portal bind, carved from
[t046](queue-16-portals-and-confined-applications.md#t046). An output-only
grant binds a frame record, carrying dimensions, format, output identity and
presentation generation, and a frame file of bounded bytes into the
requester's tree. The frame comes from an immutable renderer-owned snapshot
at the [renderer import boundary](../../renderer-import-boundary.md), the
same primitive an accelerated same-namespace GetImage would use. Exit: a
small CLI reads one image and its generation under one grant; capture is
refused while the session is locked and none in flight completes after the
lock is applied; the fid answers ESTALE after revocation; bytes already
delivered cannot be recalled; known pixels match
on a CPU client and an accelerated client; no XTEST admission is implied; the
dma-buf form stays with the recording kind on a separate descriptor channel.
Depends on t315 and t318, including t318's admission and group prerequisites,
and on the bounded availability/status slice of t257 described above.

Loss or replacement of a captured output invalidates its generation-bound grant;
retained bytes do not become a capture of the replacement. Prove the old fid's
stale/revoked result and a new grant's independent identity. The t257 status view
must distinguish current availability from the last presented topology, with
bounded recovery stage, owner/presentation generations, retry time and failure
identity. Output protocol revision 1 cannot carry an empty head set, so do not
use an invalid topology record as this availability signal. These additions
do not reopen the accepted t306 physical gate.

## t320

Design the 9P-side input injection contract, the driver tier's mechanism for
9P applications and the equivalent of admitted XTEST for X clients. An
admitted window's input file is served by the application authority and bound
into a driver's tree by its grant; the application never sees the file and
cannot tell injected input from physical input except through the journal.
Injected events enter Engine's routing at the same seam as admitted XTEST
injection in `crates/sophia-x-authority/src/x11_socket/connection/xtest.rs`,
so target resolution against presented state, bounded capture, cancellation,
the lock refusal and revocation epochs from the
[target-resolved input contract](../../target-resolved-input.md) apply
unchanged. The design must settle: the record format and its relationship to
the application authority's routed-input encoding; provenance marking so the
journal and policy distinguish injected from physical input; the single
namespace a driver grant may address; backpressure, deadlines and the slow
writer; refusal while locked with nothing in flight completing after the
lock; and how the X-side XTEST admission and the 9P-side file share one
admission so a namespace has one injection permission regardless of its
clients' protocols. Exit: a reviewed design recorded in the
[application frontend document](../../sophia-9p-authority.md) as labeled
target text, an amendment to the one-core decision naming the chosen seam,
a device-free test list covering accept, refuse, revoke, lock and provenance,
and no implementation. This precedes any application API under t316's
successor work.

## Connections

- [Serve every public role from one 9P core](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md)
  is the accepted decision these rows implement.
- [Plan 9 integration points](../concepts/ernn0bkv-plan-9-integration-points-for-sophia.md)
  holds the reasoning and the cost estimate.
- [Desktop role migration](jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
  owns t249, t250, t252, t254 and t255.
- [Socket directories and frontend groups](ooy00zjd-socket-directory-and-frontend-multiplexer-architecture.md)
  owns t142, and the [pidfd proposal](esnqxpqw-pidfd-and-namespace-admission-optimizations.md)
  with the admission investigation owns t133.
- [Portals and confined applications](queue-16-portals-and-confined-applications.md)
  keeps the remaining t046 gaps and t045.
- [Namespaces and portals](../../namespaces-and-portals.md) is the normative
  contract these rows extend and must update as each lands.
