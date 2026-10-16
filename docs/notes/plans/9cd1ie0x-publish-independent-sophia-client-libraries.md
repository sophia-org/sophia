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
No binary was copied to evidence. Next qualify protected output launch and
revocation with independent peers, then continue experimental lock,
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
