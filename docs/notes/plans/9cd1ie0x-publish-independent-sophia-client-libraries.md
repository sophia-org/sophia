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
