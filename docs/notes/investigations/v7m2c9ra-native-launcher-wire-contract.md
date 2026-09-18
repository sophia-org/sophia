---
id: v7m2c9ra
date: 2026-09-17
kind: investigation
status: implementation
tags: [shell, launcher, protocol, bemenu]
---
# Native launcher wire vocabulary and retained lifecycle obligations

This implements the byte vocabulary for
[the independent launcher contract](../decisions/f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md).
It does not enable a second live component. Session negotiation remains capped at
revision 6 and the configured independent launcher remains explicitly refused.

## Inventory and compatibility

The complete IPC inventory ended at kind 186 and shell capability bit 10. The
extension allocates kinds 187–197 and bit 11, since revision 7. Frame version and
header remain unchanged. All new records require a nonzero transaction, little
endian fields, no implicit padding and zero reserved bytes. Unknown, oversized,
truncated and trailing fields refuse. Connection/grant epochs and all exact
binding identities are nonzero; no null binding stands for a current snapshot.

The native role requests exactly catalog/content/discrete-input/native-launcher
bits (5,7,8,11; `0x9a0`), with minimum revision 7. It gets no descriptor, reservation,
indicator or GPU authority from that request. Existing r1–r6 requests retain their
meaning. C negotiation can encode this vocabulary but cannot grant it; the live
runtime's implementation intersection still excludes revision 7.

| Kind | Record | Payload bytes |
|---|---|---|
| 187 | Opening | 56 |
| 188 | AllocationRequest | 84 |
| 189 | CandidateBegin | 108 + 2 × rows (at most 32) |
| 190 | CandidateChunk | 40 + 64 × surfaces + 32 × placements + 48 × targets |
| 191 | Focus | 104 |
| 192 | FocusRevoked | 108 |
| 193 | Input | 132 + text bytes (at most 256) |
| 194 | InputAck | 124 |
| 195 | Activate | 124 |
| 196 | ActivationOutcome | 128 |
| 197 | Closed | 28 |

The schema encodes exact field order. `NativeLauncherBinding` includes grant,
opening, output/generation, allocation/generation, catalog, candidate, presentation,
interaction and state revisions plus a fresh focus lease. Event adds event ID and
resulting state revision. Activation adds an explicit cause family and catalog
slot; keyboard-event and content-action numeric IDs are not interchangeable.
Outcome echoes the complete activation and has its request transaction.

## Candidate and allocation ownership

Kind 188 is a parentless transient request; operation 1 acquires, 2 resizes and 3
releases. Resize/release name an existing allocation. Edge is the existing 1–4
placement preference and margins retain their signed bounds. Session chooses
actual placement on the opening's authorized output. Existing kind 164 returns
its allocation, with zero parent, anchor and allowed reservation. There is no
fabricated parent and no work-area claim.

Kind 189 replaces ordinary Begin in the SAME owned candidate transaction. Its
first 80 bytes are the existing Begin; opening/catalog/state revision and ordered
rows follow. One surface, at least one placement, and one target per visible row
are required. Empty results have zero rows/targets/selection; nonempty results
select exactly one included slot. Rows are unique, in 1–4096 and at most 32.

Kind 190 retains the existing chunk byte layout but explicitly permits only
surface role 3 (parentless transient) and target action kind 2 (catalog row).
Surface parent is absent, anchor zero and reservation zero. All placements and
targets use surface index zero. Target action ID names the catalog slot. Runtime
must match targets, in candidate order, against the exact ordered Begin rows and
published catalog; a codec cannot establish that cross-record fact. Resource,
pacing, End, outcome, Presented, discrete action and release records are reused.

The existing kind 173 codec continues rejecting these new roles/action kinds.
This avoids widening old clients' authority as a side effect of learning a new
payload. Runtime assembly must attach row binding to its actual candidate owner,
not keep a second pending candidate or independently acknowledged metadata queue.
Every native byte/record consumes existing negotiated aggregate limits, including
the larger Begin and its retained response credit.

## Input, activation and pending presentation

Opening starts at state revision 1. Session grants Focus only for the exact
Presented binding; initial rendering and Prepared cannot acquire keyboard input.
Text is committed UTF-8 with the catalog's control/bidi-formatting exclusions.
Semantic commands are numbered explicitly in the schema. There is no clipboard,
raw keycode, arbitrary return-string execution or helper channel. Escape and
outside dismissal remain Session cancellation, not client-generated key authority.

Edits/navigation advance Session's issued state revision and disarm activation
until that exact resulting model presents. Each input carries its original
binding and the resulting revision. ACK retains the exact issued event even if
new content or focus is later installed. Accept and Activate instead require
revision equality with their presented binding. A matching codec shape is not
proof an Accept event was issued, acknowledged or eligible.

To avoid losing Enter during pending repaint, the owner integration must retain
one bounded Accept intent with original issuance time and reserved control credit,
scoped to the opening and latest issued state revision. It may issue Accept only
after that exact state presents and receives its new focus lease. A later edit,
cancel, replacement or revoke invalidates this intent; it cannot migrate to a
newer selection. This is retained input, not early launch authority. Pointer
press/release is likewise never retargeted from old pixels to a new row.

Activate cause 1 must match an issued Accept and its selected slot; cause 2 must
match the exact issued ContentAction/target. The shared admission owner validates
current catalog, lease, presentation and pending event before enqueueing policy.
ACK and admission remain orthogonal. Outcome statuses are admitted/stale/unknown/
unauthorized/capacity (1–5); only admitted has reason zero. Admitted means queue
insertion, not application startup. No receipt, retry or reconnect may replay it.

## Evidence and remaining work

Rust validates typed encoding and decoding. Independent C code parses the byte
fields without linking Rust. Controls cover every fixed length, all truncations,
zero identities, selection/count/role disagreement, reserved bytes, invalid UTF-8,
control characters, bounded rows/text, activation cause and presented revision.
The golden corpus covers all eleven kinds; byte mutations compare acceptance
classes across the two implementations. Sanitizer evidence is payload parsing,
not native execution or focus/launch lifecycle evidence.

Still required before admission: exact candidate-store attachment and credits,
role-specific protected negotiation, two supervised live services, transient
composition, presented focus/input ownership, catalog/launch integration, C client
lifecycle and Bemenu backend, formal controls, full joined headless integration
and separately attended acceptance. This checkpoint does not make `lom-test`
ready for a native launcher.

Scoped validation before freeze: 167 protocol tests pass in a device-hidden
namespace; strict protocol all-target Clippy, formatting and source layout pass.
The independent C framing/catalog/native suite passes. Clang ASan+UBSan validate
all eleven native golden records and 3,734 byte-mutated records. Two separately
compiled Rust mutations fail their intended controls: accepting an unpresented
revision and widening legacy kind 173 to native roles. Both sources were restored
byte-for-byte. These are codec controls with supplied bindings, not real Presented
or keyboard delivery. A first sanitizer build lacked GCC sanitizer libraries and
a first isolated compiler run lacked private include paths; neither counted as a
pass. Corrected Clang and bounded include mounts produced the retained passes.
Evidence is `.artifacts/bemenu-native-wire/` in the coordinating root checkout.
Exact-source canonical validation remains a subsequent checkpoint gate.

## Native store ownership over 795a3d60

The runtime now has an immutable per-epoch native launcher storage profile. It
uses the existing allocation, resource, permit, candidate and response-credit
owners. Legacy constructors remain legacy, and their public request methods
refuse native records. This does not enable revision-7 transport negotiation.

Session-supplied opening identity stays on the pending and granted allocation.
Native allocations are parentless, limited to one per grant, and reserve no
workspace extent. A mismatching grant decision leaves the original proposal
owned. Candidate Begin attaches bounded ordered catalog slots and selected row
to the actual assembly; End checks current catalog availability, issued model
revision and exact allocation opening, and row targets must match that order.
The larger Begin and row bytes count against the candidate byte limit before
chunk admission. Renderer handoff rechecks current provenance before transferring
ownership and carries that same row binding with the real resource leases.

Chunk/End use the existing grant-wide candidate-generation correlation. Terminal
responses retain the original Begin transaction; this slice does not add a new
transaction-equality rule to the legacy assembly contract. Prepared and Presented
remain distinct caller-supplied renderer transitions, not inferred outcomes.

Ten device-hidden integration controls exercise the actual stores, including a
held submitted launcher bundle across disconnect while a separate bar uploads
under its own grant. Replacement waits for the held bytes; releasing them admits
the replacement without changing the bar's resource. The control supplies the
renderer failure/completion boundary and does not drive native rendering or bar
actions. Four separately compiled mutations fail the intended tests: omitted
allocation-opening comparison, ignored catalog row order, ignored current model
revision and omitted native metadata byte charge. Mutations run in an isolated
source copy and are restored; no live source is mutated. Evidence is retained in
`.artifacts/bemenu-native-stores/`.

The first test run had two incorrect fixture expectations that omitted the
already-owned ResourceReleased credit. The corrected assertions retain that
credit alongside candidate responses; production accounting was not weakened.
Protected role negotiation, live supervision, geometry/composition publication,
focus/input/activation and the Bemenu C backend are still required. These store
controls do not establish readiness for `lom-test`.

## Role transport over 009415a2

The independently constructed Session component owner now selects the immutable
store profile from its configured role before peer admission. A native launcher
must negotiate revision 7 and exactly application-catalog, content-surface,
content-discrete-input and native-launcher capabilities. It cannot request
indicator, descriptor-launch or switcher authority. The existing operator
content policy must also grant discrete input. Unavailable and denied remain
distinct encoded refusals, and refused negotiation retires only its reservation.
Legacy and live single-shell startup retain revision 6. The independent live
profile remains refused pending full supervision/focus/client integration.

Native resource, pacing, allocation and candidate intake shares one visit of at
most the negotiated record limit (capped at 32) and 64 KiB payload processing.
Each socket direction is additionally bounded to 64 KiB and retains partial
framing/writes. Oversized negotiated payloads refuse before record removal.
This is a per-connection content bound, not completed owner-loop fairness across
input, actions, supervision or renderer work. A new allocation/resource/demand
must have aggregate response capacity before inbox removal; candidate permits
already own their response credits. Resource transitions and event transfers
are shared with the legacy path, not duplicated resource ownership.

The native service uses Session-supplied opening, current catalog and issued
revision with the actual stores. Opening notifications and content outcomes use
the existing FIFO. The borrowed Session connection exposes the same methods as
the actual component owner. No focus, text issuance or application activation
is inferred from successful negotiation or supplied renderer completion.

Eight device-hidden socket controls cover the complete allocation/upload/pacing/
candidate roundtrip with supplied geometry/catalog and renderer completion,
exact source retention through disconnect, role widening, operator refusal,
wrong grant/legacy allocation, stale catalog, mixed-record bounds and buffered
payload continuation and EOF after a buffered request. The EOF control retains
the actual pending allocation until explicit disconnect; EOF alone does not
claim successful completion. Session's two-peer controls now use a real revision-6 bar
and revision-7 launcher, retaining old resources while the bar progresses. Their
protection evidence is supplied; no protected child, native presentation or
actual launch runs. The buffered-byte control preloads the inbox so the socket
read bound cannot hide a missing dispatch-byte bound. Evidence is retained in
`.artifacts/bemenu-native-transport/`.

Native text input still needs an explicit bounded response/issuance owner. Its
132 fixed payload bytes plus up to 256 UTF-8 bytes exceed the legacy 256-byte
control envelope. That next integration must reserve the complete frame through
FIFO/partial writes; this slice does not send native text as unreserved bulk.

Six independently compiled negative mutations in the disposable source snapshot
fail their intended socket controls: accepting widened role capabilities,
ignoring operator refusal, accepting a foreign grant, removing the payload bound
doubling the negotiated record bound and suppressing EOF notification. The source snapshot is restored after
each mutation. These are deterministic private-socket accounting/role controls,
not evidence of natural kernel saturation or physical input dispatch.

## Native focus and input over 6c65ccad

The transport now retains non-owning metadata from the actual candidate store's
successful Presented transition. Prepared cannot install focus. Focus names the
exact grant, opening, output, allocation, catalog, candidate, presentation,
interaction, state revision and a checked connection-local lease. The matching
Presented outcome enters the common FIFO first. A newer Presented disarms the
previous focus until the matching new focus is installed; stale callbacks must
also supply and match their original focus or opening, rather than acting on
whatever happens to be current.

Native control records use a 512-byte accounting envelope, sufficient for the
largest bounded UTF-8 input. Opening reserves its future Closed response; focus
reserves FocusRevoked; an unsent Enter reserves its eventual Input response.
These credits share the existing aggregate record/byte limits with resource,
candidate and FIFO owners. Encoding/refusal retains the producer; successful
enqueue is followed by Copy-state transfer without callbacks or I/O. Close
disarms first and retains refused notifications for polling. This is a returned
failure contract, not panic recovery or proof of displayed-resource withdrawal.

Sixteen fixed input receipts retain exact events across model replacement.
Wrong or duplicate ACKs cannot settle a different receipt. ACKed Accept metadata
remains distinct from application admission, which is not implemented here.
Enter while edits are awaiting presentation is retained against that exact
revision, with its original transaction and issuance time. Only matching focus
can issue it; another edit cancels the intent. Empty selection cannot retarget it.
Native candidate intake and renderer handoff check the transport's issued state,
not merely a caller-supplied revision.

The monotonic deadline visit checks unacknowledged input against the negotiated
ACK timeout and pending Enter against the presentation timeout. New issuance
also visits it. Expiry closes the exact opening; clock regression or a stale
opening cannot close a successor. Idle timeout enforcement still requires the
Session owner to call this visit before input/ACK service. Pixel consumers remain
owned independently; no resource release or launch is inferred from closing.

Nineteen private-socket controls pass, eleven new in this slice. They use real
stores, FIFO and encoded input/ACKs with supplied geometry, catalog, protection
and renderer transitions. Six compiled mutations fail their intended controls:
omitting expected focus, omitting expected opening, comparing only an ACK event
number, issuing Enter before its revision is presented, omitting timeout, and
trusting a stale caller revision at intake. Each disposable source is restored.
Evidence is under `.artifacts/bemenu-native-focus/`. The socket saturation control
counts all successfully queued bulk records, including those already written to
the kernel, before checking the two reserved terminal notifications. It is not
a latency or owner-loop fairness test. No physical input, supervised launcher,
catalog execution, native display or Bemenu backend has run in these controls.

The limit follow-up also constrains native receipts and unsent Enter by the
negotiated pending-action limit, shared with pointer cancellation obligations.
The sixteen-element array is only a storage ceiling. A smaller or zero grant
does not inherit that ceiling, and retiring a pointer cancellation or exact ACK
frees only its own slot. Native text also checks the negotiated frame-payload
bound before receipt/revision transfer, independently of its 512-byte control
credit. Four additional private-socket controls exercise zero/one-action grants,
shared pointer/native occupancy and an exact 304-byte payload boundary. The
pointer control supplies Session's target decision; it proves shared transport
accounting, not pointer authority. Evidence: `.artifacts/bemenu-native-input-limits/`.

## Native activation queue ownership over c274f7ad

The native request now reserves a response credit before leaving the transport
inbox. Its exact first outcome survives FIFO refusal and launcher dismissal;
transfer clears the Copy producer record only after the FIFO owns the encoded
frame. The shared aggregate record/byte budget includes this reservation through
its final written byte. A new opening cannot inherit the old reply's admitted
state. This is response ownership, not permission to replay a launch effect.

Session pairs the published descriptor catalog with its immutable source entries.
Keyboard activation requires the actually issued Accept and current presented
selection. Pointer activation uses the actual content-action ledger, including
its exact native binding; transport cancellation capacity is not input authority.
Both paths insert into the existing Session launch queue and retain the exact
catalog entry and activation origin. ACK order or rejection does not undo queue
admission. A refusal consumes the attempted Accept. Legacy catalog dispatch and
cancellation cannot consume the native dispatch. Revoking a grant removes its
queue authority while a previously borrowed worker payload can remain alive.

Retained Enter keeps its original issuance timestamp on the wire. Its ACK clock
starts when dispatched after the matching presentation, using the last serviced
monotonic time; time spent waiting for presentation is not charged again as peer
ACK delay. The live scheduler must still visit deadline service before dispatch.

Device-hidden affected runs in `.artifacts/bemenu-native-activation` pass 731
tests with zero failures and 14 ignored; strict affected Clippy and layout pass.
The six public Session controls use real private sockets, actual transport and
ledger decisions, immutable publication and the real launch queue. Presentation
and policy dispatch are supplied by the fixture. A socket saturation control
counts valid filler frames, drains them, and checks one queue admission/outcome.
Private response tests additionally simulate final-byte drain and defensively
reduce limits to test retained refusal; that limit change is not normal policy.

This slice does not execute an application, supervise a native launcher, service
physical input, or exercise GPU/KMS. Queue admission is not worker verification,
spawn, first-window admission, or physical readiness. Execution must revalidate
the exact connected grant and retained entry at the actual spawn boundary.

Five separately compiled mutations fail their intended controls: omitted reply
credit, lost reply on close, overwritten first outcome, replay after capacity
refusal, and bypassed selected-row validation. Disposable source hashes were
restored exactly; no mutation was applied to the live checkout.

## Native verification and execution-attempt custody

The existing catalog worker accepts the actual native queue payload by `Arc`
and returns that same owner with its rebuilt-catalog verification result. Its
legacy and native verification use one comparison/revalidation function. Changed
command/source data is refused. The result itself cannot restore queue authority
lost through grant revocation. Nonblocking shutdown stops submissions, retains an
outstanding result until `poll` takes it, and joins only a finished worker. An
owner dropped without join is still abandonment; live native supervision must
retain and service this owner rather than infer shutdown from disconnection.

The queue retains dispatch-consumed and execution-attempted state. Taking a native
dispatch cannot be rearmed to start another verification. The execution gate
requires that dispatch, exact current queue payload, current grant and the exact
verified command. It consumes the one attempt before spawn. Grant revocation
before that gate withdraws admission; after it, revocation cannot erase the
first-window attribution of a possibly started application. Explicit failed-spawn
settlement cancels the exact admission. This is not proof a process was spawned:
the live execution caller and typed child-origin join remain unfinished.

`.artifacts/bemenu-native-execution` retains eight native admission controls,
four catalog controls and twelve queue controls, all run device-hidden with no
application spawn. The native worker control uses actual worker filesystem
verification and checks exact payload identity, changed-command refusal,
revocation and result-before-join custody. The execution-attempt control exercises
the actual queue transition, not an OS execution or first-window event.

Strict Session Clippy and layout pass. Three separately compiled mutations fail
their intended controls: execution replay, wrong grant acceptance and ignored
changed command. All four tested source files match the restored disposable
archive. Full canonical validation for this successor is not yet claimed.

## Managed application origin handoff

The Session's real first-window and child-exit checks now delegate through
`ManagedSessionChild::matches_admission` to the queue's exact origin match.
Managed children can retain the native payload alongside their transaction.
Native admission requires that payload, including its retained entry identity;
a legacy catalog child with the same numeric transaction cannot settle it, and
an old native child cannot settle a later legacy admission. Non-native matching
keeps the existing catalog-versus-ordinary distinction.

The legacy launch path now calls the extracted `spawn_catalog_child`, preserving
its executable/argv, control/display environment, process group, standard streams
and working directory. It returns the existing managed child owner and can carry
a native payload. Native execution still needs to consume the exact attempt and
call this helper through its live role owner; this extraction alone does not
launch Bemenu or establish first-window acceptance.

Focused device-hidden controls: nine native admission tests, twelve queue tests
and one existing managed-exit policy test pass. Strict Session Clippy and layout
pass. A separately compiled numeric-only-origin mutation fails the new matching
control; all seven source files are restored byte-for-byte. The new control
exercises the shared queue matcher, not actual X first-window or process-exit
delivery. No application/native process was run by this slice.

The next process control now exercises the actual shared OS spawn function with
`/bin/true` inside the device-hidden test namespace. `spawn_native_catalog` reads
the grant from the borrowed connection, consumes exact execution authority,
returns the Child together with the same retained payload, and settles exact
spawn failure. A stale duplicate cannot cancel the successful attempt. Failed
verification or a disconnected grant settles only pre-execution admission.
Conversion to `ManagedSessionChild` preserves this payload for the production
first-window and exit checks; the live native role scheduler is not wired yet.

The isolated child exits zero without a window. The initial test incorrectly
expected successful catalog completion; its retained failure led to correcting
the test to preserve existing policy: no window means failed admission, even
with exit zero. Twelve native controls, twelve queue controls and one existing
managed-exit control pass. This is actual short-lived process execution without
a display connection, not a native graphical application or attended run. The
verification result is supplied in the spawn control; actual worker verification
is tested separately. No joined live-supervisor/worker/first-window claim follows.

## Joined native catalog service

`NativeCatalogService` now retains a dedicated existing catalog worker and one
pending verification. A bounded visit takes at most one native dispatch and one
worker result. It checks the current connection/queue, deadline and shutdown
state before passing the exact result to the shared spawn function. Shutdown
rejects pending authority but retains the worker/result until serviced and joined;
a stopped owner cannot return a newly started child. The role scheduler must
still own this service and adopt each returned child/origin immediately.

The joined private-socket control executes actual catalog filesystem verification
and an isolated `/bin/true` spawn. It separately tests shutdown, disconnected
grant, five-second verification deadline, queue revocation and monotonic-time
regression between submission and completion. All six schedules pass within one
parameterized test; this is not six independently counted test functions.
Renderer/protection/policy dispatch remain supplied by the socket fixture. The
control drains and joins the actual worker; no display endpoint is opened.
A compiled deadline-omission mutation fails the expired-result schedule.
The standalone service is not yet the live dual-component owner loop.

A second service control stops during catalog refresh: new requests are refused,
the outstanding result prevents early join, and draining it publishes neither a
catalog nor a child. Normal channel closure after intentional stop is idle;
worker panic is still reported by join. Final focused service evidence is two
test functions (the execution test contains six schedules), not a full Session
or graphical run. Initial strict-lint failures and their corrected run are
retained separately in the service artifact directory.

## Nonblocking component process retirement

The shared `ProcessSupervisor` now exposes request/poll termination. Signalling
and each reap visit retain the actual child across pending and returned-error
paths; replacement stays refused until reap. The existing blocking `terminate`
wraps these same transitions, preserving legacy callers. New component owners
can visit the nonblocking path without sleeping through another role's service.
This is process retirement, not disposition of content/GPU consumers.

Thirty supervisor controls pass device-hidden, including a real TERM-ignoring
process group while another child progresses. The original child remains owned
until the KILL deadline/reap, then replacement is permitted. Strict runtime
Clippy and layout pass. A compiled early-discard mutation fails the ownership
control. Evidence: `.artifacts/bemenu-component-supervision`. This does not yet
exercise dual Session protected-role scheduling, injected wait errors, or
panic/unwind retention. The actual child remains stored on returned wait errors;
that last statement is source behavior, not an injected-kernel-failure test.

## Two protected process owners over one registry

`ShellComponentProcesses` joins two actual supervisors to the existing single
connection/content registry. It reserves an exact attempt before launch-policy
preparation, retains the supervisor before spawn, revokes before nonblocking
stop, rotates process visits and caps each handshake visit. Preparation/spawn
failure closes only that attempt. Stop errors remain reported and actual process
owners prevent replacement and final backend settlement until reaped. Role
service receives the existing borrowed connection rather than another ledger.

A device-hidden control covers failed preparation, an unprotected spec refused
before spawn, a missing executable with retained failed supervisor, stale stop,
and independent neighboring epochs. A separately invoked ignored fixture starts
two real bubblewrap-protected sleep processes, stops/replaces one while retaining
the other, and then retires both. This is protected-process custody with both
connections still negotiating; the children do not implement shell IPC and this
is not a dual-peer hello or live compositor test. The first protected run failed
because the outer fixture omitted `/etc/ld.so.cache`; its log is retained. The
corrected private fixture generates the cache from its own allowlisted libraries
using the existing offline-harness recipe, without exposing host `/etc` or devices.

The protected fixture now launches real protocol peers rather than sleep-only
children. They independently negotiate revision 6 for the bar and revision 7
for the launcher using actual protected-process evidence/peer credentials, decode
their welcome/limits and upload pixels through the shared registry. The parent
retains actual resource consumers across launcher disconnect. Because the bar
and launcher reservations fill the aggregate budget, reconnect is correctly
refused while an old launcher consumer remains; releasing it permits a fresh
grant. No budget was enlarged to make the fixture pass. The original overly
optimistic reconnect expectation and its Budget refusal are retained separately.

The final control keeps the bar connected, replaces the launcher, verifies that
an old stop key cannot retire its successor, then drops the real resource
consumers and requires fully quiescent accounting. Explicit protected handshake
run passes; the ordinary target separately passes its preparation control with
two deliberate ignored entry points. This supersedes the earlier negotiating-
only fixture scope, but still uses test peers, no renderer, no Bemenu UI and no
live Session owner loop. It does not establish the client's receipt of every
resource response before the parent stops it.


## Typed public C native records

The public C header now exposes owned scalar identities for the six inbound
native records and checked encoders for all five outbound kinds. Input text is a
bounded borrowed slice valid only until its frame is consumed; clients must copy
it before retaining an edit. Decode refuses outbound kinds and leaves the prior
result untouched on malformed/truncated input. Encoders validate before changing
the destination or returned length. Candidate/chunk arrays have explicit 32-row
bounds and encode only the parentless transient native surface policy.

`tools/check_shell_c_wire.sh` runs the typed codec against the pinned Rust golden
frames. It checks every binding field, UTF-8 borrowing, all output bytes, every
short destination/truncated inbound payload, invalid selection/duplicate rows,
zero transactions, invalid ACK/activation revisions, signed margins and maximal
row/chunk counts. The focused codec also passes Clang AddressSanitizer and
UndefinedBehaviorSanitizer in the device-hidden runner. GCC sanitizer linking was
unavailable on this host; that failed build is not sanitizer evidence. Compiled
mutants replacing event revision with binding revision and replacing candidate
rows with selection each fail their intended assertions. Evidence is retained in
`.artifacts/bemenu-native-codec`.

These are codecs, not a focus/resource/candidate lifecycle. No decoded Focus may
promote Prepared content, and no encoded ACK/Activate establishes queue ownership,
peer receipt or launch authority. The joined client owner still must validate
exact current Presented bindings and retain response/resource obligations across
partial I/O, newer content, revocation and disconnect.


## C immutable resource vocabulary

The C client can now encode ResourceBegin/Chunk/End/Cancel/Retire and decode
ResourceStatus/Released through `sophia_shell_content_resource.h`. These records
are shared content vocabulary, not native-launcher-specific messages. Shapes,
reduced scales, resource dimensions, canonical whole-row chunk count, byte bounds
and status/reason combinations follow the Rust codec. Negotiated limits still
need separate validation by the client owner. Chunk encoding copies from caller
storage without a payload-sized stack buffer or heap allocation; all refusal
checks precede destination writes.

The independent golden comparison covers all seven kinds; additional controls
cover every short output/truncated reply, maximum-size chunks, overflow/zero
identities, wrong direction/transaction, scale/geometry/count errors and terminal
status bounds. Device-hidden C gate and focused Clang ASan/UBSan pass; compiled
resource-generation and offset-bound mutants fail. Evidence is retained at
`.artifacts/bemenu-resource-codec`. This still establishes codecs only. In
particular a rejected Retire can leave a resident resource live, while an aborted
incomplete transfer has different disposition; neither a generic status nor an
unmatched Released may grant slot reuse. The future shared client owner must
retain the exact request/generation and aggregate credits through those paths.

## C negotiated content limits

`sophia_shell_content_limits.h` decodes AdmissionRefused and ContentLimits without
allocating or changing connection state. The latter retains every named bound,
checks the prototype ceilings and cross-field coherence before assigning the
result, and accepts coherent tighter profiles. In particular, session retiring
capacity must cover the declared staging, resident and retiring overlap. Zero
optional facilities remain legal; zero grant identity, mandatory capacity or
timeout does not. These checks port the Rust limits contract; callers still must
match the negotiated welcome, role and current grant before reserving work.

Golden records, every payload truncation, trailing bytes, transaction/direction,
every cap exceeded, mandatory zero fields and cross-field contradictions pass in
the device-hidden C gate. The optimized GCC fixture stays under its 12,500-byte
stack warning limit. Focused Clang ASan/UBSan passes. Two compiled mutants fail:
removing aggregate overlap validation and reading target capacity from the
placement field. Disposable sources were restored; evidence is retained at
`.artifacts/bemenu-content-limits`. The initial runner invocation used an invalid
Python keyword and executed no checks; only the corrected recorded run counts.
This is wire validation, not client resource accounting or native acceptance.

## C presentation feedback and control vocabulary

The common content codecs now cover OutputFacts, AllocationResult,
CandidateOutcome, FramePermit and Action as owned decoded values, plus complete
CandidateEnd, FrameDemand, FrameDemandCancel and ActionAck frames. Output facts
are bounded to sixteen distinct output IDs. The structural checks preserve the
Rust distinctions between rejected, released and revoked allocations; prepared,
presented and rejected candidates; granted and refused permits; activation,
dismissal and cancellation. Only Presented has a nonzero presentation epoch.
Signed geometry/margins are decoded without implementation-defined unsigned
conversion. Caller output remains untouched on refusal.

The C gate compares all nine records to the Rust golden corpus and exercises
truncation/trailing data, identities, reserved fields, status-specific geometry,
all sixteen outputs, duplicate IDs with changed generations, reduced scales,
permit bounds, exact ACK fields and short destination preservation. Focused
optimized GCC and Clang ASan/UBSan runs pass under device-hidden isolation. Three
compiled controls reject omission of the Presented/epoch relation, acceptance of
same-ID/different-generation duplicate outputs, and omission of the granted TTL
ceiling. Evidence: `.artifacts/bemenu-content-feedback`; disposable source restored.

These codecs deliberately do not maintain current presented targets, consume
permits, release resources or dispatch input. The joined client must preserve
those independent lifetimes, enforce negotiated limits and queue exact ACK and
activation obligations before committing effects. A structural Action cancel
must not cause an ACK; a valid encoded ACK is not evidence of that owner rule.
Native client state, live Session dual-component wiring and physical acceptance
remain required before `lom-test` readiness.
