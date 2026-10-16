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

Signed Rust `cb34448` extends immediate, prepared and delayed batch admission
to native, descriptor, indicator and catalog operations through `ShellBatchItem`.
Validation/size failure keeps delayed credits; prepared Drop cancels. Each whole
item retains its own ticket and remote outcome. Role negotiation checks remain
in force. These raw operations do not register lifecycle state or establish
action eligibility. Signed C `5936c9f` independently tests a delayed descriptor
pair through its session peer. Both matrices are revision 14. Full Rust
workspace tests, strict Clippy, final descriptor/scripted checks and C's full
suite pass. Production-export qualification remains pending.

Signed Rust `d995058` adds borrowed `presented` metadata and reserve-before-edit
convenience on both the connection and owning session. Both ACK dispositions
are encoded before the callback; selecting its result and committing require no
allocation or I/O. A panic fails the lifecycle closed before resuming unwinding,
preventing a caught panic from replaying an edit. Signed C `058207a` adds the
independent declined-edit control. Both matrices are revision 13. Full Rust
workspace tests (38 native), strict all-target Clippy and C's full suite pass.

Signed Rust `47063e7` adds `NativeLauncherSession`: exclusive connection
ownership, ordered retained events, transaction IDs, native operations, bounded
dispatch and scheduling obligations. Repeated reads apply a head once; required
ACKs cannot be skipped. Prepared response Drop preserves the event and ID range;
commit consumes them without allocation or I/O. Input, action, allocation,
candidate and permit deadlines remain hints, never server authority. Rust uses
transport `Instant` and host-monotonic milliseconds; C uses milliseconds.

Signed C `61031c0` adds an independent check that repeated Action reads do not
restart the receipt-based hint. Both API matrices are revision 12. Full Rust
workspace tests, strict all-target Clippy, the final 35-test native suite and C's
full suite pass. Five Rust facade controls cover initialization, retained input,
response cancellation, transaction exhaustion, clocks, disconnect and Action
hints. C borrows caller-owned session storage; Rust owns the connection. Rust
applies fetched snapshot generations on observation, so the caller must install
the contents before presenting. C explicitly marks installed generations.

Earlier signed Rust `7ad2f9f` adds pointer responses. Activation can name any
eligible displayed row; activation and ACK enter the queue together but retain
separate custody outcomes. C `edc9259` supplies the independent nonselected-row
control. Rust `d65d8da` supplies resource/allocation/pacing lifecycle and fixes
refused-Begin upload slot leaks and queued-payload stalls. C `b1aae57` fixes an
indefinite wait when permit expiry arrives before candidate refusal. C `f580a2d`
and Rust `393a686` cover explicit-resource cancellation after refused End.

The SDK provenance files retain previous transport, keyboard/focus, custody,
atomic batch, reservation and event-loop checks. Local admission does not prove
server effects. Prepared commit needs no allocation or I/O; delayed commit may
fail while retaining credits. Explicitly cancel unused delayed tokens.
Simulated-time callers must use `dispatch` and non-driving receives.

Next, close configurable queue parity and audit the operation matrix. C's
`sophia_ns` supplies the checklist; the pinned contract defines semantics.
The remaining task includes Rust WM, output and lock clients, administration
coverage in both SDKs, the public broker/portal audit, paired production-export
gates, a foreign-language binding smoke test and signed release adoption.

C's earlier reservation identity repair (`a90e235`) changed its public token
layout. The candidate remains **0.10.0-dev, unreleased**; C consumers and
foreign-language bindings must rebuild together. Do not move or reinitialize
session storage with a live reservation. Mandatory parity rules are installed
in Sophia's agent instructions and both SDKs.

Sophia's production SDK pins and prepared t331 CPU pilot remain unchanged.
No quiet hold is active. C helper tests use a session stand-in; all current
checks are SDK-level evidence, not production-export, physical or performance
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
