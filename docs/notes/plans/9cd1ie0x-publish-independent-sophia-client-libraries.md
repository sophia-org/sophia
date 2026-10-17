---
id: 9cd1ie0x
date: 2026-09-26
kind: plan
tags: [plan, 9p, tooling]
---
# Publish native C and Rust desktop SDKs

## t263 — Publish versioned client libraries

Promoted by niltempus on 2026-09-27 under the
[accepted SDK decision](../decisions/gs6l7tuk-publish-native-c-and-rust-desktop-sdks-with-pinned-contracts.md).
Extract from merged master `987cfd39` (t252 source `79a09a302`). The first
release supplies the shell SDKs needed for t252 application adoption; it does
not wait for t252's attended acceptance.

Repositories: `sophia-org/sophia-desktop-sdk-c` (Codex) and
`sophia-org/sophia-desktop-sdk-rs` (Claude). Keep generic 9P separate from shell,
WM, output and administration modules. Shell ships first; releases declare
actual coverage. Use `libsophia-desktop` for the C library and reserve `-dev`
for distribution development packages. Neither SDK needs a compositor checkout.

## Scope and exit

- Inventory the Rust, C and Nim implementations and their consumers: Lom, Bemenu
  and Hagia. Preserve supported behavior and record each extraction's source commit.
- Nim can consume the C API through its foreign function interface, so a separately
  published Nim SDK is outside this plan. Under niltempus's later explicit ruling,
  Hagia uses thin C SDK bindings and retires its product-owned WM/9P transport.
  Its signed source tree vendors an exact verified SDK snapshot; builds must not
  resolve an ambient sibling checkout. Hagia retains its policy reducer and
  checkpoint/profile semantics.
- Define public APIs, ownership, licensing, versioning and the supported contract
  revisions. Publish independently buildable packages with examples and tests.
- Migrate consumers to pinned releases and verify each against the production
  exports, including negotiation, events, submit/ack, uploads where applicable,
  and revocation. Keep transport free of Sophia-specific role semantics.
- Keep authoritative KDL/specifications and server integration tests in Sophia.
  Keep the Go oracle independent of the SDK codecs so it can detect shared errors.
- Document dependency updates and compatibility checks across repositories.

Implementation and publication are authorized by niltempus's approved plan.
The application SDK, compositor portability and broker/portal migration remain separate.

## Unified role scope

Niltempus subsequently required one SDK repository per language to cover every
public WM, shell, output and admin/control operation formerly provided by IPC.
Use standard 9P2000.L with typed role files, ordinary operations and wire errors.
No private 9P opcodes, IPC tunnelling or fallback fills a missing contract. Modules
within an SDK share transport without merging role authority. The C and Rust SDKs
remain separate repositories, and Nim uses C rather than creating a third SDK.

For each role, map the old requests, responses, events, capabilities, resource
grants and terminal outcomes to file operations and named tests. Track absent
contracts separately from missing clients. A role needs server-export, C and
Rust SDK evidence before complete parity is claimed. Current output/admin gaps
remain explicit; a WM client reading `output_transport=current_ipc` in the pinned
api descriptor does not select or open that separate output transport.

Lom, Bemenu and Hagia are being made 9P-only by explicit instruction. Their
product IPC tests are retired with that wire, with required behaviour mapped to
file tests. Earlier rollback requirements below describe the prior stage; they
do not authorize reintroducing a product fallback. Existing SDK compatibility
archives remain transitional and outside the complete 9P target.

The [C WM checkpoint](../milestones/bzitjwp8-c-wm-sdk-codec-and-session-checkpoint.md)
records the signed codec/session slices and their test limits.

## t333 — Match the C and Rust public APIs

On 2026-10-16 niltempus required equivalent public APIs so applications can use
Rust directly or bind to C from another language. This extends the unified role
scope above; it does not reopen the completed first-release task t263.

Parity means matching capabilities and observable contracts, not identical
spelling, memory layouts or allocation strategies. Both SDKs must expose the
same supported role revisions, operations, events, errors, custody states,
revocation and recovery rules. Ownership, buffer lifetimes and bounded work
must be explicit. Idiomatic Rust ownership and C caller-owned storage remain
valid differences. Neither implementation may require a private protocol or
application-owned transport to fill a gap.

### Source audit, 2026-10-16

This inventory describes Sophia's pinned snapshots: Rust
`510a4349b4c62d6b32d21d7ae378423faa12ca10` and C
`ba09265d1642e9fc9e7919dfd6a634855057055c`. It is a source audit, not a new test
result or a claim about every upstream branch.

| Surface | C | Rust | Remaining work |
| --- | --- | --- | --- |
| Generic 9P, authenticated launch and namespace access | Public clients | Public clients | Map ownership, cancellation, limits and relay handoff operation by operation. |
| Shell | Codecs, session and native lifecycle helper | Codecs and shell client | Match revisions, admission, tickets, retained objects, uploads and native lifecycle operations. Presence in both is not proof of parity. |
| Event-loop integration | Descriptor/interests, caller time, timeout and byte-budget dispatch | `poll_io`, internal clock, `wake_deadline` and `wait_for_io` | Provide equivalent external-loop integration and document work budgets and clock rules. The Rust shell facade currently exposes no descriptor/interests API. |
| WM | Records and session client | No WM crate in the workspace | Add the native Rust client against the current WM file contract. |
| Output | Records and session client | No output crate in the workspace | Add the native Rust client against the current output file contract. |
| Lock | Experimental codec and client | Experimental codec only | Add Rust client coverage; retain experimental status until the role's separate acceptance gates pass. |
| Administration | No role client | No role client | Implement both against the accepted administration contract; absence in both is not completion. |

Evidence is in the pinned C `src/sophia_*_session.h`, `sophia_lock_client.h`,
`COVERAGE.md` and README, and the Rust workspace manifest and
`crates/sophia-shell-client/src/lib.rs`. C shell dispatch accepts monotonic
`now_ms`; Rust obtains time internally. The recent conditional Rust retry-clock
optimization therefore does not need a literal C port. Equivalent retry and
custody behavior still needs paired tests.

The Rust README's statement that WM and output modules await file contracts is
stale: the server and C clients already provide those interfaces. Correct that
upstream when adding coverage; never edit a vendored snapshot in place.

### Implementation order and exit

1. Publish a versioned operation matrix in both SDKs, tied to Sophia contract
   revisions. Name every operation, event and terminal outcome, its C and Rust
   entry points, ownership and test. Distinguish an absent server contract from
   an absent SDK client. Include broker and portal contracts as they become
   public; generic file access alone is not a typed role client.
2. Close shell integration gaps first: external-loop readiness, deadlines,
   bounded service, atomic admission and native lifecycle helpers. Keep
   convenience wrappers, but make equivalent control available in both APIs.
   Resolve differences in retry timing explicitly against the role contract.
3. Add Rust WM, output and lock clients in that order, using the existing C
   surface as a coverage checklist and Sophia's contracts as the authority.
4. Complete administration coverage in both SDKs with its migration owner.
   Audit the remaining public portal/broker operations before claiming full
   desktop API parity; do not fold their authority into another role.
5. Run matching scenarios through independent C and Rust clients against
   scripted peers and Sophia's production exports. Cover partial traffic,
   backpressure, same-ID retry, disconnect before and after custody,
   revocation, stale epochs, cancellation, buffer limits and terminal errors.
   Compare semantic outcomes, not language-specific call sequences. Compile
   a thin foreign-language binding smoke test to check the C ownership API.
6. Publish signed SDK revisions, refresh verified Sophia pins and developer
   examples, and run the existing independent conformance gates. Every matrix
   row needs evidence or an explicit unsupported capability in both SDKs;
   unsupported declarations cannot waive the accepted complete role scope.

The task closes when supported public desktop capabilities match, the paired
tests pass, and release documentation states the remaining experimental and
platform limits accurately. Physical desktop acceptance and performance gates
remain separate. This work precedes t289's broad engine optimization; it does
not change t331's current cost gate or the prepared CPU pilot.

### Current resumption

Submit-handle reuse is implemented in both integrated broker SDK sessions and
published as signed Rust `3830d84fba86ece78e195041d71e598bd5072659` and C
`d6f83ec67c934808221f473c12b5a195e68519ee`. Sophia imports those exact snapshots;
signature, tree and manifest verification pass. The session opens submit once
and reuses it; each transaction still gets a new assembly and waits for its
Submitted acknowledgement before clunk. Standalone transaction ownership is
unchanged. C consumers must rebuild against the matching headers because the
transaction storage fields changed.

Both independent socket peers require one submit walk across negotiation and
two responses and reject assembly cleanup before its receipt ack. All broker
Rust tests, strict broker Clippy and the full C check suite pass. Both revised
peers pass the real-export and protected-launch gates
(`/tmp/t333-submit-reuse-{c,rust}-export.log`). The rebuilt release CLI also
passes `protected_native_broker_preserves_all_operations_and_atomic_lowering`
(`/tmp/t333-submit-reuse-process.log`). No speedup is claimed yet: next compare
separate syscall traces, then untraced release latency and CPU. The full role/API
matrix and release audit still gate t333 closure.

Separate one-operation and ten-operation release traces now pass the strict
socket-byte parser. Their raw logs and reports are under the evidence directory
below in `trace-1/` and `trace-10/`. Joining each sample's peer PID and subtracting
the matched full-lifetime counts gives, per additional update, 420 bytes read
and 749 written by the native peer for all three label lengths. Legacy counts
are respectively 56/91, 74/109 and 178/213 bytes for absent, 16-byte and 120-byte
labels. All four matched cases per label agree. Native socket calls increase by
44.6–45.1 per update versus exactly three for legacy; calls include unsuccessful
nonblocking attempts. Startup and teardown are present in each raw trace, so
these differences are a diagnostic attribution, not direct interval counters.
Trace timings are not performance evidence, and internal copies remain unknown.

Source inspection identifies eight serial transaction operations in the Rust
broker SDK: transaction walk/open/write, submit walk/open/write, then both
clunks after receipt acknowledgement. Journal reads and acknowledgements add
traffic. The first bounded optimization candidate is a session-owned submit
fid: its server handle has no assembly state, unlike the transaction handle.
Review the pinned contract and independent C counterpart, retain immutable
retry and receipt barriers, and prove reuse through paired socket and production
export tests before adopting new pins. This is a candidate, not a measured fix;
the traces do not attribute the entire CPU delta to those three avoidable calls.

The first untraced release broker diagnostic at signed Sophia `313e0160c`
completed all 24 blocks (1,000 operations each) on October 17 at 02:52 UTC.
Evidence is in
`~/.local/state/sophia/broker-cost/313e0160c-20261017T025157Z/`:
artifact hashes and clean source checks, raw samples, process snapshot and log.
Median update latency across blocks was 84.7–94.2 µs for native files versus
6.6–8.4 µs for the legacy transport. Combined owner/peer CPU was 97.8–112.4 µs
per operation versus 8.3–10.6 µs. Correctness checks passed; source and artifacts
were unchanged. This is diagnostic evidence, not performance acceptance.
Socket traces and internal copy accounting remain separate and incomplete.
The next performance step is to attribute the extra native exchanges and CPU
cost before proposing changes; the final SDK role/API audit also remains open.

The Mac mini confirmed its quiet hold at 02:50 UTC and reported no crunch
activity before release at 02:52. Muse was visibly stopped at an interactive
permission prompt, without an explicit hold confirmation. Herdr rejected its
release message with `agent_blocked`; its permission prompt was left untouched.
The hold is over. Expected connection-reset messages came from deliberate
transport teardown outside measurement, after the descriptor checks passed.

The diagnostic now closes its transport and waits for process exit before
supervisor cleanup, outside measurement. This lets blocked reads return instead
of leaving syscall traces indeterminate. `cargo xtask check broker-socket-bytes
/absolute/trace.log` counts returned Unix-stream read/write lengths by task;
it handles short transfers, errors and interleaved resumed calls and refuses
incomplete traces. Three parser tests and strict xtask/harness Clippy pass.
The previous interrupted trace is a negative control and is refused. A new
one-operation smoke at `/tmp/sophia-broker-trace-complete.BwTbKc` completed and
parsed 25 socket tasks. Read and write totals differ by 132 bytes at teardown;
they must remain distinct, not be assumed equal or multiplied into a copy total.
The report includes startup and teardown and explicitly excludes internal
buffer copies. New workload records name peer PID and owner PID/TID for joins.
No performance qualification follows from these debug smoke traces.

Signed Sophia `ab5e8b7ba` is pushed. Release CLI and diagnostic artifacts are
recorded, without binary copies, in
`~/.local/state/sophia/broker-cost/ab5e8b7ba-20261017T024355Z/prepared.json`.
No timing qualification has run. The diagnostic now adds five empty-work CPU
observations before each CPU block, without subtraction or per-operation
counter reads. Strict harness Clippy and the two-operation smoke pass; the smoke
validated all 12 CPU observation sets and their absence from 12 latency blocks
(`/tmp/sophia-broker-observer-smoke.nPLrh3/samples.jsonl`). This harness change
requires a new benchmark artifact; the broker implementation is unchanged.

A separate one-operation strace smoke completed through the protected processes
(`/tmp/sophia-broker-trace-smoke.dsm0Kd`). Successful socket reads/writes can be
observed, including split and resumed syscalls. They establish a measurable
user/kernel boundary, not internal buffer-copy totals. Any parser must account
for unfinished/resumed calls and short transfers, exclude startup or report it
separately, and reject incomplete traces. Do not count requested lengths as
transferred bytes or reinterpret this smoke as a performance result.

Final SDK pins are signed and pushed: Rust
`da0937fab757eec2cce37e38813761b875d84487` and C
`3bead0580870db1005b76db4f55631500fbf553f`. Each differs from the tested
implementation pin only by removing a trailing blank line in `ADMIN-CLIENT.md`.
Both snapshots were reimported and passed signature/tree/manifest verification;
no vendored source was hand-edited. The staged diff check caught these lines in
new files after the canonical check, so the upstream correction is docs-only.

The full `cargo-slot sophia cargo xtask check` now passes; the log is
`/tmp/t333-native-broker-repository-check-rerun.log`. The first run passed
workspace tests and strict Clippy, then found two layout false positives in the
9P core's test-only alias/readiness hooks. Narrow exceptions now identify their
external fixture; no test bodies moved into production source. The successful
rerun includes SDK integrity, all-feature workspace tests, strict Clippy,
layout/tooling checks and the canonical gate's offscreen render-node pixel
proofs. This does not establish broker performance or physical desktop acceptance.

The integration now connects `LiveMetadataBroker` to the native shared-core
driver and launches `metadata-broker-files-serve`. It keeps the protected peer
check, X-client route checks, retirement tombstones and issuer-generation checks.
Shells read descriptors and grants from the file owner; presentation fixtures
use a read-only sanitized view instead of constructing a legacy transport.
The legacy command remains for comparison and its existing smoke tests, not as
a live Session fallback. This is an implementation checkpoint, not an installed
desktop update or task closure.

Validation: the protected live-wrapper test
`protected_live_broker_routes_and_retires_native_metadata` passes with
`--features native-session --lib -- --ignored`; it covers real launch,
admission, wrong-route refusal, descriptor delivery, action invalidation and
retirement, including retirement before admission. The 24 `metadata_shell_tests`
and protected C descriptor/work-area presentation test pass. Session's native
build and strict library Clippy pass. The protected five-operation process test
also passes, including equal-generation acceptance, zero/older-generation
rejection and both atomic disclosure-lowering paths.

The process tests used the slot 1 debug executable SHA-256
`a8783403f40039d28a64a05a7052eb90309240a115a2738e08afb9ba03e2d93b`;
the live-wrapper test executable was
`sophia_session-e50e790e77087e2d`, SHA-256
`4b0661bd319caee6c737a40a6c1feda94593907e87b8ea44a46fde16b6998c3f`.
These are protected, device-free checks, not physical desktop acceptance.

Next: complete t314's matched descriptor-workload comparison against the retained
envelope (latency, CPU and copied bytes), run the remaining replacement gates,
then finish the t333 operation-matrix audit and publish Sophia's verified SDK
pins and integration. Neither task is closed; no speedup is claimed.

`crates/sophia-session/tests/broker_performance.rs` now provides an opt-in
paired diagnostic using both actual protected broker commands. It runs an ABBA
sequence with absent, 16-byte and 120-byte labels; admission and warmup precede
measurement, and the final descriptor and grant are checked. Candidates and
sample storage are allocated before measurement. Latency and combined
owner/peer CPU use separate passes; the latter includes final receipt settlement.
A two-operation smoke passed all 24 blocks, writing
`/tmp/sophia-broker-cost-smoke.bec6r6/samples.jsonl`. This checks harness execution
only: debug build, no quiet window, no performance conclusion. The harness
passes strict Clippy. Copy accounting is explicitly null, and the result cannot
claim qualification. Before interpreting costs, add the copy-accounting pass,
account for CPU observation overhead, and bind the run to signed source and
release executable identities. Do not treat wire length as copied bytes.

### Broker migration checkpoints before live integration

The native `metadata-broker-files-serve` command now passes
`protected_native_broker_preserves_all_operations_and_atomic_lowering` in
`sophia-session/tests/broker_process.rs`, using the real protected process and
Session's file owner and driver. The test covers all five operations, equal
generation acceptance, zero and older generation rejection, stable grants,
attention no-change, atomic disclosure lowering and retirement. The original
test incorrectly expected an equal generation to be stale; the reducer accepts
it. Correcting the expectation required no production behavior change.
The CLI also passes strict Clippy with `native-session`. The tested executable
is the slot 1 debug build, not an installed desktop. Next, connect
`LiveMetadataBroker` to this driver and update its descriptor fixture; its
production transport is still legacy. Performance comparison remains pending.

As of 2026-10-16, t333 remains open. C and Rust broker sessions pass 12
independent socket schedules, including failure custody, explicit retry, retained
labels and both atomic two-row response forms. This is SDK evidence, not
production qualification. Portal requester production evidence is recorded below.

The immediate work is t314's server migration. New native codecs in
`crates/sophia-protocol/src/broker_files.rs` and `broker_files/request.rs` cover
Limits, handshake events, all five BrokerRequest kinds, Negotiate, submit and ack. Native ResponseSet decoding now checks both rows,
reserved fields, UTF-8, grant generations, request correlation and all five
request-specific rejection sets before returning borrowed rows. No allocation
is needed for decoding; current owner state and effect capacity remain unchecked.
Nine literal/boundary tests and strict library Clippy pass. Request encoding uses
fixed storage and preserves absent/empty/control/UTF-8 labels and raw zero domain
values. The current protected legacy transport remains active.

Runtime custody now uses the shared 9P journal in
`crates/sophia-runtime/src/broker_files.rs`. Five tests and strict library Clippy
pass: atomic handshake publication, immutable retries, receipt-ack release,
refusal draining, one pending request, request deadlines after ack, and response
preparation with no effects or receipt on failure. An accepted retry never calls
the preparation/commit hooks again. Request publication reserves the future
receipt's space and identities. The owner supplies its request-ID watermark when
creating replacement connection custody; this state does not authenticate peers.

The new `broker_files/filesystem.rs` implements the shared Export trait.
Two direct export tests and strict library Clippy pass: protection-role evidence,
exact peer/single attach, single-writer bounded staging, immutable accepted bytes,
ack-stale transaction reads and fresh Qids. These fixtures supply trusted evidence
and do not prove kernel socket admission or production integration.

C and Rust broker peers now pass the shared-core export gate
`cargo test -p sophia-runtime --test broker_sdk -- --ignored`, with
`SOPHIA_BROKER_SDK_PEER` naming each independent executable. Both exercise native
startup, negotiation, single-row retirement and two-row disclosure responses,
acknowledgement and cleanup. The owner checks one atomic effect commit. The
socketpair fixture uses synthetic protection evidence; it does not prove protected
launch or foreign-process admission. Production selection remains legacy.

The new `external_sdk_protected_broker_lifecycle` gate also passes for both SDKs:
actual ProcessSupervisor/Bubblewrap evidence, exact kernel peer admission,
negotiation, retirement and two-row disclosure, receipt ack and cleanup. The
original socketpair gate still passes. Peers use an explicit supplied socket
path for protected launch, never fallback on failure. Production reducer and
live Session integration remain separate. Run the `broker_sdk` command above
with `SOPHIA_TEST_BUBBLEWRAP=/home/linuxbrew/.linuxbrew/bin/bwrap` as well.
Current executable SHA-256: C `5bafac353d5f1d64642412627a3b02f737d7edd0e04384164146ec19cc3e60c9`;
Rust `b10603e16a7a8885bc30b7167c538a8b5a8b1e2a0e45ba8d469b88307fb5dd58`.
Strict peer/harness Clippy and C compilation pass. SDK sources are published
below; Sophia's harness remains uncommitted.

`broker_files/service.rs` now binds an owner-only role endpoint, requires
MetadataBroker protection evidence, verifies kernel UID/PID, and adopts the single
admitted socket into the shared-core server. Turns are bounded by custody and
staging deadlines. Four service tests and strict library Clippy pass, including
a compiled foreign process rejected before attach and endpoint cleanup.
Discovery uses `SOPHIA_BROKER_9P_SOCKET` without fallback.

The opt-in `broker_protection` test now passes with actual ProcessSupervisor
evidence and Bubblewrap 0.13.0. The exact protected child attaches and reads the
native API; it checks absent desktop environment variables, hidden input/GPU
devices and a host marker, plus a distinct network namespace. This qualifies
protected admission/file access, not the reducer or full SDK exchange.
Run with `SOPHIA_TEST_BUBBLEWRAP=/home/linuxbrew/.linuxbrew/bin/bwrap cargo-slot
sophia cargo test -p sophia-runtime --test broker_protection
protected_broker_is_admitted_from_real_supervisor_evidence -- --ignored --nocapture`.
Bubblewrap SHA-256: `e4abc70832b1ee7099e58800d714a5888056931c8b313c13c7a9731cbfee9d82`.
Test executable SHA-256: `036958309106f4917132f83b275fe427ec89b47721e0b16027e5e82fcf6447ce`,
at `~/.cache/cargo-slots/sophia/1/debug/deps/broker_protection-85f64e947f76fdad`.

The broker reducer now accepts all five native file requests without truncating
its command list. Three tests cover disclosure lowering, readmission with the
default class-only policy, label removal on later attention updates, retirement
and invalid admission; strict library Clippy passes. Both disclosure lowering
and readmission can require a rule plus a descriptor. The legacy CLI still
rejects multiple commands; the new entry point awaits production wiring.

Engine's descriptor table now exposes nonmutating `prepare_metadata`; Session's
current broker path uses it before committing the descriptor and action grant.
The 18 chrome/session tests pass, including abort, stale generation, invalid
surface and label cases. Preparation moves the label's existing allocation;
the pointer check proves that narrow property, not an end-to-end speedup.
The native-session build check passes.

`sophia-session::broker_responses` now prepares whole response sets against
current admission, retirement, rule, descriptor and grant state. It rejects
changed grants, stale replacement generations and a lowering rule without its
replacement descriptor. Five tests cover all request kinds, two-row failures,
absent effects on failure, explicit rejection/NoChange and labels that pass the
wire codec but fail Engine presentation checks. The prepared result feeds
`broker_file_owner`, which owns routes, rules, descriptors and grants and reserves
one downstream completion before publication. Its runtime hook commits the whole
result. Seven Session tests now pass, including a custody retry that commits only
once, a full completion slot, route mismatch and a bad second row that changes
no state and latches failure. The service closes custody when that owner fails,
without waiting for client traffic; four service tests and two export tests pass.
The live process still needs to use this owner and drain its completion;
production selection remains legacy. Default-feature library Clippy encountered the existing
missing native EGL renderer dependency; strict native-session library Clippy
passes.

`broker_file_driver` now provides bounded negotiation, request publication and
completion delivery around that owner and the shared service. It drives receipt
acknowledgements between calls, retains request IDs and closes on any terminal
error. Two socket tests pass: a complete native admission/receipt exchange and
stalled startup with immediate socket closure. Strict native-session library
Clippy passes. The fixtures use synthetic evidence, not a protected launch.

Next wire this driver into the live Session and the native broker process,
qualify the full protected broker exchange, and compare latency, CPU and copied bytes with the
legacy workload before retirement. Reconcile the full t333 role matrix before
signed publication and pin adoption.

Both SDK candidates are signed and pushed: Rust
`1c3a00d432de3bee405c9bff779945365da13a38` and C
`5de85f2e5a8e314248e58bc79b5529e43df89cce` (implementation `4daa707`, followed by
the shell contract reconciliation). Sophia's working tree now imports both exact
archives; signature, tree, manifest and contract verification pass. The C import
first refused an older shell specification and rolled back; the correction was
made upstream and signed before reimport. Full Rust workspace tests and strict
all-target Clippy pass; full C `make check check-binding`, changed C formatting,
Rust formatting and pinned contract hashes pass. README and provenance now name
the portal and broker clients and retain their qualification limits. Logs are
`/tmp/t333-sdk-rust-tests.log`, `/tmp/t333-sdk-rust-clippy.log` and
`/tmp/t333-sdk-c-tests.log`. The owner's unlock allowed publication. Sophia's
integration changes and updated pins remain uncommitted; the installed desktop
is unchanged. The native-session build check passes against the updated pins
(`/tmp/t333-sdk-pin-check.log`). Continue with the native broker process and live
Session wiring.

### Earlier broker and requester checkpoints

Production broker foundation (2026-10-16): `sophia-protocol::broker_files`
now encodes native Limits, Negotiated, Refused and Submitted records and decodes
Negotiate directly from the designed KDL. Three independent literal/boundary
tests and strict library Clippy pass. Unsupported revision ranges remain valid
candidates for semantic refusal; malformed envelopes are rejected. No SDK code
or IPC envelope is imported into these codecs.

Production tracing confirms `LiveMetadataBroker` uses the protected legacy
`MetadataBrokerSessionTransport`; the migration must preserve its protection
evidence and exact peer admission. Its single-response API also needs atomic
response-set handling. Next: native BrokerRequest/ResponseSet codecs, bounded
export custody on the shared core, protected service and broker process wiring,
then both SDK production gates and the t314 comparison measurements. The new
handshake codecs alone do not implement an export. Production selection is
unchanged; all current work remains uncommitted.

Broker response checkpoint (2026-10-16): both SDKs pass 12 independent
socket scenarios. New coverage preserves a pending label containing NUL, newline
and multibyte UTF-8 after journal consumption. Both SurfaceAdmitted and
SetDisclosure two-row responses reject a bad second row locally, then accept the
corrected pair with submission ID two. The peer receives one complete 448-byte
candidate, not separate row submissions. Strict Rust Clippy passes.

Next: t314's production broker export and runtime migration. Its acceptance
requires every existing broker path through files, no legacy production caller,
a compiled foreign-attach negative, and latency/CPU/copied-byte comparison before
legacy retirement. The owning plan is `jsschoen` section t314; existing Session
integration is `crates/sophia-session/src/live_session/metadata_broker.rs`.
The client fixtures do not prove those server properties. Changes remain
uncommitted, and the installed desktop and SDK pins remain unchanged.


Broker deadline, loss and retry checkpoint (2026-10-16): both SDKs now
pass nine independent socket schedules. New cases prove that an acknowledged
request still expires at its response deadline, loss after response submission
leaves custody Unknown, and explicit EAGAIN retry preserves every candidate and
control byte. The deadline test first drains the journal ack and prior cleanup,
so an unrelated handshake or ack timer cannot satisfy it. Strict Rust Clippy
passes. Next: retained-label and two-row integrated response coverage, then the
production broker migration and its protected-admission and measurement gates.
No production qualification is claimed; changes remain uncommitted.


Broker session failure checkpoint (2026-10-16): both independent socket
fixtures now pass six schedules: the full lifecycle, stale receipt epoch,
contradictory negotiated limits, refusal through terminal acknowledgement, short
ack reply and wrong receipt ID. Stale or uncorrelated receipts leave custody
unknown; errors after a valid receipt preserve Submitted. Both sessions stop
being ready on failure. C callers close the borrowed fd; Rust closes its owned
connection. Test peers accept EOF/reset and broken pipe during deliberate closure;
all protocol and custody assertions remain required. Strict Rust Clippy passes.

Next: response timeout and connection-loss schedules, immutable EAGAIN retry,
label retention and two-row integrated responses. Production export, protected
admission and comparative measurements remain open under t314. No production
qualification or publication is claimed; all changes remain uncommitted.


Paired broker session checkpoint (2026-10-16): C `sophia_br` now supplies
the integrated session counterpart to Rust `session::Session`. Both retain one
request in fixed storage independently of journal consumption, route all replies
through one connection driver, encode responses once, and wait for receipt ack
before transaction cleanup. C borrows the transport and requires fd closure on
terminal failure; Rust owns it. Neither reconnects or replays work.

Independent C and Rust socket fixtures pass negotiation, request retention,
a 448-byte response, acknowledgement and both transaction clunks. All 16 Rust
broker client tests and eight protocol tests pass. C's new session compiles under
strict warnings and passes its lifecycle test. These fixtures do not yet qualify
failure behavior or the production export. Next: matched refusal, loss, stale
epoch, deadline, malformed-event and two-row session schedules. Then t314 still
requires the production migration, protected admission and comparative measurements.
Changes remain uncommitted; SDK pins and the installed desktop are unchanged.


Rust broker session checkpoint (2026-10-16): `session::Session` now owns
the pipeline and routes startup, journal, ack and transaction replies. It copies
one pending request into fixed storage so journal consumption cannot invalidate
its label; response construction encodes borrowed rows once. Dispatch shares a
bounded I/O budget. A literal socket fixture passes negotiation, request delivery,
response submission, acknowledgement and both transaction clunks. Strict Clippy
passes. This is one happy-path fixture, not production qualification.

The C integrated session is still missing. Its independently tested startup,
journal, transaction and ordering components are ready to wire together next.
Do not claim broker session parity, publish this capability or adopt SDK pins
until C implements it and both pass equivalent fault schedules and production
export gates. Rust also still needs refusal, loss, stale epoch, response deadline,
malformed event and two-row integrated tests. t314's export, protected admission
and comparative measurements remain open.


Broker ordering checkpoint (2026-10-16): C `sophia_bp` and Rust
`phase::Ordering` now check handshake receipt ordering, negotiated limits,
one pending request, increasing request IDs and response receipt correlation.
Refusal keeps a fixed deadline from local observation; acknowledging only the
preceding receipt cannot extend it. The server owns publication-time enforcement.
Paired tests and strict Rust Clippy pass. This component assumes journal and
transaction validation; it does not own request data, socket I/O, response timers
or ack/cleanup pacing. Those remain work for the integrated session. Production
migration, protected admission and measurement gates remain open under t314.


Broker transaction checkpoint (2026-10-16):

Broker transaction drivers now stage Negotiate and complete 448-byte
ResponseSet records, submit explicitly and clunk both fids after receipt
acknowledgement. C `sophia_bt` and Rust `transaction::Transaction` use fixed
candidate storage and borrow operation bytes; they neither poll nor own the
transport. One session driver must match tags and close on terminal failure.
The original deadline includes cleanup. The assembly deadline starts when the
first write enters the transport and never extends with progress. EAGAIN waits
for explicit retry of the same control. Connection loss preserves known custody.

Thirteen Rust broker client tests and the independent C transaction suite pass,
including fragmented staging for both record sizes, retry, receipt/write ordering,
acknowledgement before release, assembly expiry, disconnect and failed or timed-out
cleanup. Strict Rust Clippy passes. These are component tests, not production
export evidence. Next: integrate startup, journal and transactions into the broker
session, then qualify against t314's production migration and admission gates.


Broker startup checkpoint (2026-10-16):

Both SDKs now discover broker metadata on the caller-admitted root: Rust
`startup::Opening` and C `sophia_bo`. They retain the connection, read exact API
and 72-byte Limits records through EOF, and require successful clunks before
handoff. Fixed metadata storage and bounded dispatch avoid per-fragment role
allocations. The caller retains the admission authority channel; discovery does
not authenticate or negotiate. Rust owns and closes failed connections; C borrows
the wire and requires its caller to close the fd on failure.

Paired socket fixtures cover seven-byte fragments, one-byte I/O budgets,
incorrect version/epoch, directory replies, truncated/trailing records, failed
clunks, deadlines and clock regression. Rust's nine broker client tests, the C
startup suite and strict Rust Clippy pass. Transaction I/O, session integration,
t314's production export, protected admission and comparative measurements remain
open. No SDK pin or installed desktop changed.


Both requester sessions now admit subsequent requests and route Decision and
terminal outcomes through retained, connection-scoped request handles. Fixed
storage holds 64 grants plus one pending/denied request. Terminal results require
acknowledgement before release; duplicate-request denial preserves the original
grant. Independent C/Rust socket tests pass allowed/denied requests, duplicate
denial, revocation, mismatched broker generations and premature Completed
rejection. Strict Rust Clippy and the C build pass.

Both sessions now submit Cancel only for an Allowed grant before End issuance.
They retain that observation until the correlated Revoked event, reject repeated
local cancellation and release terminal state only after acknowledgement.
Independent socket tests pass cancellation and issuer-scoped request handles.
Rust handles use a unique SDK issuer; C handles include their immovable session
address. Equal wire epochs cannot substitute another live session's handle.
Rust's four session tests and C's eleven schedules pass; strict Clippy passes.

Both SDKs now have standalone upload writer components, Rust `upload::Upload`
and C `sophia_pqu`. Independent tests pass empty/one-byte/64-KiB payloads,
iounit/msize bounds, retained writer lifetime, original-buffer identity, partial
walk, short write, ESTALE, deadline and failed-clunk cases. Strict Rust Clippy
and C builds pass. Rust owns the supplied Vec; C borrows immutable caller storage.
Neither allocates or copies a separate payload buffer. No measured speedup claim.

Rust now integrates Begin/upload/End in `requester/session/upload.rs`. Its fifth
session test passes empty and 64-KiB payloads, exact End length, writer retention
through End, Completed routing and return of the original Vec. Upload requires
four request slots and six fids; handshake/request-only minimums remain three
and five. Both standalone upload components now drain one outstanding write and
clunk after correlated terminal settlement, including ESTALE without replay.
Those component tests and strict Clippy pass. Rust Begin currently consumes the
Vec on admission error, matching the existing owned-buffer convention; retain
this ownership limitation for the final API review.

The C session now also integrates Begin/upload/End and transaction readiness.
Both SDKs pass empty and 64-KiB uploads and cancellation with the completed
writer still open. Cancellation preserves Allowed until the correlated Revoked
event; writer cleanup then releases the payload storage. Rust's six session
tests and C's fifteen schedules pass independently.

Both sessions now pass Revoked before a pending upload write's ESTALE reply.
The peers withhold the reply until acknowledgement, reject another payload
write and verify writer clunk. Original storage is released with Revoked
retained. Rust's seven tests and C's sixteen schedules pass.

ESTALE without a terminal event now has paired failure controls. Both sessions
close, hide the grant, preserve the last Allowed observation and release storage
after connection closure, without replay or fabricated Revoked. Eight Rust tests
and seventeen C schedules pass. Connection-preserving recovery for ESTALE before
a later outcome remains an explicit ownership-audit question.

Both independent SDK peers now pass the real PortalFileExport lifecycle gate:
completion, executor failure, denial and cancellation, each with empty and
64-KiB payloads. The harness verifies actual executor bytes and counts, with no
execution for denial/cancellation. It supplies private admission and test owner
authority; no desktop or recipient frontend runs. Rust peer Clippy and Sophia's
17 ordinary portal-file tests pass. Both SDK parity files record executable
hashes and uncommitted base revisions.

The production gate now also covers disconnect before End and loss of the
outcome after execution custody, for twelve cases per SDK. Before End, nothing
executes. After custody, the executor runs once but both clients retain
disconnected EndIssued rather than fabricate a terminal result. Both release
payload storage; the harness drains transport retirement and rejects late work.
Updated executable hashes are in the SDK parity documents; Rust Clippy passes.

Both peers now cancel a partially acknowledged 64-KiB production upload. A
one-byte dispatch budget exposes that some but not all bytes are acknowledged;
both require Revoked and zero executions. Thirteen production cases per SDK
pass. This does not control the ordering of a withheld write reply.

The SDK portal parity documents now map requester entry points, ownership and
evidence. Integrated End EAGAIN/retry now passes empty and 64-KiB uploads in
both peers. Retry is explicit; the staged candidate and submit control remain
identical, and one Completed follows. Nine Rust session tests and nineteen C
schedules pass. EndIssued remains conservative across EAGAIN; cancelling a
refused End on the live connection is not provided. Next resolve failed-admission
buffer ownership and terminal-error recovery in the release audit.

Failed-admission buffer ownership is now addressed by Rust `begin_buffer`,
which takes the Vec only after validation. Rejection preserves its original
pointer, bytes and capacity; admitted storage returns through `take_upload`.
The consuming `begin` remains a convenience wrapper. C already borrows caller
storage and needs no implementation change. Paired scripted tests cover rejected
deadlines, successful reuse and a busy second Begin. Both production peers pass
all thirteen cases with rejected Begin followed by reuse; strict Rust Clippy
passes all portal-client targets. Latest executable hashes are in parity docs.
The recovery audit now documents matched fail-closed behavior: neither SDK
reconnects/replays; uncorrelated ESTALE closes without inventing an outcome,
whereas terminal-correlated ESTALE drains and cleans up. EndIssued remains
uncertain after disconnect. Live cancellation of a refused End is still absent.

C's new `make check-binding` passes a thin Nim import of public structs and
functions against the native static libraries. It checks empty/one-byte/64-KiB
borrowed buffers, pointer identity, bounded writes, held lifetime and clunk
release without Rust or Sophia. This satisfies the foreign-binding smoke, not
a complete Nim client. Full C `make check` and both Rust portal crates pass.
Next reconcile remaining role coverage and t314's broker dependency; the latter
still requires its server migration, protected admission and measurements.

Broker SDK discovery now has independent C/Rust implementations, with the KDL
pinned from signed Sophia `519f07fd1`. Exact API identity, canonical epochs,
Limits bounds, reserved fields and truncation controls pass; C preserves output
on failure, and Rust Clippy passes. This does not implement the export or client.
Next implement broker request/response codecs and correlation against that
design, then the bounded client/server lifecycle and t314 integration gates.

Incoming broker events now decode independently in both SDKs: handshake,
custody and all five request kinds. Labels borrow the input and preserve
absent/present-empty/control text; raw zero surface/generation values remain
available for domain rejection. Literal, truncation, unused-field, boolean,
UTF-8 and envelope tests pass, with output preservation in C. Four Rust tests
and both C broker suites pass; strict Clippy is clean. ResponseSet encoding and
pending-request correlation are next; no export/client parity claim yet.

Both SDKs now encode atomic ResponseSet records and check request-visible
correlation. Independent tests cover all five request kinds and rejection sets,
both two-row lowering paths, present-zero icons, label distinctions and grant
generation mismatch. Invalid second rows leave all output unchanged. Six Rust
tests and three C broker suites pass; strict Clippy passes. The server must
still validate retained owner state and prepare both effects atomically.
Next complete negotiation/submit/ack codecs, then bounded broker client custody.

Negotiation, submit and cumulative ack encoders now pass independent literal
and boundary tests in both SDKs. Unsupported revision ranges are encoded for
server refusal; invalid IDs, lengths and short buffers preserve all output.
Eight Rust tests and four C broker suites pass; strict Clippy passes. The
client-side record foundation is present, but typed client custody, journal,
startup and transport integration remain. Next implement bounded immutable
submission custody, keeping broker authority separate from portal/session roles.

Both SDKs now retain immutable broker submissions in fixed storage. Four Rust
custody tests and the independent C suite pass explicit retry, both receipt/write
orders, acknowledgement barriers, short/failed replies, clock regression,
deadlines and known/unknown disconnect custody. Incoming events retain epoch
for receipt correlation; C's unreleased event layout changed accordingly.
Eight protocol tests and strict Rust Clippy also pass. Next add the bounded
retained journal, then startup/transaction I/O and integrated broker sessions.

Both broker journals now use fixed 512-byte receive storage and retain one
borrowed event head. Three Rust tests and the independent C suite pass partial
and coalesced records, compaction, label pointer lifetime, immutable pending
acks, sequence/epoch errors, malformed text, clocks, deadlines and overflow.
Seven Rust client tests and strict Clippy pass. The observation-based ack timer
does not replace the server's publication clock or Refused drain deadline.
Next implement admitted-root startup and transaction I/O, then integrate the
session with negotiation, one pending request and atomic response custody.
Cross-role recovery/ownership, t314's design-only
broker export, foreign C bindings, signed publication and verified Sophia pin
adoption still block t333. Production pins and physical/performance acceptance
remain unchanged; no speedup is claimed.

The requester series and preceding shared-recipient changes are uncommitted.
Rust's base is `4c18605`; C's is `2d719d5`. The staged hashes in the component
checkpoint below predate the unstaged state/session additions. GPG signing
remains pending unlock after the earlier timeout; do not retry without a changed
condition or create an unsigned commit. No hold or build process is active.
Pinned requester contracts come from signed Sophia
`519f07fd1b6db3a665ae1e74368a3d2e234cbf9c`; both `PORTAL-PARITY.md` files record
the source hashes and exact implementation limits.

### Requester component checkpoints before request integration

Both SDKs now have unstaged per-request grant correlation: Rust
`requester::transfer::Transfer` and C `sophia_pqf`. Three Rust tests and the
independent C suite pass all grant-field mismatches, domain denial, disconnect,
premature completion and duplicate terminal outcomes. Strict Rust Clippy and
the C build pass. These fixed-storage components retain known observations;
disconnect hides grants without inventing terminal outcomes. Next integrate
request admission and correlated Decision/outcome routing into both sessions,
then implement the single upload slot. No production grant qualification yet.

Both unstaged requester sessions, Rust `session::Session` and C `sophia_pqr`,
now integrate negotiation, retained journal events, explicit cumulative
acknowledgements and transaction cleanup under one nonblocking connection owner.
Independent scripted peers pass normal and receipt-before-write negotiation,
refusal, wrong receipt and short ack at three request slots and five fids.
One-byte I/O budgets preserve retained events; readiness waits for acknowledgement
and cleanup. C also exercises discovery-to-session handoff. Strict Rust Clippy
and the C build pass. These sessions currently support the handshake only:
next add subsequent requests and correlated grants, then upload ownership and
terminal outcomes. Production requester gates remain open; no complete client
or speedup is claimed.

Both SDKs now have unstaged requester transaction operation drivers:
Rust `transaction::Transaction` and C `sophia_pqt`.
It stages immutable bytes in iounit-sized writes, submits an exact control,
waits for explicit EAGAIN retry, and retains both fids until the Submitted receipt
is acknowledged. Assembly and overall deadlines bound staging and cleanup.
Three Rust tests and the independent C suite pass ordered staging/retry/ack
cleanup, assembly expiry, disconnect custody and failed or stalled clunks.
All eleven Rust requester tests pass, as does C's transaction suite with UBSan
trap instrumentation. The ordinary UBSan link lacked its runtime; trap mode
passed without one. Next integrate negotiation, journal and transaction driving
under one connection owner. Neither SDK yet has that requester session or
production requester gates.

Requester discovery startup now exists in both SDKs: Rust
`requester::startup::Opening` and C `sophia_pqo`. Independent scripted tests pass
exact api/Limits reads, epoch correlation, partial traffic, successful cleanup,
root preservation and malformed metadata/deadline failures. Strict Rust Clippy
and the C build pass. These are unstaged changes with no production admission
claim. Next connect negotiation and operation transport to the existing
submission/journal state, then implement grants and uploads.

Rust and C now both have fixed-storage requester submission custody and a
retained journal with explicit cumulative acknowledgements. Six Rust tests and
an independent C suite pass receipt/write reply ordering, explicit EAGAIN retry,
uncertain delivery, fragmented/coalesced events, acknowledgement deadlines and
malformed input. Strict Rust Clippy and the C warnings-as-errors build pass.
These additions are unstaged; transport integration and production gates remain
required. The staged hashes below describe the preceding checkpoint only.

Trusted portal requester codecs now exist in both SDK worktrees, still staged
and uncommitted with the preceding shared recipient work. Rust's base is
`4c18605`; C's is `2d719d5`. GPG signing remains pending unlock after the earlier
timeout. Do not repeat signing attempts without a changed condition or use an
unsigned commit. There is no active hold or build process.

The Rust `sophia-portal-protocol::requester` module and C
`sophia_portal_requester_files.h` provide exact api/Limits decoding, all five
candidate encoders, all five event kinds, and submit/ack controls. The codecs
preserve lifecycle Denied and negotiation Refused inputs, optional UTF-8 MIME
and all seven transfer kinds. They use fixed record scratch, reject malformed
records and preserve output on failure. Three Rust tests and the independent C
suite pass literal-layout, bounds, padding, truncation and failure-output checks;
strict Clippy, C UBSan trap mode and both spec manifests pass.

Requester docs, KDL and referenced broker custody rules are copied unchanged
from signed Sophia `519f07fd1b6db3a665ae1e74368a3d2e234cbf9c`. Portal map 23 records
source hashes and the stale design-only KDL preamble; the newer requester doc
states the implemented private channel. The broker export remains design-only.
No requester transport, custody or production-export claim follows from codecs.
The preceding 22 recipient production gates were not rerun for this isolated
protocol addition. Staged binary-diff SHA-256: Rust
`d2a93fb7337f69cfb92e6310ebccb5a4455c3299b1cdaa3be582fa8badb2ff05`;
C `651bb5b3358892c70150ead2cba28633c7faba59dbc0febe5c256d077a510966`.

Next implement the admitted requester startup/session: exact api and Limits,
negotiation with immutable candidate/submit custody, retained journal and
cumulative ack, then grants, upload/0 and terminal outcomes. Reuse generic 9P,
not recipient authority; no ambient endpoint or permission bits may substitute
for the trusted connection. Pair independent SDK tests and production export
gates. Cross-role ownership/recovery audit, the t314 broker dependency, foreign
C bindings, signed publication and verified Sophia pin adoption still remain.
t333 is active; production pins and physical/performance acceptance are unchanged.

### Shared frame checkpoint

Recipient composition now supports one watch beside a snapshot, capture request
or frame read in both SDKs. Changes remain staged and uncommitted on Rust
`4c18605` and C `2d719d5`, together with the preceding capture work. Signing is
waiting for GPG unlock after the earlier timed-out Sophia attempt; no unsigned
commit or retry was made. Continue implementation while publication waits.

Shared frames reuse the exclusive operation state and preserve four concurrent
pixel reads, exact EOF, retained metadata recheck and both clunks. Six requests
reserve watch/flush capacity; four fids retain the objects. Rust validates bounded
input capacity against the existing 60 KiB read cap, reuses Vec storage and moves
the completed buffer. C borrows disjoint caller storage without allocation.
Portal map 22 documents APIs, lifetimes and the unreleased C layout change.

Both scripted peers require four reads before replying, reverse that first
batch, return short reads, cancel a watch during partial pixels and reuse the
same image address. Stale/changed metadata, excess data, premature EOF and clunk
failure hide results; C also checks overlap and insufficient capacity. All 22
Rust portal tests, affected strict C tests and strict Clippy pass. Both peers pass
all 22 production portal gates. The C peer's initial new-mode assertion used the
wrong expected capability string; it was corrected, then all gates passed.
Peer SHA-256: Rust
`ec6c329e9f4c8a39fa8d1a281ce45384bd3f5a89427f77162a7826f665299282`;
C `e47a5e05ff4333074ce347daefd762d31406152e5dad173cc8190ccee404d2b5`.
Staged binary-diff SHA-256: Rust
`0f6702b6037cf15590ac739ed1066c9b78ff4ed8e0c6bf17d1c124f6eea02141`;
C `b3f26eeafcdc09598287ac44f99d08c22a95059e2412aee569cb5891d48f1158`.
These identify local candidates, not published SDK revisions.

Next audit and implement the distinct trusted portal requester API against
`docs/sophia-portal-files.md` and its KDL. Its admitted private connection is not
an ambient endpoint or recipient capability. Finish cross-role recovery and
ownership evidence, foreign C-binding smoke, signed publication and verified
Sophia pin adoption. The task ledger explicitly makes t333 depend on t314;
`docs/sophia-broker-files.md` still marks that export and its clients design-only.
Do not claim full parity while that dependency lacks production evidence.
t333 remains active; no quiet hold, production pin update, physical acceptance
or measured performance claim.

### Shared capture checkpoint

Shared capture scheduling is implemented in both SDK worktrees, staged but not
committed: Rust based on `4c18605`, C on `2d719d5`. The previous Sophia checkpoint
commit timed out waiting for GPG; no unsigned commit was made. All three
repositories use the same signing key. Await unlock before committing/pushing;
implementation work may continue.

The shared session now runs a capture request beside a watch, reusing the
exclusive capture operation state. One foreground slot retains results until
release. Custody survives terminal failure; results do not. Wake scheduling
includes paced status snapshots without mistaking their wake for expiry.
Inline request/status storage and a single I/O driver preserve bounded work.
Portal map 21 lists the paired APIs and notes C's unreleased layout change.

Independent C/Rust tests pass concurrent watch cancellation, paced fragmented
status, explicit reuse, Busy admission and seven capture failure modes. The
existing exclusive tests pass after refactoring. Both external peers pass all
21 production portal gates, including replacement of a watched capture by a
new request. Strict Rust Clippy and diff checks pass. Peer SHA-256:
Rust `93afc9d820a72dcb1555115dea2ca9df7293cecea139d66eca129b82c86fd24b`;
C `7673d5d44dfdd3cb8ae70a49906fc5bdccdeb68e514074edfffe880b03a5f911`.
Staged binary-diff SHA-256 before this note: Rust
`9040a1c1ff28fa307cc367faf5136b6541a227f7266409e8572c6bc6e49f9c3a`;
C `a9870d810d73f7b97acb66ddeb12108680b0a6652a16e1de8ef67f8a54b89ad2`.
These identify uncommitted candidates, not published revisions.

Next integrate frame reads into the same scheduler without duplicating pixels
or reducing the existing bounded read window. Then finish the cross-role
ownership/recovery audit, trusted requester/broker gaps, foreign C-binding smoke,
signed publication and verified Sophia pin adoption. t333 stays active. No quiet
hold is in force; production pins and physical/performance acceptance are unchanged.

### Buffered cancellation checkpoint

Signed Rust `4c18605` and C `2d719d5` extend the shared-session suite to ten
scripted cases. Empty/payload watch reads during flush fail without Rflush;
cancellation before walk dispatch or during a held open reply avoids both the
watch read and Tflush. Foreground status work still completes, and cleanup
returns the original root.

A peer handshake supplies status and revocation together before cancellation.
Rust's exact read budget leaves revocation decoded but unconsumed; C routes
both replies in the same dispatch. Both preserve Revoked without flushing a
settled tag. A Rust immediate-flush mutation fails with "tag not outstanding";
the original bytes were restored and the test passes. Native C/Rust suites,
strict Clippy and diff checks pass. Portal map 20 records the evidence.
Production code, pinned contracts and Sophia SDK pins are unchanged.

Next integrate capture requests into the shared scheduler, then frame reads.
Extract the existing exclusive capture transport's operation state so both
entry points share custody, status pacing and deadline rules; route its tags
alongside the watch, with one foreground operation at a time. Qualify both
SDKs independently and through the affected production portal gates. Preserve
caller buffers and bounded work; do not duplicate payloads or claim unmeasured
savings. Recovery, trusted requester/broker, foreign bindings and publication
remain open. t333 is active; no quiet hold is in force.

### Flush custody checkpoint

Signed Rust `db1dbaf` and C `122f9d4` add matching independent shared-session
tests. Five cases force cancellation alone, watch revocation before Rflush,
failed clunk after revocation, an error answering Tflush, and disconnect during
flush. Successful cases preserve foreground status work and return root-only
ownership after explicit releases. Failures expose neither event nor snapshot.
Both check the exact old tag, one watch/flush, no second version/attach,
one-byte dispatch and harmless clock regression. Focused native C/Rust tests,
strict Rust Clippy and diff checks pass. Production code and pins are unchanged;
the preceding production gates were not rerun for these test-only changes.

Next force replies already decoded when cancellation is requested, malformed
watch reads during flush and cancellation before the watch read; then integrate
capture/frame work into the shared scheduler. Portal map 19 records the limits.
t333 remains open. No quiet hold or measured performance claim.

### Shared recipient session checkpoint

Signed Rust `4dc6e67` and C `31911c2` add a developing shared recipient
session: one watch alongside one metadata/transfer snapshot on the same
pipeline. Separate tags route replies; events and snapshots remain retained
until released. Transfer storage is borrowed and reused. Cancellation drains
decoded replies before flushing a pending watch read, then clunks its fid
without abandoning foreground work. Original deadlines bound cleanup.

Both independent production peers pass concurrent status access, watch
cancellation with a continuing transfer, and old-watch revocation beside a
replacement transfer. All twenty external portal gates, nineteen Rust
component tests and strict Clippy pass. Peer SHA-256: Rust
`3ce54e7af95e27f76f319c42a784a7b586b4691b8eefcde0b6ed19173f738e7a`;
C `70c27add8b2c8363b850b1ce83da59f2b8ee2eec822317be4cfbc299d80a5d7f`.
Portal map 18 records the remaining limits.

Next qualify deterministic flush/reply races and shared-session failures, then
integrate capture requests and frame reads into this scheduler. Production
scenarios do not prove every reply ordering. Reconnect/recovery, trusted
requester/broker, foreign bindings and publication/pin adoption remain.
t333 stays open; production pins are unchanged. No quiet hold or measured
performance claim.

### Watch failure audit checkpoint

Signed Rust `1957ab2` and C `b483632` add independent watch protocol-failure
and explicit-reuse tests. Empty/payload Rread, unrelated remote errors,
directory replies, disconnect and clunk refusal never report revocation.
ESTALE during open or read requires successful clunk. Two explicit watches
reuse the same root with one version/attach exchange; one-byte dispatch and
clock-regression checks pass. Both watch tests and strict Clippy pass.
Production code and the preceding eighteen-gate evidence are unchanged.

Portal map 17 records the result. Next replace the exclusive-operation
composition gap with a shared recipient session: watch and foreground work
must route replies by separate tags on one pipeline, retain independent object
identities, share bounded dispatch and support explicit cancellation without
discarding unrelated work. Qualify concurrent status/capture/transfer/frame
work, flush races and connection loss independently in C and Rust, then through
the production export. Reconnect must remain explicit, with no request replay.

Trusted requester/broker, foreign bindings and publication/pin adoption remain.
t333 stays open; production pins are unchanged. No quiet hold or measured
performance claim.

### Retained watch checkpoint

Signed Rust `460c461` and C `1b6aabc` add retained watch drivers. One nonempty
offset-zero read waits for ESTALE; successful clunk returns the original root.
There is no automatic reopening or inference about a separately fetched
transfer. Local cancellation/expiry close the owned role (C requires caller
fd closure), without claiming revocation or flush-based connection reuse.

All eighteen external production portal gates pass independently in both
languages, including watch lock/replacement/source-generation, absence, local
cancellation and deadline cases. Strict Rust Clippy passes. Peer SHA-256:
Rust `0929b69098eea8951a745ac50cb112a78e7b035ccc679f6dda371e7a71ec90f3`;
C `ebd10bdcad89f7f114b82136a7968c2dfded1053cfa663c5c663ef9f2675d0b2`.
Portal map 16 records the limits.

Next audit malformed watch replies/clunk failure and recovery, then compose
watch with other recipient operations without surrendering shared-role
ownership. Exclusive component drivers are not a complete concurrent recipient
session. Trusted requester/broker, foreign bindings and publication/pin
adoption remain. t333 stays open; production pins are unchanged. No quiet hold
or measured performance claim.

### Transfer transport checkpoint

Signed Rust `0d474bb` and C `778e550` integrate retained transfer reads with
authenticated transport. Both borrow caller storage, require EOF/clunk and
return the same root without a payload copy. The independent production peers
verify seven supplied bytes, the original buffer pointer and untouched tail.
Absent transfers preserve ENOENT; revocation after payload receipt but before
completion preserves ESTALE and hides scratch.

All twelve external portal gates pass independently in C and Rust, as do the
seventeen Rust component tests and strict Clippy. The 64 KiB limit is still
component evidence, not this seven-byte production gate. Portal map 15 records
ownership and limits. Peer SHA-256: Rust
`b74008e5df9efa3ef6ef54fa204e10c810ca622be568f5cd5406754a6241d82d`;
C `def40cbc2bac71705f6165ad3387cb31286a86eb59c4f597f2906c87457f4807`.

Next implement retained watch in both SDKs, then finish recovery and local
cancellation audits. Trusted requester/broker, foreign bindings and
publication/pin adoption remain. t333 stays open; production pins are unchanged.
No quiet hold or measured performance claim.

### Transfer component checkpoint

Signed Rust `5ca560f` and C `37c0138` extend the snapshot operation driver
to opaque retained transfers. Both borrow caller storage of at least 64 KiB,
leave unused bytes untouched, enforce the payload limit and expose bytes only
after EOF and clunk. No second payload allocation or copy is introduced.
Rust Fetch carries a storage lifetime; metadata remains inline. The driver
does not associate a separately walked watch with the transfer.

All seventeen Rust client tests, C transfer/snapshot/request tests and strict
Clippy pass. Rebuilt peers pass all nine existing production portal gates as
regressions; those gates do not yet exercise transfer delivery. Peer SHA-256:
Rust `d4a9393ab6f967d598e58541bb1fd55db2af998610633ac3a5ecb82d5b96e127`;
C `bf654a383b003e3e67da1a86339a4d788ae7a65a2ce627abbd624360db8243f4`.
Portal map 14 records the component scope.

Next compose transfer reads with authenticated transport and qualify delivery
and revocation, then implement watch and finish recovery/local-cancellation
audits. Trusted requester/broker, foreign bindings and publication/pin adoption
remain. t333 stays open; production pins are unchanged. No quiet hold or
measured performance claim.

### Capture refusal and cancellation checkpoint

Signed Rust `c0d99d2` and C `f4b3ec1` pass all nine production portal gates.
New gates qualify locked/busy/stale-serial refusal with exact EACCES/EAGAIN/ESTALE,
and cancellation after queueing or owner pickup. Both SDKs preserve Refused
versus Cancelled. The fixture rejects late completion after unlock and confirms
that no frame was installed. Cancelled status leaves a quiescent root-only role.
Strict Rust Clippy and the two ordinary portal-dispatch regressions pass.

Portal map 13 records the limits. Peer SHA-256: Rust
`47c05ca921892fc675f915122a453c77c5da6d9f2f2058d73a6a807f22fa949b`;
C `c6dbc61962a9ae2348f381d5ce24fca74069e2545ad0e5faab8964815a323402`.
Next implement retained transfer/watch APIs in both SDKs, followed by the
remaining recovery and local-cancellation audits. Trusted requester/broker,
foreign bindings and publication/pin adoption remain. t333 stays open;
production pins are unchanged. No quiet hold or performance claim.

### Capture transport checkpoint

Signed Rust `cd674b9` and C `df58325` now drive capture requests on the
authenticated portal root. Both send one complete write, close its fid, read
fresh bounded status snapshots through EOF/clunk, and return the same root for
frame reads. Caller timers schedule pending status; no driver sleeps or replays
requests. The caller owns serial sequencing across reconnects.

Independent socket tests cover partial I/O, reuse, refusal, short writes, loss,
identity mismatch, terminal-status clunk failure, limits, expiry and cancellation.
The fifteen Rust client tests, C request test and strict Rust Clippy pass.
Both independently built peers pass all four production portal gates, including
the new request-to-frame path through real authentication and coordinator
settlement. The two ordinary portal-dispatch regressions also pass. Policy
and pixels are supplied; this is not native capture or performance acceptance.

Use `SOPHIA_PORTAL_AUTH_PEER` with `external_sdk_portal_` as below.
Peer SHA-256: Rust
`53d89a923d4cf259f8d21f129165494849094513b6e06ff95596cc4d80e974b8`;
C `ee3215d266fdf1cc12589ac4e70cb2ad061163a92a2d5d135fbbbf72dc34c681`.
Portal map 12 records API ownership and limits. Next qualify production request
refusal and lock cancellation, then finish transfer/watch and recovery coverage.
Trusted requester/broker, foreign bindings and publication/pin adoption remain.
t333 stays open; production pins are unchanged. No quiet hold is active.

### Capture-custody component checkpoint

Signed Rust `c18c9e4` and C `f8f6586` add capture-custody components with
inline request storage and no allocation. They distinguish pipeline admission,
exact write acceptance, refusal, uncertain delivery and all five terminal
capture outcomes. Verified status snapshots must match serial and output;
ready is historical status, not a frame grant. One caller-monotonic deadline
covers observation. Local cancellation cannot cancel admitted server work.

All twelve Rust portal-client tests and strict Clippy pass. C's independent
capture-client test passes equivalent outcome, refusal, short-write, loss,
identity, clock and deadline cases. Portal map 11 records this component scope.
Next integrate capture writes and status reads with the authenticated role
transport: enforce increasing owner serials, capability and write limits,
then qualify request custody against the production export. The frame gates
below remain evidence for frame delivery, not this new request path.

t333 remains open. Transfer/watch, requester/broker, foreign bindings and
release adoption remain; production pins are unchanged. No quiet hold or
measured performance claim.

### Mid-read revocation checkpoint

Signed Rust `228ce0a` and C `e650904` pass production mid-read frame revocation.
Each peer pauses after accepting part of the payload; the fixture advances the
lock fence, then lets the peer continue. Both fail without a completed image;
timeout cannot pass this check. Already buffered bytes are not recalled.
All three external portal gates pass independently in both languages, as do
the two ordinary portal-dispatch regressions and strict Rust Clippy. Portal
map 10 records the limits.

Peer SHA-256: Rust
`2f94decb9742518519fb5088605d233a0e60872224c1c58c44537c8245303b53`;
C `7b405aee867ccae4c702cb1f53d04c863ca57fe2525de9df3d555fe0b4a6bb2a`.
Next implement capture-request custody and remaining recipient operations.
Requester/broker, foreign bindings and release adoption remain. t333 stays
open; production pins are unchanged. No quiet hold or performance claim.

### Supplied-frame delivery checkpoint

Signed Rust `328db4a` and C `5d805c7` pass real authenticated delivery of a
supplied 256×128 frame. The fixture supplies policy, pixels and the request;
the production coordinator installs the object. Both peers check metadata,
every pixel and final root-only fid ownership. The startup and delivery gates
pass independently in both languages; existing portal-dispatch tests and strict
Rust Clippy pass. Portal map 9 records the limits.

Use the earlier `SOPHIA_PORTAL_AUTH_PEER` command with filter
`external_sdk_portal_` to run both gates. Updated peer SHA-256: Rust
`d5c28815ddc7905e9e189955becff7d246c6f8cc6265c8b7cb769068151fb706`;
C `ae63e59b06b88ec2e363932e4faceeef9e11d1fa9fc42994ec931335944b681f`.
Next qualify mid-read revocation and implement SDK capture request custody and
remaining recipient operations. Requester/broker, foreign bindings and release
adoption remain. t333 stays open; branches pushed, production pins unchanged,
no quiet hold or measured performance gain. Supplied frames do not prove native
presentation or physical capture acceptance.

### Retained-frame reader checkpoint

Signed Rust `afa827c` and C `44157af` now integrate retained frame reads into
reusable image storage. Both use four requests, verify payload/EOF and the
original metadata fid, and clunk before exposing results. Repeated captures
reuse the root and image buffer without api reads. Rust grows its Vec if needed
but avoids re-zeroing unchanged storage; C allocates nothing and refuses
insufficient caller capacity. Portal map 8 records ownership and failure rules.

Independent peers pass short reads, one-byte dispatch, repeated reads, stale or
changed metadata, extra pixels, early termination, clunk refusal, deadline and
cancellation. C also tests insufficient capacity. Nine Rust client tests and
strict Clippy pass; C's frame-client test passes. SDK branches are pushed.
Next qualify both frame readers against the production authenticated portal
export, then add capture custody and remaining recipient operations. Requester/
broker, foreign bindings and release adoption remain. t333 stays open; production
pins unchanged, no quiet hold or measured speedup.

### Frame scheduler checkpoint

Signed Rust `7966089` and C `58c25f6` add a fixed four-request frame range
scheduler. It honors framing/iounit/chunk bounds, handles out-of-order and short
reads without repeating accepted bytes, and rejects empty, oversized or
duplicate replies. Independent exact-byte-coverage and 64 MiB range tests pass,
with strict Rust Clippy. Portal map 7 records the component's limits: it owns
neither transport nor image storage and performs no payload copies.

Next integrate retained frame-info/frame fids, the range scheduler and
caller-controlled image storage into recipient sessions. Require exact payload,
EOF, a reread of the retained metadata fid, and successful clunk before exposing
a completed image; do not infer binding from private qid numbering. Capture
custody, revocation/delivery gates, requester/broker, foreign bindings and release
adoption remain. t333 stays open; SDK branches pushed, production pins unchanged,
no quiet hold or measured speedup.

### Authenticated portal startup checkpoint

Signed Rust `5f6b5bb` and C `82ffda2` pass protected namespace launch and
Factotum authentication followed by portal metadata startup. Both independently
built peers retain the admitted root and relay, read api and exit successfully.
Portal map 6 records the startup-only scope. The two existing portal-dispatch
tests also pass after the fixture extension; strict Rust Clippy passes.

Run with `SOPHIA_PORTAL_AUTH_PEER` set to either SDK's built
`portal_authenticated_peer`, then `cargo-slot sophia cargo test -p sophia-session
--features native-session --lib external_sdk_portal_startup_uses_real_namespace_authentication
-- --ignored --nocapture`. Rust executable SHA-256:
`1f03bb2b4beee0e889456674a8c3525fec128a5d46e847785b4aee8f14d815de`;
C: `d7fd227d631c13e8b42f78891c2e953aef0d0dd65dfc7c184a7ce422be13e556`.
The Rust executable is in the SDK worktree's cargo-slot 1 debug/examples;
the C executable is in its worktree's build directory.

Next implement recipient session operations, retained-object frame reads and
capture custody, then qualify revocation and delivery. Requester/broker,
foreign bindings and release adoption remain. t333 stays open; SDK branches
pushed, production pins unchanged, no quiet hold or measured speedup.

### Attached-root startup checkpoint

Signed Rust `91d5d21` and C `61fb84d` add metadata startup on the existing
attached recipient root. Both read api through EOF/clunk, preserve root and
pipeline, and issue no new version/attach/reconnect. The caller must complete
launch authentication first and retain the relay separately. Root ownership
checks prove no authority. Five Rust client tests, C startup tests and strict
Clippy pass; portal map 5 records evidence and lifetimes. The scripted attach
is not a production authentication proof.

Next compose launch authentication with this handoff and implement recipient
session operations, retained-object frame reads and capture custody. Production
portal gates, requester/broker, foreign bindings and release adoption remain.
t333 stays open; SDK branches pushed, production pins unchanged, no quiet hold
or measured speedup.

### Portal snapshot checkpoint

Signed Rust `3a90668` and C `8da1961` add provisional metadata snapshot
drivers for api, status and capture-status. Results wait for EOF, decoding and
clunk; reads respect msize/iounit and file bounds. Three Rust tests, C's
independent snapshot test and strict Clippy pass. Rust uses 101 inline bytes;
C borrows caller storage. The helpers allocate no storage themselves, but
Rust transport replies may own allocated data. Portal map 4 records limits.

Next integrate authenticated startup using the existing launch client's
authenticated root and exclusive role/relay pipelines, then retained-object
frame reads and capture custody. The snapshot helpers neither authenticate nor
own transport. Requester/broker, production gates, foreign bindings and release
adoption remain required. t333 stays open; production pins unchanged, no quiet
hold or measured speedup. SDK branches are pushed. The two formerly dirty
native-scanout files were formatting only, checked and pushed separately as
`8233b7641` at the owner's request.

### Portal discovery checkpoint

Signed Rust `574bb1f` and C `231e012` add allocation-free recipient api/status
decoders. Both retain distinct request capabilities and transfer states, reject
malformed snapshots and enforce 101/43-byte bounds. Independent tests and strict
Rust Clippy pass; portal map 3 records coverage. Next build authenticated
recipient sessions with retained-object reads, caller-controlled frame storage,
capture custody and production gates. Requester/broker and release-wide parity
remain required; t333 stays open, production pins unchanged, no quiet hold or
measured speedup. The owner authorized pushing these signed development branches.

### Capture record checkpoint

Signed Rust `030128f` and C `14c687b` add allocation-free capture request
encoding and status decoding. Both preserve all eight states and enforce
exact record sizes, version and idle/nonidle identity rules. Independent literal
and malformed-boundary tests pass, including unchanged C output on failure.
Both portal test suites and strict Rust Clippy pass; portal map 2 records the
scope. These passive codecs do not enforce serial ordering or authorize pixels.

Next add recipient discovery/status records, then authenticated sessions with
caller-controlled frame storage and production gates. Requester/broker and
release-wide qualification remain required. t333 stays open; pins unchanged,
no quiet hold and no measured performance gain.

### Portal frame codec checkpoint

Signed Rust `cf22985` and C `1d0f53c` add independent passive portal frame
metadata decoders. Both enforce exact record size, identities, format, u32
dimensions, canonical stride and the 64 MiB image limit without allocation or
pixel copies. Independent malformed/boundary tests and strict Rust Clippy pass.
`PORTAL-PARITY.md` revision 1 pins the unchanged recipient contract from Sophia
`9b57fd040` and records the remaining scope. These are codecs, not portal clients.

Next add capture request/status and discovery records, then authenticated
recipient sessions with caller-controlled frame storage and production gates.
Keep trusted requester authority separate; requester/broker and release-wide
qualification remain required. t333 stays open, pins unchanged, no quiet hold
and no measured performance gain.

### Administrative namespace-denial checkpoint

Signed Rust `bda343d` and C `9e71ea2` now pass foreign-namespace denial
against the production administrative export. Both connect to the reachable
socket, then require transport closure during negotiation; a timeout or ready
session fails the check. No request reaches the owner. All five external SDK
gates pass for each peer, and strict Rust Clippy passes. Admin map 15 records
the scope: HostDomain admission, not another role's Factotum authentication.

Peer SHA-256: Rust
`e5b735c0790036ee455a582d0ba4c13f66d214d5b0bc9ea896fbb57d50f79bac`;
C `f45a6ea4e5e841dd4fb0420308c9ad423db37f49b0f014bc4839f94aa6ef6ac2`.
Use the executable paths and filtered production command below. Next implement
typed portal recipient coverage against `docs/portal-client-files.md`, keeping
requester authority separate; audit requester/broker requirements and the
remaining release-wide gates. t333 stays open; pins are unchanged, no quiet
hold is active and no performance gain is claimed.

### Integrated failure checkpoint

Signed Rust `2e84ad0` and C `7af01ca` add matched integrated failure checks
for short write replies, malformed status detail lengths and an illegal
owner/success outcome. All six negative schedules retain the expected custody,
refuse further invocation and issue no replay. One-byte command dispatch also
passes the successful three-command sequence, retaining the catalog until
explicit refresh. Both independent tests and strict Rust Clippy pass; admin
map 14 records the evidence. No production code or measured performance changed.

Next qualify namespace denial, then finish the portal/broker and release-wide
parity requirements. t333 remains open; pins are unchanged and no quiet hold
is active. The four production gates and peer hashes below remain unchanged.

### Abrupt process-loss checkpoint

Both independently built administrative peers now pass abrupt death of a
test-owned production service process before and after owner claim. The parent
waits for the accepted/claimed marker, kills only that child and waits for its
exit. Each SDK retains uncertain custody and refuses another invocation. The
effect marker exists only after claim; the old endpoint refuses connection,
and a replacement service gets a new endpoint. SDK doc checkpoints are Rust
`0af3cf4` and C `a7fe221` (admin map 13); peer code/hashes are unchanged below.

All four production external-SDK gates pass for each peer. The existing
runtime-client crash gate also passes after extracting their shared service
fixture. Run the SDK gate with `cargo-slot sophia cargo test -p sophia-runtime
--test admin_external_sdk external_sdk_ -- --include-ignored --nocapture` and
the same `SOPHIA_ADMIN_SDK_PEER` paths below. The name filter excludes the child
helper, which requires its parent-provided environment. Next audit integrated
malformed/partial traffic and namespace denial, then finish the remaining
portal/broker and release-wide parity requirements. t333 stays open; pins
unchanged, no quiet hold and no measured speedup.

### Replacement-service recovery checkpoint

Signed Rust `51e75e1` and C `b557c34` pass explicit recovery on a replacement
production administrative service. Each peer retains its uncertain old command,
waits for an explicit endpoint, discovers a different service identity and
invokes a newly advertised probe. The new service deliberately reuses the old
connection/request numbers. Finishing the old ticket as Committed does not
settle the probe; the new owner's Rejected result does. No uncertain mutation
is replayed, and the old command remains uncertain. Admin map 12 records this.

All three `admin_external_sdk` tests pass independently for each SDK; strict
Rust Clippy and the C peer build pass. Updated peer SHA-256: Rust
`19a1566dd2204838a2245da541859426111c371a21347fd1c976d831fb98f1f7`;
C `7d68379daeab5d72b0d3a32424f7877b5fb095f7f14dd9d14652617b805f2d43`.
Use the same command and executable locations below. Next qualify abrupt
service-process death and remaining integrated negative schedules, then the
portal/broker and release-wide audit. t333 remains open; pins unchanged,
no quiet hold and no measured speedup.

### Service shutdown checkpoint

Signed Rust `bf06a6f` and C `5367415` pass production administrative service
shutdown before and after command-ticket claim. Both clients preserve uncertain
custody and refuse further invocation. Dropping the service cancels the unclaimed
ticket; the claimed ticket stays dispatched and may finish after the peer exits.
This is orderly worker teardown, not abrupt process death. Both independent
peers pass both `admin_external_sdk` tests, including the earlier completion,
stale-refresh and admission-revocation schedules. Admin map 11 records the limit.

Updated peer SHA-256: Rust
`b1224db04ef31a021aa5904b497a68f13d90053987128b59f751ba02f0296ce2`;
C `207e07651049b3592811426cad5976a9ee9ff8f862993ae9f8efe9adca4be7d0`.
Use the same command and peer locations below. Next qualify explicit recovery
on a replacement service without replaying the uncertain mutation, then remaining
process-loss/integrated failures and the portal/broker release audit. t333 stays
open, pins unchanged, no quiet hold and no measured speedup.

### Initial administrative production checkpoint

Signed Rust `c586e1b` and C `c40ce7a` pass the same production administrative
export gate, `admin_external_sdk`. Each peer completes request 1 through a
claimed ControlTicket, receives Stale for request 2 after catalog generation
changes, refreshes and completes request 3 on the same connection. Only 1 and 3
reach the owner. A separate run revokes child admission after its ticket is
claimed: both SDKs terminate with uncertain custody and refuse another command;
the ticket stays dispatched. The fixture supplies policy completion and does
not execute a WM action. Admin map 10 records this qualification boundary.

Run `cargo-slot sophia cargo test -p sophia-runtime --test admin_external_sdk
-- --include-ignored --nocapture`, setting `SOPHIA_ADMIN_SDK_PEER` separately
to each independently built executable. Rust: its worktree cargo-slot
`debug/examples/admin_conformance_peer`, SHA-256
`48f6f2e4cb5e45b85f73d01ab32b5af2f96d0ba3ba0c9dc131ad87b9b1d8d7bd`.
C: `build/admin_conformance_peer`, SHA-256
`89bcd5698f3316a96aa71f7cc062b6f6402f3d01f36b5599ce67ceb5ff88194c`.
Both runs pass; strict Rust Clippy and the C peer build pass. No binaries were
copied into evidence. Next qualify service loss/recovery and remaining integrated
failure schedules, then continue the portal/broker and release-wide parity audit.
t333 remains open, pins unchanged, no quiet hold and no measured speedup.

### Socket startup checkpoint

Signed Rust `99b9023` and C `3f90f17` add nonblocking startup from a caller-
connected HostDomain socket, covering version, file opening and initial catalog
under one absolute deadline. One phase spends the caller's I/O budget per
dispatch; the ready session transfers once. The full scripted command exchange
now uses these facades. Paired stalled-version tests cover zero/one-byte budgets,
clock regression and expiry without extra writes; unsupported versions terminate
without attach or fallback. Twelve Rust client tests and C's session/connecting
tests pass; strict Clippy passes.

Admin map 9 and shared `ADMIN-CLIENT.md` document admission, storage, event-loop
driving, custody and explicit recovery. C retains caller descriptor ownership
and requires immovable session storage; Rust owns its stream and uses offsets
into retained catalog bytes. Neither automatically reconnects or replays. Next
extend integrated failure schedules and run both independent peers against the
production administrative export, including ticket/owner and process-loss
behavior. t333 remains open, pins unchanged, no quiet hold and no measured
speedup.

### Integrated session checkpoint

Signed Rust `1ea6ffe` and C `c201c6e` integrate administrative sessions on their
startup handoffs. Both retain one catalog, admit one command, assign increasing
IDs, drive matched replies and expose custody/outcomes, refresh, write readiness,
next wake and close. Rust retains catalog bytes plus name offsets; C's borrowed
names point into its fixed buffer, so initialized C session state must not move.
Neither clones a catalog per command. Status storage is reused; idle sessions
schedule no status reads. Terminal failures preserve custody and forbid replay.

Ten Rust client tests, nine protocol tests and strict Clippy pass. C's independent
integrated socket test passes. Both peers cover three commands on one attach,
pending before each outcome, catalog reuse and refresh after a Session result,
unadvertised/busy refusal, explicit write refusal, loss after write and wrong
outcome generation. Confirmed outcomes survive close. Admin map 8 records the
limits: these are scripted peers, not production export evidence. Next add the
raw-socket/version facades, integrated deadline/partial-I/O failure cases and
ownership/recovery guidance, then run paired production gates. t333 stays open;
pins unchanged, no quiet hold and no measured speedup.

### Command custody checkpoint

Signed Rust `d50871a` and C `2ad3a29` add administrative command custody and
caller-driven status scheduling. An immutable command distinguishes queued,
issued, accepted, settled, refused, dropped-unsent and uncertain states.
Explicit write errors prove refusal; short/lost replies and expiry after issue
preserve uncertainty. Complete outcomes survive close. Status must match the
connection/request/generation and the owner's legal success outcome.

Pending results schedule another snapshot after one millisecond without
extending the original deadline; each snapshot has its own bounded frame wait.
The tracker allocates no heap memory and copies at most 256 detail bytes once
for retained results. Eight Rust client tests, C's independent command test and
strict Clippy pass. All owner/outcome combinations, identity refusal, custody
on close, short/refused writes, expiry, clock regression and cadence are covered.
Admin map 7 records the component boundary. Next integrate startup, snapshots
and commands into the full session: retained catalog ownership, monotonic request
sequencing, same-pipeline dispatch and external-loop facade, followed by paired
production gates. t333 remains open; no speedup measured, pins unchanged and
no quiet hold active.

### Startup checkpoint

Signed Rust `fdc9688` and C `826eeda` add administrative startup on an exclusive,
already-versioned 9P pipeline. Both attach once, validate info through EOF/clunk,
enforce timeout ceilings and open ctl with a complete-command iounit. They hand
off the same pipeline/root/ctl without reconnecting. Rust uses a shared budget
across at most 32 rounds; C services its bounded pipeline once per dispatch and
queues the next request. Neither sleeps or performs I/O after startup is ready.

Five Rust client tests, C's independent startup test and strict Rust Clippy pass.
Socket cases cover fragmented info, one-byte budgets, file order, retained fids,
directory/timeout/iounit/clunk refusals, expiry before transmission and clock
regression. C distinguishes transport errors and requires its caller to close
the borrowed fd; Rust drops its owned pipeline on terminal failure. Admin map 6
records the scope. Raw-socket/version facades, catalog retention/refresh,
invocation custody, status scheduling and paired production gates remain.
Next implement the command session on this startup handoff. t333 stays open;
pins are unchanged, no quiet hold is active and no speedup is measured.

### Earlier SDK checkpoints

Signed Rust `96aba59` and C `0797146` add provisional administrative snapshot
state machines for info, commands and status. Both emit bounded Walk/Open/Read/
Clunk operations and expose a result only after EOF, syntax validation and
successful clunk. Reads respect msize/iounit and probe beyond the file bound
without storing overflow. Remote errors do not cause retries. Absolute deadlines
and nonmutating clock-regression refusals use caller-supplied monotonic time.

Rust reserves/reuses the supplied Vec; C uses caller storage. Tests verify
storage identity, fragmented reads of all three files, EOF/clunk barriers,
malformed EOF, oversized replies, overflow, clunk refusal, EAGAIN, deadline and
clock regression. Three Rust component tests, C's independent snapshot test and
strict Rust Clippy pass. Admin map 5 documents the limits: matched replies are
supplied by the tests, and the connection driver must match tags and close on
terminal errors. Scratch storage before completion is not a validated result.
Next join these components to each SDK's pipeline for startup, readiness and
bounded dispatch, then invocation custody and pending-only status wakeups.
No complete administrative client or production gate is claimed. t333 remains
open, pins unchanged, no quiet hold or measured speedup.

Signed Rust `42d28dd` and C `e184709` add complete administrative catalog
validation and binary-search membership lookup. Both keep at most 259 inline
rows with names borrowed from the immutable snapshot, without heap allocation.
They reject the whole catalog for bad counts, ordering, duplicate names,
unsupported Session operations, excessive policy rows or malformed text.
C leaves caller output unchanged on failure. Nine Rust administrative tests,
all four C administrative test binaries and strict Rust Clippy pass. The full
259-row catalog and the 256-policy boundary have independent tests in both.

Admin map 4 records this checkpoint. Next implement caller-driven clients over
the existing 9P pipelines: bounded snapshot reads through EOF and clunk, one
ctl invocation, exact identity/custody, pending-only status wakeups and explicit
failure/recovery. The runtime reference clones catalogs during commands and
invocation and sleeps between status snapshots; the SDK clients should retain
one validated catalog and expose wake times instead. Record buffers must outlive
borrowed views, with no self-referential Rust storage. Paired production and
failure gates remain required. t333 stays open; no speedup is measured and pins
remain unchanged.

Signed Rust `a53824f` and C `a19fa07` add allocation-free invocation and
status decoding and fixed-buffer invocation encoding. Names and outcome details
borrow input storage; all ten outcomes remain distinct. Encoders validate
before copying into caller storage, leaving it unchanged on failure. C permits
the source name to overlap destination storage. Neither parser establishes
catalog membership, admission, EOF or owner-specific outcome legality.

Six Rust administrative tests, all three C administrative test binaries and
strict Rust Clippy pass. Coverage includes literal wire records, maximum
identities and lengths, all outcomes, borrowed pointers, short output buffers,
truncated prefixes and invalid text. Admin map 3 records the boundaries.
Next implement complete catalog validation, then the nonblocking clients and
paired production gates. This is codec evidence, not client parity or a
measured speedup. t333 remains active with unchanged pins and no quiet hold.

Signed Rust `54d86dd` and C `d582a23` begin administration support with
equivalent allocation-free info decoders. Rust returns a Copy value; C writes
caller storage only after full validation. Independent literal and malformed
tests cover maximum identities/timeouts, zero/overflow, canonical spelling,
every truncated prefix, extra fields and invalid text. Both suites and strict
Rust Clippy pass. Admin map 2 records the scope: parsing alone establishes
neither EOF nor admission. Next implement catalog, invocation and status
codecs, then the bounded external-loop clients. No CPU gain is claimed.

Signed Rust `f649aff` and C `88cf285` also pass protected unfinished-upload
revocation. Each writes and settles four bytes of a 16-byte resource, then
queues its remaining twelve bytes with zero I/O budget. The parent revokes
launch authority before the peer resumes. Both clients terminate, retain
ResourceBegin custody, clear pending work and reject End/Cancel. No resource
or candidate reaches Session; Rust returns the same owned tail buffer.
This proves a queued-tail schedule, not interruption of a partially sent wire
payload. Fresh authorized replacement and SDK reauthentication remain open.

Both protected tests pass independently for each SDK with the
`lock_external_sdk::` filter, `--include-ignored` and the peer paths below.
Strict Rust Clippy, the C peer build, both contract hash manifests and diff
checks pass. Updated protected-peer SHA-256: Rust
`c7dedce92e7c9891552ae0d4c24e0a59a55972361aaa4f43c8ee4e86674d07a5`;
C `6e0a2b60cd94703a61090a19bb2f926a5989503bc2f153c7645a0b28b4951a3b`.
API matrix 48 / lock map 8 record the qualification boundary.

The remaining-role audit confirms that administration has a production export
but neither SDK has its typed client. Both now pin `sophia-admin-files.md`
from Sophia `baedfd0c9` and carry `ADMIN-PARITY.md` revision 1. Implement its
bounded passive codecs, then caller-driven clients in both languages. Preserve
one pending invocation, uncertain custody after lost replies, and explicit
recovery. Reuse connections and buffers; poll status only while a command is
pending, with caller-driven wakeups rather than blocking sleeps. No speedup
has been measured.

Broker files remain design-only under t314; the legacy broker transport is
not SDK file coverage. Portal requester and recipient/capture interfaces exist
but need separate typed SDK coverage: private requester admission must not be
confused with namespace-selected recipient authority. Administration and these
portal gaps precede release-wide parity, foreign-language smoke, signed
publication and verified pin adoption. t333 remains open; pins are unchanged
and no quiet hold is active. The checkpoints below retain earlier evidence.

Signed Rust `3e5b679` and C `165c250` pass paired lock production-export and
protected idle-revocation gates. The runtime gate runs pinned C, candidate C
and external Rust through the same production worker: chord negotiation, exact
object fetch, two-chunk 2x2 upload, demand/permit, candidate outcome, Entry/Chord
events and Unlocking publication. All three pass. The harness supplies admission,
lock state, permits and outcome; this is not Engine rendering or physical-lock
acceptance. The C candidate source path is explicit and pins remain unchanged.

Both SDK peers separately pass the native-session launch-authentication test
`external_sdk_lock_authenticates_and_observes_revocation`: real Factotum and
bubblewrap, same admitted root, object/ACK, revocation, retained negotiation
custody, one connection/disconnection and zero remaining namespace admissions.
The existing raw authenticate/reauthenticate/revoke control also passes, but does
not qualify SDK session reauthentication. Lock state and the private channel
class remain supplied. In-flight upload revocation is still a separate gate.

C now has a passive `sophia_lc_obligations` query corresponding to Rust's
consumed/acked, pending flags, write count and retry/ACK waits. Invalid arguments,
retry, held/drained uploads, terminal flags and production ACKs are tested.
All 37 Rust lock tests, strict Clippy and C's full suite pass. Both SDKs record
API matrix 47 / lock map 7 and the ownership guide. No performance gain is claimed.

Reproduce with `cargo-slot sophia cargo test -p sophia-runtime --test
lock_client_c_sdk -- --include-ignored --nocapture`, setting
`SOPHIA_LOCK_RUST_SDK_PEER` to the independently built example and
`SOPHIA_LOCK_C_SDK_SOURCE` to the candidate checkout. Run the protected test with
`-p sophia-session --features native-session --test launch_authentication`, the
test name above and `--include-ignored`; set `SOPHIA_LOCK_AUTH_PEER` separately
for each SDK. Rust peer binaries are in the SDK worktree's cargo-slot pool;
C's protected peer is `build/lock_authenticated_peer`.

SHA-256: Rust production peer
`a268d30fe0a557eff8b01184eab74e0ebc1f387993ca29174dff582aac4e5d90`;
Rust protected peer
`b2d06f663134eb8165f5c5fec12f2500c494fa9b1accb456466fe2bfd401dc72`;
C protected peer
`d68cdfa475b8d7e55c3bb80409258e2d84ddf32eeaa6b56d6759de10e9e81db2`.

Next qualify in-flight lock revocation/recovery and audit remaining role failure
coverage, then administration and public broker/portal APIs. Release-wide parity,
foreign-language binding smoke, signed publication and verified pin adoption
remain required. t333 is open; no quiet hold is active.

Signed Rust `94ff9db` integrates the experimental lock Session and nonblocking
Connecting facade. Direct startup and admitted-root handoff retain one connection;
the facade transfers a ready session once and preserves terminal inspection.
The session owns ordered events, exact object fetch, custody, explicit retry,
ACKs and 1–8 upload writes. Control requests precede bulk writes; after draining
cancellation, a second submission drive exposes the new request before returning.
The default window now admits six request slots, matching C; larger windows need
their width plus five slots. Rust still requires sixteen fids and bounded input.

All 37 Rust lock-client tests, strict Clippy and formatting pass. Six integrated
socket tests cover both startup paths, negotiation/later retry, object delivery,
control progress with eight held writes, End/cancellation/short-write/rejection,
short/error/stale ACKs, refusal ACK, uncertain disconnect and confirmed custody
on close. A one-byte-budget test proves Cancel has POLLOUT after the last held
reply drains. Signed C `86b3e89` adds independent six-request negotiation retry;
the C lock suite passes. Both SDKs have API matrix 46, lock map 6 and LOCK-CLIENT
ownership/recovery guidance. No measured performance gain is claimed.

Next run paired C/Rust lock production-export and protected-launch gates. Then
finish remaining role coverage, release-wide parity audits, foreign-language C
binding smoke, publication and verified pin adoption. The lock contract remains
draft; scripted peers do not establish production or physical acceptance.
t333 stays open, pins are unchanged and no measurement hold is active.

Signed Rust `506cec0` adds the provisional lock upload controller: one upload,
1–8 write slots, five request slots reserved for controls, explicit cancellation
and draining before cleanup. Reversed replies may free slots for refill while
an earlier reply remains held. Pipelined short writes cancel; single-write mode
continues at the returned count. Slot errors end or cancel only the upload;
malformed replies terminate the session. Resource status and submission refusal
drive cleanup, allowing later uploads after the slot is released.

Chunks transfer as owned Vecs without a payload copy. Failed admission returns
the same buffer; take_chunk returns completed storage for reuse only after all
issued writes settle. C borrows storage instead. Transport may still copy wire
payloads, and runtime gains remain unmeasured. Seven upload tests bring the Rust
lock-client total to 31; all pass with strict Clippy. C's independent lock suite
passes, including socket-backed window, cancellation and control-progress tests.
Signed C `f7d3d45` records API matrix 45 / lock map 5 in both SDKs.

Next integrate startup, submission, journal, object fetch and uploads into the
lock session/connecting facades. Test real-socket control progress, immediate
readiness after cancellation drains, terminal custody and recovery before paired
production/protected gates. Remaining role coverage, publication and verified
pin adoption still block t333. No quiet hold is active; pins remain unchanged.

Signed Rust `70c9b22` adds the provisional lock journal and object fetch.
The journal retains one application head and does not decode controls behind
it, matching C. Both byte buffers are fixed at 1024 bytes. A cursor avoids
moving unread events after every consume; compaction happens only when an
append needs tail space. Lock fetch verifies Qid, envelope epoch, body and EOF,
then clunks before exposing the value. A newer Qid supersedes the announcement;
it is released unread and consumed internally. The decoded allocation list
still allocates, but its verified value transfers without cloning. Neither
component adds clocks, retries or polling; no runtime gain is measured.

All 24 Rust lock-client tests and strict Clippy pass. Signed C `bdcd0f6` adds
independent rejection checks for old Qids, wrong epoch, premature EOF and
trailing bytes; the lock suite passes. Existing C checks cover supersession,
ordered Entry/Chord events, grants and refusal ACKs. Both SDKs record API
matrix 44 / lock map 4. Next implement upload lifecycle and 1–8 write windows,
then integrate journal, fetch and submission into session/connecting facades.
Production tests, remaining roles and publication still block t333. Pins and
the measurement pilot remain unchanged; no quiet hold is active.

Signed Rust `834d5b9` adds the bounded lock submission lane: immutable candidate
storage, borrowed write slices, explicit retry, latest custody, refusal and
disconnect handling. Transaction settlement clunks and walks the file again;
reopening waits for the Submitted ACK. Earlier references below to a persistent
transaction fid describe startup retention, not reuse across submissions.
Signed C `b383726` fixes a contradictory EAGAIN after confirmed custody: fail
closed and retain custody, never offer replay. Both SDKs document this behavior
in lock map 3 / API matrix 43. Fourteen Rust lock-client tests, strict Clippy,
formatting and C's full independent suite pass. The submission lane adds no
clock reads or polling and owns fixed storage; candidate body preparation still
uses a temporary allocation. Runtime improvements have not been measured.

Next implement the lock journal and exact Lock object fetch, then uploads with
1–8 outstanding writes and integrated session/connecting facades. Keep the
caller-owned retry policy and avoid cloning fetched objects. Independent
production gates, remaining role coverage and publication still block t333.
No production pins changed and no measurement hold is active.

Signed Rust `4c422a9` adds provisional lock fixed-file startup. It reads the
exact decimal API epoch, verifies the Limits envelope against it through EOF
and clunk, and opens events, transaction, submit and ACK with their proper
modes. The transaction fid and iounit remain available to the submission lane.
Attached startup preserves the exact sole quiescent root without reattachment.
Seven socket tests cover fragments, malformed/overflowing epochs, epoch mismatch,
control iounits, root/bounds, clock regression and unsent expiry. All ten Rust
lock-client tests, strict Clippy and C's full suite pass. C `c101d3d` records
matrix 42 / lock map 2, including Rust's conservative eight-request/sixteen-fid
bound versus C's six-request minimum and caller-owned timeout difference.

Next implement lock submission on the persistent transaction fid: immutable
bytes, explicit EAGAIN retry, latest-result custody and Submitted ACK barriers.
Then journal/Lock object handling, upload lifecycle and 1–8 write windows,
integrated session/connecting facade and independent production tests. Startup
alone does not establish lock-session parity. Production pins remain unchanged.

Signed Rust `2ce53f7` starts experimental `sophia-lock-client` with typed
preparation for all seven candidate kinds. Immutable encoded candidates fit
fixed 128-byte owned storage; the body codec still uses one temporary Vec,
so preparation is not allocation-free. Direct envelope writing avoids a second
temporary allocation. Three tests cover all typed bodies, literal FrameDemand
bytes, caller mutation, full-size presentation and invalid shapes/offers.
Strict Clippy and C's independent full suite pass. C `fd99803` records the
complete operation checklist in `LOCK-PARITY.md` revision 1 / matrix 41.

Next implement lock startup and the persistent transaction submission lane,
then journal/object handling and uploads. Lock differs from output: the API
line contains the epoch, four fixed streams include a retained transaction
fid, retries are explicit, superseded Lock publications are consumed internally,
and uploads support 1–8 writes with five control slots reserved. Drain issued
writes before cancellation; pipelined short writes cancel, single-write mode
may continue. Keep startup/upload-owned candidate kinds out of unrestricted
public submission. The pinned contract remains draft and providers only render;
there is no credential or unlock authority. No session or performance claim
is made by this foundation. All other t333 scope remains required.

Signed Rust `5d66fec` and C `cce3196` add independent protected output peers.
Both pass the real protected process/Factotum fixture, hand the exact admitted
root to their native session, fetch and ACK topology, then observe idle grant
revocation while retaining negotiation Submitted custody. Namespace admission
returns to zero. The original raw reauthentication/revocation control passes
separately. C's full suite and strict Rust output all-target Clippy pass.
Matrix 40 / output map 9 record the scope: no SDK reauthentication or in-flight
proposal revocation is claimed. Production pins and the desktop are unchanged.

Run each prebuilt peer with
`SOPHIA_OUTPUT_AUTH_PEER=/absolute/path/to/output_authenticated_peer cargo-slot sophia cargo test -p sophia-session --features native-session --test launch_authentication external_sdk_output_authenticates_and_observes_revocation -- --include-ignored --nocapture`.
The invocation without `native-session` ran zero tests and is not evidence;
the corrected explicit target ran one passing test for each SDK.
Rust peer SHA-256: `02a798b4e609d77bb5673d2f2ebaae0e674365a815c3e4a6b669bd8f30a82551`
at `~/.cache/cargo-slots/sophia-desktop-sdk-rs@sdk-rs-t331-reply-storage/1/debug/examples/output_authenticated_peer`.
C peer SHA-256: `704b5dd29890dcd80e92cb1c437521b9114cc21d018d7d4f13db5280d6559d26`
at `~/dev/sdk-c-t318-namespaces/build/output_authenticated_peer`. No binaries
were copied into evidence. Next implement the experimental native Rust lock
client against C's operations and pinned contract, keeping remaining output
qualification limits in its operation map for the final release audit.

Signed Rust `82b7319` adds `output_conformance_peer`; it and Sophia's existing
C peer pass the same production output export/worker exchange. Both negotiate,
fetch the exact topology, submit ValidateOnly, receive Validated and settle
the final ACK. The fixture admits each peer's exact PID and supplies topology
and the validation outcome. This proves wire/worker interoperability, not
Engine validation, physical application or protected launch authentication.
Signed C `936465a` records matrix 39 / output map 8. C's implementation and
Sophia's verified SDK pins remain unchanged.

The paired command passes two tests; the normal output export suite passes
15 with the external Rust case ignored. Explicit invocation is required:
`SOPHIA_OUTPUT_RUST_SDK_PEER=/absolute/path/to/output_conformance_peer cargo-slot sophia cargo test -p sophia-runtime --test output_file_export sdk_session_against_the_output_file_worker -- --include-ignored --nocapture`.
Rust all-target output Clippy passes. The built Rust peer at
`~/.cache/cargo-slots/sophia-desktop-sdk-rs@sdk-rs-t331-reply-storage/1/debug/examples/output_conformance_peer`
has SHA-256 `2da234f7c6b93cfeb28b64e1bb9cf307aa17d53cb1bd45fab4228fc8ccd087d0`.
No binary was copied to evidence. Protected output launch and idle revocation
are now covered above; continue experimental lock,
administration and the remaining t333 audit/publication requirements.

Signed Rust `9ac9742` and C `fc67ebd` add independent output terminal socket
checks through direct and attached startup: short ACK, ACK EINVAL/ESTALE and
disconnect before/after observed proposal custody. Confirmed Submitted and
UnknownDisconnected remain distinct; terminal state, errno and custody survive
explicit close. All 37 Rust output-client tests, strict Clippy and C's full
suite pass. Both SDKs carry `OUTPUT-CLIENT.md` with ownership, event-loop,
topology and recovery rules; matrix 38 / output map 7 record evidence limits.

The Rust output production-export peer now shares C's existing fixture above;
protected launch and revocation remain. The
scripted tests supply admission and do not prove Factotum grant revocation.
No implementation change or native performance claim is made by this checkpoint.
Production pins and the prepared t331 pilot remain unchanged.

Signed Rust `fadc53f` adds nonblocking output `Connecting::direct` and
`attached`, sharing the original deadline across version, fixed files and
negotiation. Three tests cover expiry, remote errno retention and Observe
validation. The integrated proposal test covers both startup paths, with and
without EAGAIN: exact candidate/control bytes, caller mutation, domain IDs 50
then 3, allocator 51, and transaction clunk/Submitted ACK barriers. Retry cases
exercise Connecting through one-time Session transfer. All 35 output-client
tests and strict Clippy pass; C's full suite passes. Signed C `034d426` records
its independent existing counterparts in matrix 37 / output map 6.

Terminal socket faults and recovery/ownership documentation are covered above;
independent paired production-export and protected-launch peers remain. No extra
clock or readiness syscall was added by this checkpoint. Native performance
savings remain unmeasured; production SDK pins and the t331 pilot are unchanged.

Signed Rust `f3a61c1` integrates the developing output session on one pipeline.
Three independent socket tests cover startup/negotiation, topology delivery
only after EOF/clunk, Refused only after its final ACK, and zero-budget/expired
startup without sending queued negotiation. All 31 output-client tests and
strict Clippy pass. C `ceb13ba` records the existing independent counterparts
and remaining gaps in matrix 36 / output map 5.

Keep performance in view while completing t333, as the owner requested.
The session uses caller time and one shared dispatch budget. Verified topology
ownership transfers without cloning vectors or labels; C already decodes into
fixed session storage. This removes a concrete copy but establishes no native
CPU savings. Preserve custody, fairness, deadlines and EOF/error behavior;
measure any performance claim separately under t331.

Proposal/retry, attached handoff and the connecting facade are now covered
above. Terminal errors and paired production-export tests remain required.
The Rust session is not yet fully qualified. Production pins
and the t331 pilot remain unchanged; no quiet hold is active.

Signed Rust `3f96b00` adds provisional publication-bound topology fetching.
Walk/open must match the announced Qid; the decoded object must match both
connection and topology epochs. Reads are bounded, and no value is exposed
before EOF and successful clunk. Withheld opens clunk before a new walk;
backoff never extends the original deadline. Five matched-reply tests cover
fragmentation, identity, malformed lengths, early EOF, retry and clunk failure.
All 28 Rust output-client tests and strict Clippy pass. C's full suite passes;
signed C `c532820` records its existing independent fragmented-fetch, withheld
open and wrong-Qid controls in matrix 35 / output map 4.

The session above joins output startup, submission, journal and topology
components with ordered reply handling, cumulative ACK barriers, Refused ACK
settlement, caller-clock dispatch and terminal custody retention.
Then add the nonblocking connecting facade and paired production-export tests.
The standalone fetch tests do not qualify integrated socket dispatch.

Signed Rust `34740d5` adds the provisional output journal. It checks granted
capabilities and revision, makes Refused an internal ACK target, retains ordered
application heads and validates custody behind a held head. ObjectPublished
blocks delivery and ACK progress until an object driver confirms its sequence,
Qid and topology epoch. Seven literal-wire tests cover these rules, all six
outcomes, malformed-record stop, fragment deadlines and bounded retention.
Signed C `67c8d95` independently checks all six outcomes and held-head ACK
ordering through both constructors. All 23 Rust output-client tests, strict
Clippy, formatting and C's full suite pass. Matrix 34 / output map 3 record
the scope. A test callback does not qualify the missing object fetcher.

The fetcher above now supplies publication-bound reads through EOF and clunk.
Integrate startup, submission, journal and ACK handling into the output
session and nonblocking connecting facade. Refused must settle its ACK before
terminal handoff. Production pins and the t331 pilot remain unchanged.

Signed Rust `a62d3f9` adds provisional immutable output submissions: Configure
and epoch checks, separate domain IDs and tickets, 64-ticket custody history,
same-ID retry, Submitted/ACK/clunk ordering and caller/assembly deadlines.
Output permits out-of-order domain IDs and has no WM EALREADY success exception.
Signed C `348efeb` independently checks both rules through direct and attached
socket startup; C implementation is unchanged. All 16 Rust output-client tests
(seven startup, nine submission), strict targeted Clippy, formatting and C's
full suite pass. Both SDKs record matrix 33 and output map 2.

The ordered journal above supplies grant intersection, explicit Refused ACK
targets and retained application heads. Rust's
submission tests supply matched replies directly; they do not qualify socket
dispatch or production output exports. The final connecting facade still needs
the nonblocking version handshake. Production pins remain unchanged.

Signed Rust `21feb45` starts `sophia-output-client` with bounded fixed-file
startup. `Opening::direct` accepts a versioned empty pipeline; `attached`
preserves the exact quiescent admitted root without reset/reattach. It validates
the exact output API and 72-byte Limits through EOF/clunk, then opens the three
role files with control-iounit checks. Observe is required locally; unsupported
revision requests remain for the server's explicit refusal. Seven socket tests
and strict targeted Clippy pass. Signed C `4f64371` records matrix 32 and the
new `OUTPUT-PARITY.md` operation map. This is not a complete output session.

The submission implementation above follows C's
`output_session/submission.c` behavior: Configure is required,
domain IDs are distinct from tickets, and Submitted ACK plus transaction clunk
must settle before the next transaction. The final connecting facade still
needs the generic nonblocking version handshake. Keep output's explicit
Refused state and grant/ACK/replay rules, rather than copying WM semantics.

Signed Rust `8fe1c1a` adds `sophia-output-protocol`, extracted from signed
Sophia `ad2fa4962ba1e7626b601e1a5b6b0ab95324bfc3`. Its 21 source hashes verify;
the output schema and contract match C's pins byte for byte. The extraction
retains passive values and codec validation, with no device or topology owner.
All 30 extracted tests, the full Rust workspace suite and targeted strict
Clippy pass. The spec hashes pass from `spec/`; the initial repository-root
invocation used the wrong base, and the new hash rows were corrected to the
existing relative-path convention. Signed C `a88efe7` records matrix 31.

The codec foundation does not establish connection behavior. The developing
client follows the C session operations below as its checklist.
The output role differs from WM: explicit negotiation Refused state, Observe
required, granted Configure, publication/Qid matching before exposing and ACKing
ObjectPublished, bounded ACK progress, and domain replay capacity from Limits.
Keep these semantics distinct instead of mechanically renaming the WM driver.

Signed Rust `9890ed8` and C `afd9f90` check all nine profile completion
kind/outcome combinations through independent sockets. Literal inspection
confirms transaction/generation/digest echoes and outcome bytes, followed by
Submitted and ACK settlement. The policy transaction allocator remains at 1.
Rust's journal now accepts lifecycle/chord Cycle causes only with their
negotiated capabilities, matching C's existing socket controls. All 45 Rust
WM tests, strict targeted Clippy and C's full suite pass. Matrix 30 / WM map 6
cover the implemented WM operations; publication-wide qualification remains.

C's `sophia_output_session.h` owns the output comparison:
direct/attached startup, observe-required negotiation with explicit Refused
state, immutable proposals/custody, automatic topology fetch before publishing
ObjectPublished, Qid/epoch/topology identity, ACK/clunk barriers and bounded
caller-clock service. The neutral protocol crate above uses Sophia's
`output_files.rs` and its six modules, passive `output_role.rs` and
`packets/output_configuration.rs`, with shared typed IDs/geometry and the pinned
output schema. Existing `output_file_{events,topology,limits,schema}.rs` and
`output_files.rs` tests are the extraction controls. Preserve candidate
structural parsing versus server-owned topology validation; no Engine authority
belongs in the SDK. Record source hashes and exact signed source revision.

Signed Rust `0681b4a` and C `016405a` independently check delivery of all three
profile commands, all five configuration/projection/session-operation outcomes
and all three presentation outcomes through their socket clients. Literal wire
bodies avoid using the encoder under test. Held heads survive dispatch, ACKs
never skip them, and semantic Disconnected outcomes leave the transport Ready.
All 43 Rust WM tests, targeted Clippy and C's full suite pass (the final rename
of the Rust fixture scenario enum was followed by its session suite).
Matrix 29 / WM map 5 retain the limits: these are ordered observation tests,
not execution of application-owned rollback policy. Do not require SDKs to own
the application's rollback decisions.

Signed Rust `d660990` and C `fe67eb3` add independent protected WM peers.
Sophia's `policy_file_protected_authentication.rs` accepts an explicit absolute
`SOPHIA_WM_AUTH_PEER` path while retaining its raw-protocol control. Both SDK
peers pass the production protected supervisor/Factotum admission, transfer
their exact admitted root into WM startup, settle configuration-capability
negotiation and observe terminal closure/staleness after registration revocation.
Confirmed negotiation custody remains readable. The shared Factotum stays ready.
The native Rust run and the C run both passed; the raw control also passed.
Strict Rust SDK Clippy and C's full `make check` pass. Matrix 28 and WM map 4
record the build/run commands. This proves idle revocation, not revocation with
application candidates in flight or automatic recovery.

Built peer hashes (no executable copies): Rust
`~/.cache/cargo-slots/sophia-desktop-sdk-rs@sdk-rs-t331-reply-storage/1/debug/examples/wm_authenticated_peer`
SHA-256 `9b154d801f9fbbef49fe658c45a1844323efa9038e92b4942ebbdc26bdbb84e3`;
C `~/dev/sdk-c-t318-namespaces/build/wm_authenticated_peer`
SHA-256 `712dcc04042a8b65d1593574935aded8a2ff955c81a48b485d73eec0ecfaa550`.
The first Rust attempt requested the wrong capability bit and was refused by
Limits validation; the fixture now uses the named configuration constant.

Signed Rust `1594612` and C `3809590` add paired terminal checks and the shared
`WM-CLIENT.md` ownership/error guide. Independent socket peers cover short ACK,
ACK EINVAL/ESTALE, and event-read revocation before/after Submitted. Confirmed
custody survives; unconfirmed issued work becomes UnknownDisconnected. Remote
errno, terminal state and custody survive explicit close. All 42 Rust WM tests,
targeted strict Clippy and C's full `make check` pass. Matrix 27 and WM map 3
distinguish these scripted checks from production grant revocation.

Signed Rust `3228a06` adds startup status/error/custody inspection and the
independent `wm_conformance_peer` example. Both C and Rust now pass the same
production WM adapter/worker exchange: profile prepare/activate, configuration,
a 64-surface snapshot spanning multiple reads, projection, session operation
and receipt delivery. Signed C `324b2b1` records matrix 26 and WM map revision 2.
All 41 Rust WM tests, the full SDK workspace suite and targeted strict Clippy
pass. Startup checks preserve remote errno and ESTALE through explicit close.

The Sophia fixture uses its pinned C `ba09265`; C WM code is unchanged in the
candidate. The Rust test explicitly requires an absolute prebuilt peer path;
it does not change production dependencies or verified pins. Run with
`SOPHIA_WM_RUST_SDK_PEER=/absolute/path/to/wm_conformance_peer cargo-slot sophia cargo test -p sophia-session --features native-session --lib sdk_drives_profile_configuration_snapshot_and_policy_exchange -- --include-ignored --nocapture`.
Both tests passed. Without `native-session`, the filter runs zero tests and
provides no evidence. The first Rust run correctly refused invalid fixture
chrome (disabled ring with nonzero width); the peer now sets width zero.
The verified peer binary at
`~/.cache/cargo-slots/sophia-desktop-sdk-rs@sdk-rs-t331-reply-storage/1/debug/examples/wm_conformance_peer`
has SHA-256 `945af0642b5ab0fe976a6ad382fa28e4642b3834599ba175a60be266476d5ed8`.
No executable was copied into evidence.

The exchange test supplies admission and outcomes; protected admission is now
covered separately above. Engine settlement and actual presentation remain
separate claims. Publication/pin adoption and the other t333
roles (output, lock, administration and broker/portal audit) remain required.

Signed Rust `228f263` starts `sophia-wm-client` with typed application candidates
and owned immutable encoding. Five tests cover all seven kinds, literal Dirty
bytes, caller mutation, identity/capability rejection and separate domain IDs;
targeted tests and strict Clippy pass. Signed C `4070e29` maps the counterparts
in matrix 18. This prepares submissions only; the transport state machine is
still required. In C, Submitted is validated before interpreting submit replies,
and its ACK must settle before opening the next transaction. Preserve both
ordering rules when implementing the Rust driver.

Signed Rust `b71abfb` adds `sophia-wm-protocol`: neutral WM codecs and passive
policy records extracted from Sophia `1d071c144`. Its extraction manifest's
39 source hashes verify against that revision. The WM schema, API/file docs
and golden corpus match C's pinned bytes; generated WM rows are unchanged.
All 71 extracted tests, the full Rust workspace suite, strict Clippy and C's
independent suite pass. Signed C `b6a99a3` records the comparison in matrix 17.

Next is the Rust WM session client: direct and authenticated startup, bounded
caller-clock dispatch, ordered retained events, submission/ACK barriers,
same-ID retries, custody retention and Cycle-bound snapshot fetching. Codec
tests do not establish session parity. Output, experimental lock, administration,
broker/portal audit, paired production-export tests, foreign-language binding
smoke and signed publication/pin adoption remain required. Production SDK pins
and the prepared t331 pilot are unchanged. No measurement hold is active.

Signed Rust `e2dfdc6` adds explicit local close, retaining custody and decoded
observations without I/O; the native facade also disarms lifecycle authority.
Tests distinguish Submitted, uncertain submission and unsent tails, and cover
idempotence, held-credit release and retained-input cleanup. Full workspace
tests (43 native), strict Clippy and C's full suite pass. Signed C `997f22d`
records the paired existing close controls. Matrix revision 16 maps shell
operations and explicitly distinguishes Rust's automatic snapshot/journal
management from C's manual controls. Rust also lacks a separate allocation-free
encoded-size query; prepared admission supplies a different pre-effect check.
These differences require production-export review, not a silent parity waiver.

Signed Rust `513cb31` adds configurable queue record/byte limits and optional
control headroom. Defaults remain 64 records/2 MiB, reserving 32 records/8 KiB
from bulk use. Zero headroom selects a shared pool; every C queue size is
representable. Configuration requires a live empty queue with no delayed
reservation. Invalid limits and busy resizing leave settings unchanged.

Signed C `804c5f2` tests a two-slot queue and documents handoff accounting.
C copies the front into separate transaction storage and releases queue credits;
its limit excludes that single in-flight record. Rust transfers ownership and
keeps the record charged until settlement. Identical settings therefore do not
promise identical admission timing or memory use. Both remain bounded. Full
Rust workspace tests, strict all-target Clippy and C's full suite pass. Both
API matrices are revision 15; production-export qualification remains pending.

The shell implementation now includes external-loop readiness and bounded
caller-clock dispatch; prepared and serviceable delayed reservations; atomic
content and general-role batches; custody retention queries; native transport,
keyboard/focus, pointer, allocation/resource/pacing lifecycle; an owning native
session with retained heads and scheduling hints; presentation metadata and
reserve-before-edit callbacks. The SDK provenance files retain individual
signed revisions, tests and limitations. Raw batches validate records and role
support but do not establish action eligibility or register lifecycle state.
Local atomic admission never implies atomic server effects.

The owning Rust facade applies fetched catalog/facts generations when returning
the observation; install its contents before presenting. C explicitly marks
installed snapshots. Rust uses transport `Instant` and host-monotonic
milliseconds; neither clock may go backwards. Scheduling deadlines do not
settle server authority. Prepared commit needs no allocation or I/O; delayed
commit can fail while preserving credits. Cancel unused delayed tokens.
Input callback panic fails Rust's lifecycle closed before resuming unwinding,
preventing replay of potentially applied external edits.

Next, implement the Rust WM client against the pinned contract and C's coverage
checklist, retaining the shell control differences above in the final audit.
Output and experimental lock clients follow. The full task also requires
administration coverage in both SDKs, the public broker/portal audit, independent
paired production-export gates, a foreign-language binding smoke test, signed
publication and verified Sophia pin adoption. Unsupported declarations cannot
waive that accepted role scope.

C's earlier reservation identity repair (`a90e235`) changed its public token
layout. The candidate remains **0.10.0-dev, unreleased**; C consumers and
foreign-language bindings must rebuild together. Do not move or reinitialize
session storage with a live reservation. Mandatory parity rules are installed
in Sophia's agent instructions and both SDKs.

Sophia's production SDK pins and prepared t331 CPU pilot remain unchanged.
No quiet hold is active. C helper tests use a session stand-in; current checks
are SDK-level evidence, not production-export, physical or performance
acceptance. t333 remains open.

## SDK platform policy

Approved by niltempus on 2026-09-27: design both native SDKs for Linux and BSD,
with Linux as the first supported platform and FreeBSD as the next qualification
target. Keep byte codecs, value validation, queueing and lifecycle logic free of
OS-specific APIs. Confine peer credentials, socket flags and signal handling to
small platform adapters. Preserve the distinction between the 9P2000.L wire's
error numbers and the host's errno values.

FreeBSD support requires native build, protocol, socket, retry, revocation and
peer-authentication tests in CI. Cross-compilation alone is not qualification.
Record OpenBSD and NetBSD separately when native runners and equivalent tests
exist. No general BSD support claim follows from portable source or a Linux
test pass. Initial Linux daily-driver adoption does not wait for BSD runners;
broader Sophia server/compositor portability remains t256.

## Extraction and completion

Claude extracts Rust blocking/pipelined 9P clients and neutral value types, and
the shared shell protocol crate used by the SDK and Sophia server. Server
connection/export/journal/admission owners stay in Sophia. B6c completes
catalog/indicator objects, outcomes, candidates and activation responses, and
fixes submit EAGAIN handling and custody tracking. Keep staging available after
EAGAIN; clunk after a successful submit reply cannot undo transferred custody.

Codex extracts C transport/codecs/session support, preserving the existing
same-ID EAGAIN retry. Add reusable lifecycle handling, bounded multi-record
queue admission, explicit connection selection, poll interests/deadlines and
terminal errors. Share lifecycle logic through typed values; keep UI/rendering
in applications. Retain IPC compatibility backends in both SDKs until t255.

Local queue admission, Submitted custody, semantic outcome and presentation
remain distinct. A sent request without observed custody can have unknown
outcome; never replay across epochs. Resolve EALREADY from tracked state or
fail closed. Bound raw object buffers and owned decoded data separately; never
ack past unresolved retention obligations.

SDK tests carry immutable specification copies with source revisions and
digests, verified by Sophia's integration gate. Pin Rust dependencies by exact
revision and lockfile and provision verified sources before offline gates.
C consumers use immutable snapshots with upstream revision and per-file hashes.
No gate fetches sources or reads a mutable external worktree. SDK release ->
explicit source/spec pin update -> full gate. Compatibility manifests include
SDK version, source hash, dialect, file API version and role revisions/masks.

After a tested SDK commit is pinned, Codex adopts C in Bemenu; Claude assigns
disjoint Lom and Provlita lanes. Preserve Bemenu `7d2d239` and its five unpushed
commits. Avoid Lom's active `test/t099-content-lifecycle` branch; coordinate
Provlita's Lom GPU-helper pin. Add production-export tests to all three clients.

Exit of the first release: both SDKs build independently, declared shell
coverage passes against the production export, source/spec provisioning is
reproducible offline, and applications pin the required releases. Retain literal
vectors, pipeline regressions, malformed/partial traffic, retry/disconnect,
bounded-memory, maximum-object and revocation controls, and the independent
Go oracle's exact 96-check verdict. Native rendering, installed rollout and
performance acceptance remain t252 gates.

Codex directs tracking, normative changes and integration review.
Claude owns Sophia-side Rust dependency integration; Codex owns C snapshots,
Bemenu and the oracle. Use isolated worktrees, reusable private targets,
signed commits and herdr handoffs. The
[build and test coordination policy](../../build-and-test-coordination.md),
updated on 2026-10-03, allows concurrent ordinary builds and correctness gates.
Never reset gpg-agent. Authentication may
block publication but does not block local implementation.

## Application adoption, 2026-09-27

Both SDK repositories are published under sophia-org. All three applications
now pin them:
- Bemenu vendors C `8decca1d`.
- Lom `53d3a921` pins Rust `ea9cf651`.
- Provlita `0942e07` pins Rust `ea9cf651`. It runs 9P-only on its catalog
  profile, with custody observation and native candidate budgets. It passes its
  full offline gate, and negative controls cover refusal and budget handling.
  Its evidence is under `development-evidence/provlita-9p-only/`.

Sophia's vendored Rust snapshot moves to `ea9cf651`. That revision is
client-only: it adds readiness wakeups and a scripted test, and changes no
contract file. `cargo xtask check rust-desktop-sdk` passes, as do the layout
check and the nine production-export tests in `shell_client_b6c_live`. The logs
are `sdk-rust-gate/{revendor,b6c-live}-ea9cf651.log`.

The application tests use scripted peers. Live runs of each product against a
real Session export belong to the external integration repository. They also
carry native rendering and attended acceptance, which stay with t252.

## First release and closure, 2026-09-27

Both SDKs have published their first releases:
- `sophia-desktop-sdk-rs` v0.1.0 is signed tag `v0.1.0` at `ea9cf651`. That is
  the revision Lom, Provlita and Sophia's vendored snapshot pin.
- `sophia-desktop-sdk-c` v0.1.0 is `4cfee26`. It declares `release=true` and
  `wm_files=true` (WM file API 1). The WM condition in `src/README-wm.md` is
  met: the production WM export gate passed at `c4e17899e`, and Hagia uses the
  SDK through thin bindings (`b3d8496`, 449 tests).
- The C source is identical to `8decca1d`, which Sophia, Bemenu and Hagia vendor.
  Only the documentation, the manifest and the version string differ.
- `make check` passes with and without IPC compatibility. The log is
  `development-evidence/sdk-release-0.1.0/c-check.log`.

The first-release exit is met. Both SDKs build independently. Their declared
shell coverage passes against the production export, including the Rust live
suite at `ea9cf651`. The vendored sources are provisioned offline. Bemenu, Lom,
Provlita and Hagia pin released code.

At the operator's direction, Provlita's production-export and live check moved
to Provlita t007. Output and admin file clients remain explicit gaps under t253,
t254 and t270. BSD qualification remains under t256. Native rendering,
measurements and attended acceptance remain under t252.

## Dependencies and connections

### Repository boundary cleanup before the installed session

On 2026-09-27 niltempus required Sophia to remain independent of particular
shell and WM products, including tests and release tooling. The required rule
is in `docs/style-guide.md` and the Sophia agent instructions. Generic SDK and
server conformance stays in Sophia; actual desktop assembly belongs in
`sophia-desktop-integration` or the personal installer, and UI behavior belongs
in the client repository.

The first relocation moves Bemenu artifact preparation and the live Bemenu
test to the independent integration workspace at signed revision
`d39c351fa72d30eb19e1da9d064bc49e23ad8373`. It pins public Sophia crates at
`9fcaec782ce4fe9978568c0466ee17a78b3d4571`, with a committed lockfile and
explicit offline provisioning. Its artifact refusal tests pass 4/4, pin tests
5/5, and the live Bemenu check passes with two openings, three candidates,
one edit and one activation. The source is Bemenu `52a6e309`, the SDK snapshot
is `a0ab8c8`, and the prepared binary SHA-256 is
`c2b16fabd9564d506548e3181f8903abb6ee0a72a0c56651b890276f37f5d00d`.
The live log is `bemenu-files/live-52a6e30-integration.log` under development
evidence. Session decisions remain scripted; this does not prove physical
rendering, expiry behavior or launch policy.

The remaining cleanup includes product-specific discovery and preflight,
WM launch environment names, named-stack build and installed-session tooling,
and product evidence gates. Relocate each gate with its inputs and negative
controls before removing its Sophia entry point. Preserve generic policy,
shell, protected-GPU and configuration checks. New release preparation waits
for matching external adapters and WM support; an explicit checker must
produce `policy=validated` wherever the desktop previously required validation.
Publication and attended acceptance are separate from these local gate results.

Installed wrappers must emit `SOPHIA_DESKTOP_PROFILE_MODE` before a release uses
the generic reader. Do not prepare a release from an intermediate cleanup tree
with the old wrappers. Keep the personal installer's user-owned WM and reload
workflow; an immutable WM pair used for integration packaging does not change
that default.

Preserve IPC rollback coverage alongside file-wire tests. External Bemenu
acceptance covers both wires and the production Session launch, process custody
and settlement path before the remaining product-specific tests leave Sophia.
Whole-desktop recipes, physical runners and their negative controls move
together. Sophia retains generic protocol checks, input/TTY safety primitives
and contract-derived configuration fixtures.

The [boundary cleanup checkpoint](../milestones/htxttn94-desktop-boundary-cleanup-checkpoint-before-integration-relocation.md)
records the explicit policy-checker seam, generic WM environment and discovery,
personal installer compatibility checks, and sibling-free verifier fixtures.

The [extraction checkpoint](../milestones/i7pfnyzy-desktop-sdk-extraction-checkpoint-before-application-adoption.md)
records the first merged C snapshot, validation and the scope still unclaimed.

Prerequisites are the accepted t251 contract and reviewed t252 foundation, now
merged. This replaces the former dependency on completed t252 acceptance.
The initial SDK release supports t252 application adoption. Keep t249/t250
qualification open and t254's dependency unchanged. Later SDK role modules
follow their accepted contracts without claiming unimplemented coverage.

- [9P migration plan](jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
  owns role acceptance and compatibility retirement.
- [Shell file contract](../../sophia-shell-files.md) owns protocol behavior.
- t258 developer documentation and t260 reference clients complement packaging;
  this task owns library extraction, releases and consumer dependencies.
