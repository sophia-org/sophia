---
id: j8jkd97a
date: 2026-10-01
kind: investigation
status: investigating
tags: [investigation, output, 9p]
---
# Retire the output socket after native file-role acceptance

## Question

After t253's native acceptance, can the output IPC adapter be removed while
preserving ownership, rollback and WM restart independence?

## Evidence

The prerequisite is [t253 acceptance](../milestones/ofard23a-accept-the-revision-1-output-file-role-through-native-rollback.md)
at Sophia `ddd27bd6d9ac6d8e73394d9326705a7a62916f35`. Its physical evidence
remains bound to that source; this retirement requires its own qualification.

Signed contract C is `b0721d0de6a03cb44e57b0a923c6c385cf40b676` on
`integration/t272-output-files`. It changes the strict WM API to
`sophia-wm-files version=1 output_transport=9p2000.L\n`. C deliberately remains
off master: its vendored SDK 0.3 WM client expects `current_ipc`, so C alone
fails its own WM bootstrap. The assembled candidate with SDK 0.4 is coherent.

C SDK 0.4.0, signed commit `497e7e01531415078a4a3da2455ebe82ec18fd0e`,
imports C's WM and output contracts verbatim. The other nine imports are
unchanged. Sophia's new vendor manifest is
`ab45a6460bef0606210c9dd3da168dafdd9f3f902a639a3cdfaf6bc3945bf5af`.
The archive and raw signed commit bind the SDK source; the imported trailing
blank line in PROVENANCE is preserved rather than patched locally.

Evidence is under `~/.local/state/sophia/development-evidence/`:

- `ipc-retirement/t272-retirement-01`: production and neutral regression
  checks, first failures, strict Clippy and stale-epoch negative control.
- `t272-sdk-040-01`: 17 SDK programs, exact API refusals, and the mutant
  accepting both strings that fails the new negative.
- `t272-session-test-ports-01`: local reload and profile-only Session ports.
- `t272-hagia-strict-api-01`: external WM client's strict API probe.
- `t272-hagia-repin-01`: exact SDK archive import and the external client's
  policy, model and runner gates.

## Finding and resolution

The output file service now owns the only runtime output endpoint. Session
creates it only for an explicitly configured protected output process. With
no such process, the existing native bootstrap still creates the profile's
authority and startup transaction, with no listener. This does not change the
existing requirement for a supervised public WM to create that bootstrap.

The WM receives neither an output path nor an OutputAuthority grant. Automatic
and control WM restarts leave the output authority, epoch, transaction and
separate supervisor alone. Tests cover both pending and dispatched startup
effects across both restart paths. These tests supply the bootstrap and
dispatch; they make no physical apply claim.

Deleted production surfaces are the output IPC encoder/decoder, message kinds
64–68, socket transport and worker, schema, generated wire table and golden
frames. Retained OutputV1-named passive types carry revision-1 domain values;
they expose no socket framing. Unknown legacy message kinds fail decoding.
The shared control IPC envelope remains outside t272's scope.

Neutral coverage survives in these owners:

| Retired coverage | Retained or added coverage |
| --- | --- |
| Socket candidate codec and topology examples | Native output file codec, topology, and seven conformance candidate cases |
| Admission, latest replacement, replay and abandonment | OutputConnection plus file admission, journal and export suites |
| Malformed connection and partial-frame pause | Real 9P malformed-peer isolation and partial-request pause tests |
| Old epoch settlement after reconnect | Worker test reuses the same transaction on a new epoch; old settlement cannot settle it |
| Zero/future epoch owner errors | Worker rejects them without retiring the current proposal |
| Reload settlement stays local | Connected observer receives no Outcome; the same connection admits its next proposal |
| Combined WM/output restart | Separate protected output-process test and profile-only authority tests |
| Apply, restore and peer-loss recovery | Existing generic C SDK peer, protected Session fixture and native gate |

The obsolete startup-only `--output-proof-rollback-after-apply` control is
explicitly refused. The independent file peer-loss hold and readback remain.
The old niltempus frame-fed runner and its archives therefore belong to their
historical source pairs; its launcher must refuse post-retirement pins.

## Validation and remaining work

The targeted protocol/runtime checks, seven neutral conformance cases and
Session output tests pass. The stale-epoch mutant reaches ESTALE instead of
being ignored, failing the expected test. First compile and fixture failures
are retained. Workspace strict Clippy passes. A subsequent layout pass found
three missing declarations for external test mounts and two owner-loop
fragments over the source ceiling from t253. The mounts now have explicit
entries; authority service waiting and completion rollback were extracted
without changing their statements or order.

The clean signed `ae5a746c5` full gate reached a pre-existing shell fixture
race: `component_disconnect_settles_real_debt_without_disposing_pixels_or_neighbor`
expected Revoked after one service call following SDK-handle drop, but observed
Connected. The focused repeat passed. The fixture now services until the owner
observes EOF, under a two-second bound; it does not force revocation and retains
every debt, pixel and neighbor assertion. No shell production code changes.
Logs `18-signed-full-check.log` and `19-shell-reconnect-focused.log` preserve
the original failure and repeat. Log `20-session-eof-wait.log` exposed the same
single-visit assumption at another fixture site. The shared bounded wait now
covers all five asynchronous disconnect sites, including the budget cases.

Log `21-session-eof-waits.log` exposed the previously recorded descriptor
startup fixture failure: its negative path launched the supervisor's stale
`--serve` arguments instead of the configured child test. The fixture now
prepares and replaces the launch specification as production does, then
requires the child's successful exit. It also requires `UnsupportedRevision`
from the content-only parent, so a timeout cannot satisfy the negative.
Log `22-session-fixture-repairs.log` records 738 passing Session tests before
that exact-refusal assertion; `23-session-exact-refusal.log` records its check.
These follow-up changes repair test fixtures only.

The signed follow-up `ee3819e16` passed the Session library, then its full gate
(`24-d2-full-check.log`) exposed a parallel fixture directory collision.
`component_files` and `component_publication_files` each had their own counter
but used the same `session-component-files-{pid}-{counter}` path. The publication
fixture now uses its own prefix, so its creation and teardown cannot affect
the other fixture's sockets. `25-publication-directory-full-check.log` retains
the subsequent full check. This change also affects only a test fixture.
That check passed the repaired fixtures and reached the Rust SDK snapshot
tests, which correctly refused the stale imported WM contract. The Rust SDK
also imports the WM envelope and custody rules for its shell clients. Signed
Rust SDK `b38b809e1a09a62911dd940cfcc5067a7ee5b06d` refreshes that document
verbatim from C, its checksum and provenance, with no library or version
change. Its 324 tests and strict Clippy pass. Sophia vendors that exact signed
snapshot through `vendor-rust-desktop-sdk`; the contract equality check remains
required. Evidence is in `t272-sdk-rs-wm-doc-01` and
`26-rust-sdk-vendor.log`.

Log `27-final-full-check.log` then exposed a production ordering gap in the
output worker. After the old connection departed, Session queued Committed
settlement and topology publication. The worker consumed only the settlement
before accepting a replacement with its superseded snapshot. The publication
followed later, but the replacement's first fact was stale. A stale-base proposal
would still be refused; that effect safety did not satisfy bootstrap ordering.

The worker now accepts only after observing its command queue empty, with no
intervening command read. Already queued owner commands therefore precede the
replacement bootstrap. Commands arriving after that observation remain normal
subsequent updates. Live exports retain one-command-per-turn fairness. The
original Session test still requires the committed snapshot on negotiation;
the attempted fixture-only wait in log `28-replacement-publication.log` was
discarded. A deterministic worker regression queues settlement, publication
and a backlog connection before starting the worker, then checks the first
published object and complete topology. The worker and Session recovery checks
pass in `29-command-before-bootstrap.log`.
Removing the empty-queue accept guard in a private source copy and separate
target fails the new test deterministically: the first publication has topology
epoch 4 instead of 5 (`30-bootstrap-order-mutant.log`). Canonical source and
build artifacts were not used for the mutant.

Log `31-bootstrap-order-full-check.log` passed the output recovery checks but
found an older WM idle-test sampling race. The counter condition also held
while the Tflush-serving turn was still returning, after writing Rflush. The
fixture now identifies the next idle entry with the reply already on the wire,
releases its gate, and observes entry into the real idle call before measuring.
The same 50 ms no-spin assertion and Stop/bell assertions remain. A single
two-second deadline bounds the synchronization. The seven idle tests pass,
the repaired test passes 100 repeats, and three parallel Session library runs
each pass 738 tests. Replacing the real blocking turn with a zero-duration
turn fails the unchanged no-spin assertion. Evidence, including qualification
wrapper mistakes distinguished from source failures, is retained in
`t272-idle-fixture-01`. No WM production code changed.

Log `33-final-assembled-full-check.log` passed the tests, then strict Clippy
refused the new worker test's second mount of the raw 9P peer in the runtime
library test binary. Shell and output fixtures now share one test-only mount;
the peer implementation and assertions are unchanged. The source-layout
ledger records that external support mount.

The complete `cargo xtask check` passes in `34-shared-peer-full-check.log`,
including workspace and SDK tests, strict Clippy, source layout, wire checks
and six promoted direct-scanout archives. The device-hidden gate explicitly
does not establish new pixel or native output acceptance.

The assembled signed Sophia candidate, SDK 0.4 and repinned Hagia must pass
the clean-tree gates, fresh preparation/performance and attended native run
before promotion. Intermediate C remains reachable for SDK provenance. Rollback
restores the whole previous release: pre-C Sophia with its old WM and SDK;
mixing the old WM SDK with the new API intentionally fails bootstrap.
The reverse mix also fails: SDK 0.4 refuses the old API. Hagia's repin includes
a strict refusal through its real file client. Its obsolete pairing overlay
is labelled historical, remains pinned to `be6e5888`, and is not a t272 gate.

Startup rollback after physical apply is no longer an executable hardware
proof here. Supplied startup recovery and owner tests remain; the current
native peer-loss gate qualifies peer transactions, not startup transactions.
The native gate's existing one-card, two-head, refresh-only limits remain.
No revision-2 confirmation, persistence, docking or product launcher is added.

## Connections

- [Retirement inventory](1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md)
  owns t272's original scope and earlier coverage.
- [Migration plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
  defines the lockstep release and physical exit.
- [Output files](../../sophia-output-files.md) and
  [WM files](../../sophia-wm-files.md) are the normative contracts.
