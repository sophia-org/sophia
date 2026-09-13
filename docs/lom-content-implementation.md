# Lom CPU content implementation

The operator selected GPU rendering followed by bounded readback and CPU-byte
transfer as the first native Lom experiment on 2026-09-12. Direct GPU-buffer
handoff remains a later measured optimization. This record does not establish
that Lom is an admitted or visible native shell.

## Implemented boundaries

`sophia-protocol` now encodes all twenty-one content records at 160–180 with the
existing 24-byte envelope. The family stays at revision 6; indicator kinds
181–186 and older messages are unchanged. Capability constants are vocabulary,
not an enabled production grant. The legacy `ShellSessionTransport` entry point
reports content unavailable; an explicit admission API distinguishes unavailable,
operator-denied and granted states for controlled hosts.

The codec validates bounded lengths, identity shape, reserved fields, exact
format, scales and transfer dimensions. Actual grant, allocation, permission,
reference and lifecycle checks belong to the session. A different nonzero epoch
is structurally valid data but stale relative to a particular live grant.

The resource owner reserves staging storage, resident credit, a resource slot
and terminal/release response capacity before admitting a transfer. End moves
the owned byte vector into immutable accepted storage. Partial, malformed,
duplicate and expired transfers cannot acquire leases. Retire rejects new
consumers; existing leases keep their bytes until the last reference drains.
Release is independent of a candidate outcome.

The epoch pool reserves the maximum live footprint before admission: 8 MiB
staging + 16 MiB resident + 16 MiB retiring = 40 MiB. Disconnected owners remain
until every consumer drains; actual retained bytes count against 64 MiB globally.
A new 40 MiB reservation fits only with at most 24 MiB already retired. Pinned
resident storage is not forced through the active retiring ceiling. At most
sixteen retired epochs can retain metadata; tiny resources cannot retain an
unbounded number of replay tables. Either limit causes backpressure, not eviction.

The transport now reserves a session-global epoch owner before it publishes
`Welcome` and `ContentLimits`. Its bounded resource service accepts only the five
resource request kinds for the active grant, leaves allocation and candidate
records queued for their own owners, and routes exact status/release events over
the same socket. A fixed response slot is checked before consuming each request,
so backpressure does not discard an owed result. Disconnect revokes the active
epoch while renderer leases keep retired storage alive.

A display-independent candidate reducer now carries a one-use permit through
assembly, accepted pending work and the non-cancellable renderer boundary. It
validates exact output facts, interaction, allocation, scale and resource
generations at End; pins every distinct resource before acceptance; bounds table
counts and control outcomes; and emits visible permit, assembly and preparation
timeouts. Only pending work may be superseded. Submitted work survives peer
revocation until the caller reports renderer failure or actual native retirement.
Prepared and Presented are separate records, and Presented cannot be emitted
before Prepared. The epoch pool retains a disconnected candidate owner alongside
its resource owner, so a renderer lease cannot disappear with the socket.

The shell transport now services Begin, Chunk and End through that reducer for
the exact output context supplied by its Engine caller. It exposes distinct
methods for entering the non-cancellable submission phase and for reporting
Prepared, Presented or renderer failure. A protected conformance host grants one
permit, receives a complete candidate from Lom, verifies its immutable pixels
and tables, and deliberately reports renderer failure because the host has no
native output. That is a real terminal path and explicitly not presentation.

Frame demands now take the same bounded route. One standing demand per output
coalesces until Engine grants it; withdrawal cannot be replaced by lower-priority
dirty or animation work. The client may hold one new standing demand alongside
one granted, unconsumed permit. Exact cancellation and permit expiry produce
visible `FramePermit` states, and their response capacity is reserved before the
demand is accepted. Lom's conformance client derives the output identity from
`ContentOutputFacts`, raises a demand, and uses only the returned permit.

Allocation proposals now have their own Engine-owned reducer and transport
service. Engine publishes complete output facts, supplies presented-parent
evidence for popouts and resolves each request into an exact logical and physical
snapshot. The reducer owns request freshness, finite pending slots, deadlines,
role and coverage budgets, endpoint quantization, generation changes and
correlated grant, rejection and release results. The protected host makes Lom
request its panel allocation rather than injecting a fixture behind the client.

The production metadata-shell owner now calls the resource, allocation, demand,
candidate, preparation and presentation services from its bounded owner loop.
It publishes current outputs, resolves panel and anchored-popout allocations,
projects accepted pixels into Engine composition, retains immutable resource
leases through renderer retirement, and reports Presented only after the exact
native candidate retires. Panel reservation bands replace the presented work
area at that same boundary. The content grant remains unreachable in production
until the separately admitted GPU domain exists. Discrete actions and exact
presented-target routing remain absent.
Event creation and output queueing are not proof of client receipt. Cancellation
echoes its request transaction; timeout correlates to Begin. Peer loss accounts
for undeliverable events without reporting delivery.

Lom converts Vello's straight-alpha RGBA readback into premultiplied B,G,R,A in
place, using rounded integer arithmetic in sRGB channel space and transparent
black at alpha zero. Its iterator produces maximal whole-row chunks without
duplicating the image. Content dimensions are checked against 4 MiB before GPU
rendering; the diagnostic preview has its own separately named bound.

The renderer uses the texture path and its own map/copy operation instead of
`imaging_vello`'s indefinite readback wait. One two-second deadline covers the
poll and callback. A failed job is retained and the renderer refuses new work.
This recovery budget is not presentation pacing, proof of GPU completion, a
bound on arbitrary driver calls, or an aggregate GPU allocation budget.

## Evidence and limits

TLC v1.7.4 was verified against the SHA-256 pinned by `tools/check_tla.sh`.
`ShellContentLifecycle` and `ShellContentBundleComposition` pass. The retire-
during-assembly, silent-timeout and latched-readiness controls fail on their
intended invariants. Logs are in `.artifacts/lom-content-admission/`. The initial
sandboxed attempt failed to bind TLC's RMI listener; it is retained separately
and is not counted as a model result. Renderer-progress liveness remains an
explicit assumption, not a proved completion guarantee.

The codec corpus has 21 valid records and 68 malformed vectors. The independent
C reader uses no Sophia libraries. Inverse outcome checks make the gate fail.
This proves the exercised byte cases, not every semantic predicate or native
publisher behavior. Existing descriptor, tab, reference and launcher clients
remain in the shell gate.

Resource tests cover consumer retention, exact release, timeout settlement,
replay, foreign grants, response pressure and partial reconnect retirement. A
real private Unix-socket test covers negotiation, limits, upload admission,
immutable acceptance, permit and candidate transfer, ordered outcomes, retire
and release through the display-independent client crate. The separate protected
host runs Lom's own client and therefore checks the same candidate bytes without
using Sophia's client implementation.
Candidate tests cover permit and assembly deadlines, exact End validation,
malformed direct calls, resource retirement during assembly, pending-only
supersession, Prepared-before-Presented ordering, renderer failure boundaries and
disconnect retention through a real epoch owner. They do not constitute a native
submission or retirement test.
Lom tests cover pixels, row chunking and the production poll/callback deadline
function without opening a GPU. The changed Lom readback has not been GPU-tested.

The subsequent full Sophia gate accidentally inherited
`SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1` and attempted a real DRM initial modeset.
Resource and framebuffer creation succeeded; atomic submission failed. This was
an unintended hardware test, not Lom or native-content acceptance. The log does
not establish that the desktop was unaffected. The full gate failed and must
not be reported as passing. Evidence and its digest are preserved in
`.artifacts/lom-content-admission/hardware-attempt.md` and `full-gate.log`.
Further offline gates require the device-hidden conformance wrapper with a
cleared environment and private `/dev`; clearing opt-ins alone does not prevent
tests that discover render nodes automatically. The failed hardware run will
not be retried.

## Production admission remains closed

GPU permission is an explicit startup protection-domain grant, not a negotiation
bit or an implication of content permission. Selected device visibility and
enforceable allocation accounting must exist before enabling production content.

The existing DRI3 256 MiB limit is per import; it is not an aggregate budget and
does not cover Vello's internal GPU buffers and caches. Pinned Vello 0.8.0 and
imaging_vello 0.0.1 expose no aggregate renderer allocation-budget hook. One job
or bounded buffer dimensions do not establish an aggregate bound. The inspected
host has no `dmem` cgroup controller and `/boot/config-6.18.50_1` states
`CONFIG_CGROUP_DMEM` is not set. This is neither a kernel-change recommendation
nor evidence that that option alone would satisfy the policy.

An allocator/device admission design with an enforceable owner remains required.
No render-node bind, GPU permission, installed profile or shell replacement was
added. The CPU pool does not claim to count renderer/upload copies that are not
yet integrated with it.

Still required: action lifecycle, GPU-domain launch admission, Lom's persistent
connection and exact presented-target adapter, and attended acceptance. The
operator profile vocabulary now defaults content off, admits an explicit
content decision, and validates the exact prototype GPU-memory number. The first
live configuration is workspaces, clock and calendar. Other Minimal modules
remain fixtures until authorized live sources exist. Acceptance must prove
retirement, activation, anchoring, consumed outside dismissal, restart and
output changes.
