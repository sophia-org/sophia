---
id: r3b9n7cf
date: 2026-09-16
kind: investigation
status: investigating
tags: [shell, native-components, ownership, validation]
---
# Native component storage and compositor identity

Parent source: `627e4b2b`. This is the first ownership implementation for the
[approved independent launcher contract](../decisions/f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md),
not native Bemenu admission or t104–t108 completion.

## Actual owners

`ContentEpochRegistry` owns bounded vectors of real allocation, candidate and
resource stores. It admits two simultaneous grants under one logical/backing
budget, returns stores only by the complete grant, and retains disconnected
stores while their actual consumers remain. Each live owner reserves a future
retirement entry; live plus retained inventories cannot exceed sixteen. Storage
is reserved at construction, not grown by transferring an epoch on disconnect.

The existing `ContentEpochPool` remains a single-connection store compatibility
facade and delegates to that same registry implementation. At `c7dea19a`, each
transport still owned that facade. The later shared transport checkpoint below
removes that dependency from the connection state; live Session construction and
routing remain the next integration. Creating two legacy owning wrappers would
still duplicate budgets and is not the independent component construction.
No Rc/RefCell sharing or parallel sidecar accounting was introduced.

The registry uses the approved 64 MiB ceiling with per-grant limits supplied by
its admission caller. Tests use bar 8/16/16 and launcher 4/12/8 MiB, totaling
64 MiB logical and 52 MiB backing reservation. Failed admission does not advance
epoch watermarks; successful admissions require both fields to increase. A
retained launcher byte consumer prevents replacement at this full profile;
Lom's real store and grant remain intact. Final backend disposition refuses
while either grant remains live and cannot settle independent byte consumers.

`CompositorNodeId::ShellContent` now includes the complete grant. Session copies
it from the admitted bundle, and backend intake requires frame, node and real
resource owner to agree before queueing. Presented-history matching preserves
that identity. This prevents independently numbered candidates/surfaces from
colliding in the damage map. It does not yet replace the output-only runtime
content map with a multi-component projection or implement input arbitration.

## Evidence and limits

Evidence is retained under `.artifacts/bemenu-components`. The new public registry
controls exercise real stores and leases; the Engine control exercises the real
damage function with non-owning metadata. The backend control drives actual
content intake and the shared owned queue with simulated native completion.
None opens a graphics/input device or establishes driver/native compatibility.

Scoped positives: five shared-store controls plus fourteen existing resource
and twenty-two candidate controls; 150 backend library controls including actual
intake; twenty Engine controls; twenty-four renderer controls; 467 Session
controls with fourteen ignored. The new admission checks, damage key and common
logical budget each have a compiled behavioral mutant. Partial-grant disconnect
has a fourth compiled negative. Disposable source hashes restore exactly.
An initial backend mutant selector ran zero tests and is excluded; the corrected
exact selector runs one test and fails behaviorally.

The first isolated Session run lacked repository fixtures: 458 passed, nine
configuration tests failed, fourteen ignored. It is retained as a harness
failure. With a bounded source snapshot and the compiled manifest-path alias,
the same binary passed 467 tests with fourteen ignored. The first renderer
selector omitted gbm-probe and ran zero head-composition tests; that is not
coverage and is kept separate from the feature-enabled run.

Outstanding: revision-7 wire/config/model contract, Session-owned multi-component
registry/supervision, complete per-grant projections/retirement claims and input,
focused text/catalog binding, reusable C lifecycle, Bemenu hookup, joined
1,000-cycle multi-component controls and attended acceptance. Existing tests of
single-shell retirement are not renamed as the new two-client 1,000-cycle gate.

## Frozen ownership checkpoint

Signed `c7dea19ae6785d1e30915a5a3ba477d2c344ceb0` passed the exact-source
contained canonical gate and was published to master. Evidence:
`.artifacts/offline-check-c7dea19a/report.json` and `execution-summary.json`.
274 Rust result groups reported 3,391 passes, no failures, thirty ignored and
zero compiler warning lines. HEAD/parent signatures passed; sibling evidence
checks signed commit identities only, not tree/blob contents. Hardware and
promoted host archives were NOT_RUN. No native acceptance follows from this.

The scoped renderer all-tests Clippy invocation initially omitted egl-probe,
exposing an existing fixture import that needs both gbm-probe and egl-probe.
The feature-complete strict invocation passes. No fixture or lint was weakened.

## Shared transport checkpoint over bef4fd7a

`ShellComponentTransport` owns the actual endpoint, socket, inbox, FIFO and
connection-local response/action obligations. Its content operations explicitly
borrow a `ContentEpochRegistry`; all active-store lookups use its complete retained
grant. The public `ShellSessionTransport` compatibility wrapper retains a private
registry and delegates to those same operations. It exposes no mutable dereference
that could detach its connection from its registry. Existing legacy call sites
exercise the shared implementation without changing their API.

`reserve_content` reserves the complete caller-selected limits before a supervisor
launch. It does not grant a capability. A second reservation on that connection or
a mismatched connection at negotiation refuses without replacing the owner. The
real Welcome/Limits exchange retains those exact limits rather than reconstructing
the larger bar defaults. Any attempted negotiation failure disconnects the exact
reservation; the original failure remains authoritative if endpoint bookkeeping
also fails. Explicit abandonment still requires the caller to disconnect.

The compatibility negotiation path obtains its next content epoch from the shared
registry. Component admission must still mint connection identities globally;
the later Session inventory must burn identities on attempts that reserved or
disclosed them, not only successful finalization. The existing blocking handshake
is not a fair multi-component owner-loop scheduler: bounded asynchronous launch/
negotiation, rotating service budgets and the two-component supervisor inventory
remain unimplemented. Live configuration continues to refuse independent mode.

Response budgets are connection-local store credits plus that connection's owned
FIFO/cancellation/outcome records. `ShellContentAccounting.epochs` is explicitly
the common registry snapshot; callers must not add it once per connection. Native
completion can settle the exact retained grant while current-peer response
delivery stays tied to the matching live connection. This is not an additional
resource ledger, and it does not make worker join sufficient to release consumers.

Five device-hidden real-socket controls cover exact pre-launch reservations and
wire limits, two owners with identical resource IDs, continued neighbor upload
while an old launcher byte consumer blocks replacement, forged peer grant refusal,
failed handshake cleanup, common epoch allocation, and preservation on refused
replacement/wrong-connection calls. They use supplied protection evidence and the
real Rust client, transport and stores. They do not launch a protected child,
exercise native completion/focus, or establish revision-7 capabilities.

Four compiled mutations each fail their exact one-test control: omit the initial
reservation; retain a failed handshake reservation; use a transport-local content
counter; omit dead-owner bytes from common admission accounting. Sources are
restored after each control. Evidence: `.artifacts/bemenu-shared-transport/`.
Existing transport credit/partial-write tests retain their assertions while
addressing the now-explicit connection state and registry separately.
Device-hidden scoped validation passes 195 runtime tests and 468 Session library
tests (fourteen Session tests ignored), strict affected Clippy, formatting and the
repository layout gate. These results are separate from a subsequent frozen
canonical gate and from physical acceptance.
