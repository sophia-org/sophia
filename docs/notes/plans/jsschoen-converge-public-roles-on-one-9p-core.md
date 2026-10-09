---
id: jsschoen
date: 2026-10-09
kind: plan
tags: [plan, milestone, 9p, namespaces, portals]
---
# Converge public roles on one 9P core

## Scope and exit

This plan owns the implementation rows that follow from the
[proposed one-core decision](../decisions/zsx0tk4k-serve-every-public-role-from-one-9p-core-with-namespaces-as-composed-trees-and-portals-as-binds.md).
The milestone ends when every public role is an export on the `sophia-9p`
core, the legacy socket envelope has no production caller, the application
authority scaffold attaches through the same core, identity can be proven at
attach, each connection's tree is derived from its admission context by a
recipe, and one portal kind has been executed as a bind. Engine, the X
authority and the existing role file contracts are unchanged throughout.

The plan creates no critical-lane work. Rows enter as parallel work where a
driver and a measurable exit exist and as candidates where they wait on a
gate. Task state and execution order live in [todo.md](../../../todo.md).
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

## Task details

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
source generation, deadline, disconnect, lock or broker restart; one existing
kind, clipboard text, is proven end to end through both recipient paths.

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
child it spawned. Peer pidfd and peer credentials remain optional platform
checks. X11 admission is unchanged. Exit: an attach without a valid afid on a
role that requires one is refused; a forged uname confers nothing; a client
the supervisor did not launch cannot acquire custody; lock-file admission no
longer hard-requires `SO_PEERPIDFD`; the admission threat model in the
[admission investigation](../investigations/1pv291te-namespace-and-client-admission-security-gaps.md)
is reconciled with the result. Depends on t133.

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
and capture grants, with input injection absent. Depends on t275 and t142.

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
lock is applied; the fid answers ESTALE after revocation; known pixels match
on a CPU client and an accelerated client; no XTEST admission is implied; the
dma-buf form stays with the recording kind on a separate descriptor channel.
Depends on t315 and t142.

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
  is the proposed decision these rows implement.
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
