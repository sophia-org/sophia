# Style Guide

This guide defines implementation discipline for Sophia. It is intentionally
small because the project is still a research prototype. The rules here defend
the data model in `dod.md`.

## Languages

Sophia user-space components are Rust by default.

- Rust: Sophia Engine, protocol authorities, portals, CLI tools, reference WM,
  and compatibility/prototype bridges.
- C: narrow XLibre prototype patches and protocol extensions.
- Nim: optional experimental WMs or policy prototypes.
- Zig: optional probes or small C-adjacent helpers, not the main architecture.

Do not mix languages inside one component without a concrete boundary reason.

### Repository tooling

Use Rust through the existing `xtask` for new maintained tooling: test harnesses,
isolation orchestration, protocol validation, evidence collection and release
automation. Keep typed identities, explicit errors and bounded subprocess work
in the owning modules rather than accumulating one-off scripts.

Use shell for short launchers and straightforward command sequences. Python is
appropriate for disposable analysis and experiments, not the default for new
reusable infrastructure. Prefer direct patches for source edits; use scripted
transformations only when their repetition warrants it, and inspect the diff.
Independent C or other language protocol checks retain their interoperability
purpose; do not replace them with a second call to the Rust codec.

Migrate recurring Python or complex shell tooling incrementally when its owner
is already being changed. Existing working tools remain supported; this policy
does not require a rewrite before shipping the dock. Preserve device-hidden
execution, fail-closed prerequisites, timeouts, evidence formats and negative
controls during migration. Prove equivalent behavior before replacing a gate;
changing its implementation language does not authorize GPU or live-session
access. Ordinary checks must not acquire hardware through autodetection.

## Rust Layout

Prefer subsystem directories over large files:

```text
src/
  types/
  state/
  protocol/
  systems/
  authority/
  bridge/
  portal/
  wm/
  engine/
```

The same data/logic split applies in Rust:

- `types` contains passive records, IDs, enums, and flags.
- `state` owns tables and lifetimes.
- `protocol` serializes packets and validates wire data.
- `systems` transforms data.
- `authority` terminates a client protocol and owns protocol resources.
- `bridge` talks to legacy or external prototype authorities such as XLibre.
- `portal` owns cross-namespace transfer policy.
- `engine` owns compositor state and hot-path scheduling.
- `wm` owns policy examples.

Avoid placing behavior on data records unless it is a pure helper such as
validation, conversion, or formatting.

Split production `src` files by domain ownership, not by strict line count. A
file should have one owner and one reason to change. Split when a file mixes
domains such as protocol parsing, runtime state, socket I/O, rendering, policy,
or tests.

Files around 800-1000 lines need a cohesion check. They may remain large when
they are one dense parser, protocol table, or tightly coupled algorithm, but the
default should be to look for real seams before they grow further. Keep the old
public module as a facade when callers already depend on that path. Do not split
purely to satisfy a number if the result would obscure ownership or scatter one
tightly coupled algorithm across files.

Run `tools/audit_source_layout.sh` during local validation. It reports
production files at 800 lines, tests at 800 lines, and rejects unreviewed
production files over 1000 lines, inline production tests, and direct printing
from libraries. `docs/source-layout-exceptions.txt` is an exact-path migration
ledger, not a permanent allowlist; remove entries as each owning domain is
extracted.

### Engine Crate Modules

`crates/sophia-engine/src/lib.rs` is the public facade for the engine crate. It
should declare domain modules and re-export the stable public API; it should not
grow into an implementation file again.

Engine implementation belongs in domain modules such as runtime driver, input,
frame scheduling, output discovery, rendering, visual state, chrome, session,
WM IPC, and the headless engine orchestrator. Keep shared helpers in exactly one
owning module and expose them as `pub(crate)` only when another engine module
needs them.

### Backend-Live Crate Modules

`crates/sophia-backend-live/src/lib.rs` is also a crate boundary, not an
implementation file. It should declare the backend domains and re-export the
public facade.

Keep kernel-facing code grouped by ownership:

- `drm` owns libdrm/KMS protocol facts, atomic requests, page-flip decoding,
  primary-plane resources, and native scanout submit/retire mechanics.
- `input` owns libinput polling and input packet reduction.
- `runtime` owns one-tick backend assembly and rendered primary-plane runtime
  adapters.
- `scanout` owns reduced scanout state, page-flip intake, rendered scanout
  tracking, and backend-neutral scanout reports.
- `hardware_validation` owns preflight probes, reduced validation gates, and
  real hardware smoke/session owners.
- `startup` owns backend discovery and renderer startup probes.

When a backend-live module starts mixing those ownership boundaries, split it
into a directory and keep the old module path as the facade.

## Test Placement

Rust tests live outside production source files.

Use crate-level integration tests:

```text
crates/<crate>/tests/
  behavior.rs
  support/
    mod.rs
```

Rules:

- Do not add `#[cfg(test)] mod tests` to files under `src/`.
- Do not add `#[test]` functions to files under `src/`.
- Put shared fixtures, builders, and mock data under `tests/support/` or inside
  the integration test module that uses them.
- Test through public APIs. Do not make private helpers public only so tests can
  reach them.
- If a private invariant truly cannot be tested through public behavior, record
  the exception in this guide before adding an inline test.

This keeps production modules readable and forces tests to exercise the same
crate boundary that downstream Sophia components use.

The renderer's private pixmap-allocation adoption handshake is an exception to
public-API testing. Its external `tests/support/shared_pixmap_adoption.rs` file
is mounted inside the service module to control cancellation between reply
enqueue and receipt. A GPU-backed public call cannot deterministically place
that race. The production API remains unchanged; all test bodies stay outside
`src`.
The session device coordinator similarly mounts external
`tests/support/render_device_coordinator.rs` inside its private owner module.
These tests control preparation and frontend acknowledgements independently to
exercise overtaken device generations without acquiring or removing real GPUs.
The mount is recorded in the layout exceptions; no test bodies enter `src`.


The shell lifecycle controls also need private-owner access. Their external
`tests/support/lifecycle_tests.rs` and companion fixtures drive the production
intake, lowering, owned queue, reservation, installer and custody with simulated
copy/device completion. The production API requires native devices and cannot
place the deterministic refusal, lagging-head and retained-consumer states.
Test-only crate re-exports join these existing private owners without widening
the release API. The runtime retirement-authority fixture similarly mounts
externally to distinguish the persistent and transient custody paths.

External transport-budget and client outbox/candidate fixtures need to inspect
exact producer credits and partial-write ownership across refusal; this includes
`tests/support/shell_indicator_responses.rs`, which retains the exact completed
WM response while forcing FIFO admission refusal. The public
socket API cannot select those internal transfer points. Session's external
content-action fixtures join the real private socket, generic client and action
ledger, using the shared WM admission boundary. These mounts and test-only
re-exports are recorded individually in the layout exceptions. All fixture
bodies remain under `tests/support`; their scope does not imply hardware or
full owner-loop acceptance.

The persistent catalog response fixture, `tests/support/shell_catalog_responses.rs`,
uses the same narrow exception: supplied negotiated state, exact private credit
accounting, forced returned refusal and partial FIFO drain. It does not enable
dock negotiation or claim kernel backpressure. Its bodies remain outside `src`;
only its individual module mount is listed, with no checker or legacy-debt change.

The private input Session fixtures mount `tests/support/private_input_session.rs`
and `tests/support/private_input_generations.rs` to inspect exact retained custody,
poison ownership locks, and prepare a candidate before a competing Engine commit.
Those intervals are unavailable through the public controller. Their two mounts
are recorded individually; the public socket acceptance tests remain separate.
The private input service, lifetime constructor and fault carrier also contain
test-only wiring for that external fixture. It drops the real command sender or
installs a thread-local subscriber around the serving call; the subscriber body
lives in `tests/support/private_input_faults.rs`. The release carrier is empty
and no public configuration can request a fault. These exact wiring paths are
listed individually because the layout checker also flags `cfg(test)` glue.

## TEA Policy Style

Use TEA-style structure for policy components:

```text
model + event/snapshot -> update -> command
```

Good fits:

- Sophia WM layout, workspace, and focus policy.
- Portal transfer policy.
- Session or launcher policy hints.

Poor fits:

- compositor hit-testing
- frame scheduling
- damage aggregation
- renderer/backend execution
- protocol event mirroring

For TEA-style modules, keep update functions deterministic where practical.
They should consume passive packets and emit command packets. They should not
reach into compositor, XLibre, or portal-owned state through callbacks.

For compositor code, prefer explicit data-oriented systems over a global message
loop. The engine is a security boundary and a hot path; clarity of authority,
bounded allocation, and predictable control flow matter more than architectural
uniformity.

## Naming

Use ordinary Rust naming:

- Types and traits: `PascalCase`
- Functions and variables: `snake_case`
- Modules: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`

IDs should make ownership clear:

- `SurfaceId`, not `WindowId`
- `XWindowId`, not raw `u32`
- `NamespaceId`, not raw string in hot paths
- `TransactionId`, not `Serial` unless it is truly protocol-local

Raw protocol IDs should be wrapped at the boundary where they enter Sophia.

## Ownership

State has one owner. Other components receive snapshots, IDs, or handles.

Prefer:

- dense tables with typed IDs
- generation checks for long-lived references
- immutable snapshots across process boundaries
- explicit handle ownership

Avoid:

- global registries with mutable aliases
- shared object graphs
- callbacks that mutate state hidden behind another component
- stringly typed IDs in hot paths

## Errors

Errors should name the boundary that failed.

Good examples:

- `XBridgeError::BadWindow`
- `InputRouteError::StaleSurface`
- `PortalError::PolicyDenied`
- `TransactionError::TimedOut`

Policy denial is not an internal error. Treat it as an expected outcome with a
clear status.

## Logging

Sophia libraries use `tracing` for structured diagnostics. Binaries and runtime
entrypoints install subscribers; libraries do not.

The CLI forwards the backend's `sophia_scanout_evidence` target to the daily
recorder only for `sophia_live_atomic_test` and `sophia_live_layout_probe` messages.
This observation layer has its own filter, independent of console `RUST_LOG`.
It forwards no spans or auxiliary fields, bounds formatting before allocation,
and sends oversized records through existing discard accounting. Library module
moves must preserve this explicit target. Mixed child stdout/stderr remains
outside daily capture.

Default logs must not expose sandbox-sensitive identity or payload data:

- no raw XIDs, namespace IDs, window titles, app classes, PIDs, or icon pixels;
- no clipboard, drag-and-drop, file, URI, notification body, or pixel payloads;
- no raw portal payload handles unless the handle is explicitly opaque and
  already user-approved for that log context.

Prefer opaque Sophia IDs, generations, counts, enum outcomes, and durations.
Engine logs should describe compositor/session decisions, not user data.

Levels:

- `trace`: per-layer, per-command, or hot-path counters.
- `debug`: normal state transitions and accepted reducer outcomes.
- `warn`: rejected, stale, invalid, timed-out, or fallback outcomes that are
  expected but security-relevant.
- `error`: only when a library cannot return the failure to its caller. Most
  engine failures should be returned as typed errors instead.

### Telemetry Schemas

A diagnostic record's fields are its schema, and gate scripts and log readers
depend on them. Name fields for what they measure rather than where they were
emitted, keep a field's meaning stable once published, and add fields rather
than repurposing them. A count of what was lost is not optional on a degraded
outcome: `discarded = 0` must mean nothing was dropped, never that nobody
counted. Report at `warn` with a cumulative total rather than per occurrence, so
a resource under sustained pressure does not saturate the log as well.

Saturation telemetry uses the shared vocabulary in
`crates/sophia-protocol/src/capacity/` rather than a per-site record, so the
dedup rule and the field names cannot drift between resources.

## Allocation

The compositor hot path should not allocate casually. It may resize capacity at
controlled boundaries, but input processing and frame planning should reuse
buffers where practical.

Allowed edge allocations:

- connecting to a protocol authority or prototype server
- discovering outputs
- creating or destroying surfaces
- resizing dense tables
- capturing test snapshots
- portal transfer setup

Suspicious allocations:

- every input event
- every damage region merge
- every frame for stable surface lists
- every hit-test walk

## Protocol Authorities

Protocol authorities are compatibility boundaries. They own protocol parsing,
client resource tables, protocol-local IDs, focus/grab/selection semantics,
configure/commit state, and namespace enforcement for their clients.

Authority code must not own:

- physical input devices;
- compositor scene graph or scanout;
- workspace/layout policy;
- compositor chrome;
- cross-namespace portal policy;
- metadata sanitization or disclosure policy.

Authorities emit bounded surface transactions, metadata candidates, portal
requests, and lifecycle facts. Sophia Engine decides whether a visual transaction
is committed, delayed, rejected, or timed out.

Raw titles, classes, icons, PIDs, and paths are authority-private. They never
reach the WM, which must stay blind, and they never reach Engine, which receives
only reduced descriptors. They do not reach the metadata broker either.

An authority reduces its own metadata under a disclosure rule the broker
publishes, and emits the result. The split is deliberate: reduction belongs where
the data already is, because moving raw identity across a process boundary buys
nothing and costs a copy, a serialization surface, and a component that would hold
every client's identity across every authority. Policy belongs where it can be
consistent, so the broker still owns disclosure rules, trust assignment, icon
tokens, and aggregation — the facts an authority cannot decide alone without two
authorities disagreeing about what a user is looking at.

The reduced label is bounded and validated the same way by every authority, so the
only thing distributed is truncate-and-validate, not policy.

## XLibre Prototype Patches

XLibre changes are C and should stay narrow. They are prototype and research
work, not the long-term center of the architecture.

Patch goals:

- add explicit protocol seams;
- preserve X11 semantics;
- keep access control auditable;
- make changes upstreamable.

Avoid server patches that make Sophia the only possible compositor. XLibre
should gain a useful extension, not a private dependency.

## Verification

Docs-only changes need inspection, not a build.

For code, each component needs a concrete check:

- Rust units for packet validation and table invariants.
- Integration tests for XLibre bridge behavior.
- Headless compositor tests for frame plans.
- Portal tests for allow, deny, revoke, and stale-transfer cases.
- XLibre protocol tests for new extension behavior.

When a test cannot exist yet, document the missing harness in the research log
instead of pretending manual testing is enough.

## Warnings

`cargo xtask check` must finish with no compiler or clippy warnings, and a
change that adds one is not finished. Fix it in the change that introduced it.

Warnings accumulate faster than anyone reads them. A hundred standing warnings
is not a hundred small debts; it is a filter that hides the next real one,
because nobody scans a wall of expected output for the line that is new. The
cost is paid at the moment it becomes hard to see, which is long after the
moment it was cheap to fix.

Three resolutions, in order of preference:

1. **Fix the code.** Most warnings name something worth changing: a borrow
   that reads as a release, a type that wants a name, an assertion that
   belongs at compile time.
2. **State the exception at the site** with `#[expect(lint, reason = "...")]`.
   The reason says why the lint is wrong *here*, not that it is inconvenient.
   `#[allow]` without a reason is not acceptable; `#[expect]` also fails when
   the situation it describes goes away, which is the point.
3. **Record a threshold in `clippy.toml`** when a default is tuned for a
   different kind of code and the same argument applies across many sites.
   Every entry carries a comment saying what the code is doing deliberately
   and what would still be a smell past it.

Silencing a lint to make the gate quiet is the one resolution that is never
available. If a warning is wrong often enough to be annoying, say so in the
config with a reason; if it is wrong once, say so at the site.

The metadata-shell `launch.rs` test mount exercises private preparation and
post-negotiation state with real local socket handshakes. Its test body lives in
`tests/support/shell_startup.rs`; supplied protection evidence is not a native
supervisor proof. The exception permits that exact external fixture mount only.

The `seat.rs` mount exercises the private broker disable boundary with supplied
backend effects. It verifies pending leases never invoke disable, without opening
libseat or exporting test-only production APIs. Tests remain outside `src`.

The `native_owner_retirement.rs` mount exercises the private production retirement
owner with simulated worker/disposition effects. Its external fixture checks
held bytes, exact successor identity, and terminal error custody without exposing
a public completion constructor. It does not construct a native scanout, exercise
KMS, or establish device cleanup. The exception permits this mount only.

The external `presented_projection.rs` fixture retains the existing private
projection and geometry controls relocated from the production module. Its mount
does not expose projection internals publicly or change the tested transitions.

The external image-snapshot ownership fixture mounts beside private plane fields
so ordinary socket descriptors can check duplicate ownership and retained-source
lifetime without constructing an EGL context or exposing a public fake-image
constructor. It proves descriptor custody only, not DMA-BUF import or rendering.

The window-allocation publisher mounts an external shutdown fixture beside its
private pending acknowledgement and applied metadata. It drives the same passive
publisher and quiescence entry as production through real channels, without a
native scanout constructor. The exception permits that test mount only; it is
not a device, frontend-worker or full owner-loop execution.

The native launcher activation owner mounts an external fixture beside its
private pending reply and FIFO. It checks reservation before intake, exact
outcome retention through returned refusal/close, and partial-write charges.
Connection/request facts and write completion are supplied; this is not kernel
backpressure or launch execution. The Session queue tests use the public native
service with actual private sockets and need no private production test mount.
