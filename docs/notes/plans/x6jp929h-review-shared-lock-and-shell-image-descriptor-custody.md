---
id: x6jp929h
date: 2026-10-17
kind: plan
tags: [plan, shell, lock, rendering]
---
# Review shared lock and shell image descriptor custody

## Current resumption

t338 remains open. Sophia `cbee1268d` adopts published Rust
`e4f59e36f6c7acbbe2ee47ee011c665dea1a6176` and C
`d51b0336649757eaf7bf6970c50c14786f0a4772`. Both snapshot checks pass,
including the Rust SDK workspace tests. Rust now has the shared sealed producer,
BAR adapter and lock adapter. Muse's shell fixes preserve cancellation metadata
after a refused forget and route statuses by transaction plus resource identity;
all 14 adapter tests independently pass. The lock adapter's full suite and
strict Clippy pass. Its real 9P/SEQPACKET controls cover successful descriptor
handoff, cancellation before late admission, ordered status/release, clean
absence, missing grant, discovery failures and channel contradictions that leave
9P usable. A failed clunk, including ENOENT, cannot imply clean absence.
These scripted peers are SDK evidence, not deployed Session qualification.

Lock runtime `011f27c05` joins custody `548c66e30` to a persistent validation
worker, authenticated Source reception and retirement leases. Review fix
`403606cdb` removes an ended epoch's parked completion bookkeeping and revokes
custody on an unexpected live completion error. Root independently reran all
33 lock image transport tests and the C lock-client export suite successfully.
The tests include held backing across successor epochs, release after the last
reader and service drop with a caller-held clone. Dropping a service does not
wait for consumer clones. Worker jobs remain a separate completion obligation.

Session `7bab56b0d` enables sealed lock images using the existing resource/live
place envelope, the assembly timeout and 64 KiB validation units. It grants no
unauthenticated channel. The seven lock-object/policy tests and native-session
strict Clippy pass. The shared-pixel Session handoff is already in `85755e5c3`;
the placed-image retirement control passes. Protected C SDK test `49da1ded4`
now proves real Factotum/afid authentication, Hello, Source, Accepted and
SourceReleased disposition 1. It verifies exact 4×4 granted pixels and keeps
them readable across authority revocation and provider replacement. The new
test and all five authentication-module tests independently pass, including
parallel execution after isolating each test's compiled C peer. This is a
protected provider/export test, not native display or performance evidence.

The lock audit corrections are signed in `7f9c96917` and `bc667ecef`.
Wrong sealed backing length retains reason 4 rather than invalid_source (10).
Worker exit wakes its owner; a dead worker revokes live custody, is no longer
polled, and refuses later offers with reason 9. All 34 lock image transport
tests independently pass. The worker-failure controls inject the owner's
failure transition; they do not prove an actual worker panic. Pending admission wait and
post-admission validation have separate bounded deadlines; Source arrival never
renews the latter. No contract relaxation is authorized by these fixes.

Shell control `9ef582fee` submits a real sealed Source to the worker, then
cancels over 9P while the supplied owner schedule withholds completion
collection. No terminal event appears before collection; the actual completion
settles once with cancellation and disposition 4, and a late Source drains.
This proves completion custody, not that the CPU scan was still running.
The separate worker-shutdown control returns to its FD baseline after submitted,
reserved and uncollected jobs, while delivered pixels remain readable until
their last owner drops them. Both shell transport tests, the shutdown control
and targeted strict Clippy pass. Transport-level teardown and the remaining
lock cancellation controls are separate qualification work.

Next are the remaining SDK/export and cancellation controls, and matched sealed-versus-byte
native panel/bulk evidence. Comparator `c1b5faa71` merges the reviewed SDK/runtime
while retaining both byte and IPC controls: its native-session check and both
device-hidden peer tests pass. `93fb5991e` also carries the worker corrections.
The generic sealed peer is being prepared. External integration `a1a3c38`
binds request, peer configuration and captured journal to the byte or sealed
source and wire; all 30 targeted tests and strict Clippy pass. Reports preserve
logical accepted payload bytes and CPU/frame alongside existing cost fields.
No new performance result follows from this merge or tooling.
No native capture has begun and no
installed desktop has changed. All four unchanged t331 +10% gates, per-request
allocation attribution and journal-read pairing remain required; the earlier
red panel result is permission to proceed, not a performance pass. DMA remains
outside this slice.

## Earlier implementation checkpoints

The following paragraphs preserve earlier results and then-outstanding work;
the current resumption above supersedes their implementation status.

t338 remains in implementation. The C pin `02ea2ad` includes offer cancellation
(`e777235`) on top of the lock adapter; an independent full `make check` passed. Its ABI
additions require rebuilding consumers. Rust role-adapter parity remains open.
The role-neutral Rust producer is published at `a366a0e`, with 19 independent
tests using real SEQPACKET and SCM_RIGHTS, kernel POLLOUT after backpressure,
both roles, cancellation and authoritative release. Its C counterpart `d51b033`
also requires an exact supported revision and a SEQPACKET socket before custody;
the full C check passes. Those newer SDK commits are not yet the Sophia pins.
Muse's BAR adapter is signed locally at `9cc9678`; final integration review is
pending. The Rust lock adapter is under cross-review, with a real descriptor
success exchange, clean absence and cancellation before late admission passing.
These scripted peers do not establish authenticated production interoperability.

Lock custody checkpoint `548c66e30` reserves terminal credits at receipt and
retains sealed backing charges through retirement and successor epochs using
the weak watches from `b55322dc6`. Its transport still does not admit Sources;
Claude is wiring the persistent worker and real-wire loss controls next.
Root integration `85755e5c3` joins Session's shared-pixel handoff and fixes the
remaining C role-peer link lists. Both affected C export tests and Session's
placed-image retirement control pass. Native measurements remain outstanding.

Claude's review found no blockers
in the retired-epoch completion fix (`d33e546d8`) or the independent journal
reservations (`310cb886f`).

The reason-name pins are C `f5e499c` and Rust `e3bdbe6`: lock r2
`invalid_source` (10) and `validation_timeout` (11), with wrong backing length
retaining reason 4. Existing open-vocabulary codecs preserve these values.
Unsent expiry uses SourceReleased disposition 3; cancellation uses 4 only after
source custody permits release. Lock admission must reserve terminal journal
capacity before committing Submitted. These rules guide Claude's next lock
admission/worker slice; that slice has not activated Source acceptance yet.

The resource owner now has a bounded four-job handoff table retaining admission
identity and completed validation results until the exact active or retired
store consumes them. Real worker tests cover disconnect, clock-regression retry,
exactly-once reconciliation and owner capacity remaining occupied after worker
completion. The table now owns Source submission and withdrawal as well: an
unsent withdrawal releases its worker credit, a submitted withdrawal waits for
the real completion, and the first store cause survives later cancellation.
Original admission deadlines and epoch-wide channel loss are explicit owner
operations. A worker timeout preserves Timeout rather than becoming Malformed.
One epoch's withdrawal error no longer prevents expiry of another epoch's jobs.
Delivered results no longer contribute a validation deadline, while store deadlines and
explicit cancellation still apply at reconciliation. A failed unsent-credit
release preserves custody for retry with the paired worker. The allocation
suite passes 23 tests, including two-epoch isolation, wrong-worker recovery and
ResourceCancel propagation to both unsent and submitted worker custody. The
owner bridge observes store stops before accepting new Sources; repeated
observations neither release submitted work early nor create another settlement.
Claude owns the lock runtime implementation lane (`lock_files` only), while
Codex owns shell/Session integration and the Rust lock adapter. Muse owns the
Rust BAR adapter and cross-reviews the lock adapter. The existing runtime worker fixes are reviewed;
targeted strict Clippy and the layout
check pass. Explicit shell owner hooks now connect ImageBegin, Source and the
terminal records. The real 9P/SEQPACKET fixture waits for ImageAdmitted, sends a
sealed memfd, closes the producer FD, validates on the worker, reads the retained
resource lease, receives ResourceStatus and SourceReleased, then retires the
resource through ResourceReleased. A missing live allocation refuses admission
without consuming terminal holds or worker credit; the same offer succeeds once
the supplied test owner grants it. A late valid Source drains silently, while
an extra-FD packet for that same stale offer faults the channel. This is supplied
afid authority, not a Factotum or deployed Session qualification.

The lock acceptance lane has an immutable `SharedPixels` backing handle for
validated mappings and existing byte Arcs. Session, Engine and the CPU renderer
retain that handle through their existing lock-image path. A pointer-identity
and clone-lifetime test proves that converting a validated mapping does not
copy its pixels. This removes the backing-type blocker, not the remaining lock
admission, release accounting or native presentation qualification.

The reviewed shell checkpoint `992e20653` keeps four live offers, exact metadata and two independent
terminal reservations each; the ChannelClosed hold makes nine. Journal encoding,
capacity and sequence checks precede resource/worker reservation. ImageBegin
shares the inbound queue with ordinary content. End/Cancel for a queued offer
cannot overtake its admission, while Retire, allocation and acknowledgement
progress remains available to release admission budgets. A failed sealed store
reservation preserves the resource generation for retry; legacy byte Begin
refusals retain their generation-consumption rule. Submitted offer IDs have a
separate monotonic mark from the admitted mark used to classify late packets.
The owner must reject permanent admission failures and queued offers after
channel closure, and bound transient retries by its offer deadline.
The resource owner still has to drive withdrawal,
completion and release hooks on every path; the live fixture covers success and
malformed stale packets, not the full cancellation matrix. Default callers remain
receive/refuse. Published sealed budgets are enforced conservatively against all
backing retained by that role and all pending work on the paired worker; separate
per-source backing sub-pools are not claimed. Cancellation identity and permanent
local send suppression are now explicit in the shared contract and both SDK
spec copies (`02ea2ad` C, `7d5d522` Rust). This is not a Rust producer parity claim.
Rust producer/adapters, lock receive/accept integration and the matched native
gates remain outstanding. Runtime wiring alone is not performance acceptance.

`ShellSealedImageOwner` now composes the shell hooks into a bounded per-component
service turn. It owns the worker, four jobs and retained release settlements;
exposes worker readiness and the earliest deadline; withdraws jobs when the
channel or epoch disappears; and bounds admission-capacity retries. A second
real-wire success case uses this owner rather than manual job handoffs. A wire
ResourceCancel after ImageAdmitted but before Source now produces one Cancelled
status and one pre-import SourceReleased, returning all job/backing credit.
Disconnect after ImageAdmitted with no Source also drains the reservation and
retired epoch without a fabricated release event. A refusal blocked on journal
capacity waits for peer progress rather than repeatedly waking on its expired
admission deadline.
Session now installs this owner beside each authenticated component transport,
using the actual reserved role limits. Byte-resource service and image jobs use
one monotonic content clock across connection replacement. Worker readiness and
deadlines survive revocation; paused/stopping visits still service the owners,
and final backend disposal refuses while image handoffs remain pending. The
Session wire test covers r9 discovery/Hello, r6 absence, replacement r9, an
admitted unsent offer's staging charge and disconnect cleanup. The replacement
epoch also sends a real sealed Source through worker acceptance, SourceReleased,
Retire and ResourceReleased. Its authority is
supplied, not a protected-launch proof; submitted-worker shutdown and native
presentation still require qualification.

Shell discovery and formats now follow output-facts generation. Encodings and
qids are prepared before the store accepts new facts, then installed before
the existing notification. Open discovery/formats fids keep their prior bytes;
the Session test checks old and new reads across generation 1 to 2 before
admitting an offer against generation 2. Native Session check, targeted strict
Clippy, the ten existing connection tests, real-wire runtime image cases, the
C descriptor peer and layout checks pass. No installed desktop was changed.

Claude's lock slices `ec550020c`, `703f246b1` and `7a5e5aee9` add explicit r2
selection, readiness-gated clean refusal, coherent role bounds and disclosed
immutable discovery snapshots. Slice `02387f6e3` adds authenticated lock
socket/Hello service and Ready/Closed journal transitions; its supplied-authority
wire tests include timeout, wrong Hello, peer close, provider exit and legacy
absence. Sealed lock admission remains in his lane. The C descriptor peer's link dependencies are corrected in
`16dead7f8`; its independent production-export test passes.

## Scope and exit

**Accepted October 17.** The independent lifecycle peers (Rust `8f7dc5d`, C
`b2ca17d`) landed and passed cross-review, satisfying niltempus's condition.
The [shared normative contract](../../sophia-image-channel.md) now owns the
custody rules and exact layouts. The proposed text below is retained as the
review record; implementations follow the normative document if wording differs.
Both role topology amendments and the shell KDL ResourceStatus bound land in
this acceptance change. t338 begins with schema/public codecs and authenticated
receive-and-refuse, not seal acceptance or DMA-BUF. No production capability
is enabled by this decision.

Acceptance validation: `cargo-slot sophia cargo xtask check` passed, including
workspace and SDK tests, strict Clippy, contract snapshot equality, layout and
reader checks, all six archived scanout runs, buffer-age pixel equivalence and
GLX/EGL first-frame and pixmap-export pixels on this host. The log is
`~/.local/state/sophia/development-evidence/t337-acceptance-20261017/check.log`.
These checks do not prove a production image channel or meet t331's cost gates.
The installed desktop was not changed.

The staged whitespace check then found a trailing blank line in the new
contract. Rust `8ad08de` and C `ca61d21` remove it and refresh the spec hashes;
the adopted pins change no code. Both snapshot gates were rerun after that
correction, with the staged diff checked as well.

On October 17 niltempus directed the descriptor design review after the matched
B2/B3 pilot. That initial direction admitted planning only; it did not yet accept
a new wire contract or implementation, change t331's gates or change the installed
desktop. Codex owns this shared design; consumer ownership is assigned when
implementation opens. Claude confirmed that no newer lock draft or active
consumer work conflicts with it.

The review starts from Sophia `924ed07f2`, the proposed
[descriptor ADR](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md),
and the October 4 revision-2 lock draft at
`~/.local/state/sophia/development-evidence/t302-integration-01/DESIGN-lock-dmabuf-phase1-01.txt`.
That draft is historical input, not the contract. The recommendations below
supersede its suggestions through the accepted shared contract linked above.

The [image-path investigation](../investigations/cerml352-what-a-fast-shell-and-lock-image-path-looks-like-beside-9p.md)
collects precedents and further options: credits, reusable GPU registrations,
sealed-memory GPU import and later fences. These are proposals for review, not
additions accepted by this plan. Its performance projections are models, not
measured bounds or grounds to change t331's gates. The custody review below
still requires an immutable renderer-owned capture of mutable DMA-BUF content;
registration or fences alone do not replace that requirement.

The t337 exit is an accepted common ownership model, exact proposed record and
object layouts with golden cases for both roles, and a compatibility/SDK review.
Normative KDL, generated rows and production implementation follow acceptance;
this note deliberately does not allocate production opcodes. The first review
must resolve the lifecycle before making those layouts normative.

## C codec review checkpoint

The independent C codec landed on its development branch at signed `0a12720`
(following `276e89b`). Codex ran `make check` successfully on the initial
candidate and reran the 54-case image codec target after review fixes. Muse
independently reviewed packet and discovery handling. The fixes move pointer
arithmetic after argument validation, state the actual validation-order promise,
make the input-buffer lifetime of format rows explicit, and leave DMA-BUF stride
validity to import validation. Legacy shell codecs retain reason bounds of 12;
the explicit revision-aware entry points admit reason 13 only for r9 Rejected.

This is a codec checkpoint, not channel I/O or custody implementation. Sophia
has not adopted this pin, and Rust parity is not yet claimed. Positive timeout,
validation-chunk and per-turn service values are being stated explicitly in the
shared contract; synchronize that clarification with both SDK specs in the
codec adoption series. The independent Go follow-up is recorded below.

The Go oracle revision check is integrated locally as signed `42548ad9b` and
`fd805a2eb`. Independent `go test ./...` and `go vet ./...` pass. A bounded
`net.Pipe` peer covers Negotiated and ResourceStatus in one read and split
reads: r9 accepts Rejected reason 13, r8 and no negotiation refuse it, and
foreign-epoch or stale-sequence negotiation cannot change the selected revision.
The oracle still offers only its existing revisions.

The Sophia lock codec at `4fbe66d71`, with test follow-up `f4eb0d2f1`, passed
Claude's independent review. Existing envelope entry points remain r1 and reject
image kinds; explicit revision-aware entry points accept r2 without advertising
it. Discovery carries publisher values rather than fixture constants. Intrinsic
capacity checks match C, and metadata encoding uses its fixed array without a
scratch allocation. Codex ran the 34 runtime custody, export and service tests,
including refusal of r2 ImageBegin before r1 epoch/submission checks, unchanged
journal state and successful subsequent legacy submission. Evidence is under
`~/.local/state/sophia/development-evidence/t338-lock-codec-review-20261017/`.

The coordinated codec checkpoint, now adopted at Rust `5c41aeb`, includes
the SDK lock mirror `1b07ebb`, shell image bodies `4c14819`, explicit shell r9
envelopes and ResourceStatus entry points, and schema tests against the new KDL
rows. Adopted C `e146897` carries matching specs and the stated parity limits. The
shared contract now makes timeout, chunk and per-turn values positive. Legacy
entry points still refuse image kinds and reason 13; legacy Submitted and
ObjectPublished encoders also retain their old embedded-kind vocabulary.

Both Rust protocol suites pass, along with strict Clippy, formatting, the new
shell field-offset tests, and C's 54-case image codec controls. Sophia's image,
lock-schema and explicit-class tests pass. Both SDK heads are published and
their signed archive snapshots pass identity and spec-equality checks. The
affected Sophia C/Rust/Go shell and lock export run passes 62 tests, with one
existing ignored test left unselected. Independent review has no remaining
blockers, and the final differential result is recorded below.
No production revision, endpoint or installed desktop changes here.

Remaining first-slice work is discovery/admission integration. C's cross-object
coherence helper has no Rust counterpart yet: the discovery reader must check
epochs, all three generations, discovery/limits equalities, source-kind grants
and per-output bounds before Hello. Rust packet shape also leaves the actual
received FD count to its caller. These gaps remain explicit and block a claim
of complete discovery/admission parity. Shape codecs alone do not close them.

Independent review found a legacy lock receipt leak: adding ImageBegin to the
kind enum made plain LockSubmitted accept it through the Candidate class check.
Both Sophia and the SDK now keep that receipt's r1 range at 256–263, with
encode and decode regressions for the image namespace. The shell Submitted KDL
ceiling is corrected from 272 to 278 to match both existing SDK encoders;
this does not add a new production kind.

Before runtime admission opens, explicit r9/r2 Submitted codecs and their KDL
rules must accept candidate_kind=ImageBegin (0x7000). The ordinary submit/ACK
path must journal the actual accepted kind, never suppress or relabel that
receipt. Plain entry points retain their legacy ranges. This is a prerequisite
for new-revision transaction wiring, not permission to widen the legacy APIs.

The receipt fix is signed in Rust `5c41aeb`; C `e146897` carries the matching
shell KDL correction. Claude's independent differential run on these exact
signed archives compared 117,756 cases per role, including
field mutations, truncation, format-row boundaries and seeded random inputs.
Accept/refuse and accepted-byte round trips agree except for the expected
opposite-role discovery case: C is shared, Rust's codecs are role-specific.
A deliberately weakened Rust codec produced 365 mismatches, demonstrating
that the comparison detects disagreement. The final lock receipt probe refuses
ImageBegin in both directions, and the differential verdicts match the earlier
run byte-for-byte.

The differential supplies matching packet expectations and FD counts; it does
not exercise the missing Rust caller-bound or cross-object checks. Harness
sources and hashes are preserved, without binaries, under
`~/.local/state/sophia/development-evidence/t338-codec-differential-20261017/`.
The generator SHA-256 is
`4f9e01122444659df7e6e55bbc40b08c8342e0466aca7a1edbf6253b5b610d31`.

## Owner direction — October 17, 2026

niltempus approved this design and ADR qujwzrdz in principle, conditional on the
independent lifecycle/custody fixture peers landing and passing review. Kernel
and decoder fixtures alone do not meet that condition. The independent lifecycle
peers and cross-review now satisfy it. Codex records acceptance and promotes
t338 with the coordinated contract change described above.

The acceptance change must widen the topology-change wording in both the shell
contract (output facts and candidate validation) and the lock contract (object
publication, stale allocations and forwarded candidates), together with the
shell KDL ResourceStatus reason bound. These changes must land together.

For t338, CPU content uses a fresh sealed memfd per image. The launch-time shared
window is not pursued unless t338/t339 measurements show per-image descriptor
cost on high-rate CPU content warrants reopening it. This decision does not
authorize mutable-source sampling or advance the DMA-BUF implementation.

Muse's [application-surface exploration](../concepts/kgrl6al7-generalising-the-shell-and-lock-image-resource-toward-an-application-surface.md)
considers registered pools, synchronization points, presentation feedback and
scanout as a gated final phase. It is exploratory, allocates no opcodes and
changes no contract. It does not add t338 acceptance requirements or authorize
direct sampling of mutable producer buffers. Codex owns integration; Claude
reviews read-only.

## Recommended answers to the nine lock questions

| Question | Recommendation |
| --- | --- |
| Capability or revision? | Keep the 9P dialect and existing byte records. Introduce revision-gated role capabilities for sealed memory and DMA-BUF, with new record kinds; never reinterpret a reserved byte in an old record. Propose shell role r9 and lock role r2, subject to the exact codec review below. |
| Channel admission? | A separate nonblocking Unix SEQPACKET endpoint, bound to the authenticated live 9P connection, immutable principal, launched assignee and a fresh channel generation. A path or an epoch value alone grants nothing. |
| Release and supersession? | SourceReleased is a separate event. Candidate Superseded never releases a source by itself. A superseded offer that has not reached the GPU may be cancelled; an in-flight capture must finish or reach a proven safe teardown before release. |
| Capture failure? | Use a bounded standalone renderer capture job. It publishes an immutable image before resource acceptance. A failed job rejects that resource and leaves the previous image or cover intact. Do not weaken X's existing FailFrame policy. |
| Synchronization? | First DMA-BUF phase accepts only producer-completed buffers, no sync_file. Provider-side completion may wait on its worker; no wait may block Session, covering, input or unlock. Import/capture remains asynchronous to those owners, and its completion must be proved. |
| Devices and formats? | Same composing device only, with an exact per-output generation and advertised format/modifier pairs. No presumed LINEAR support, cross-device bridge, resampling or implicit conversion. Unsupported cases use bytes. |
| Renderer identity? | An owner-tagged, generational allocation in the renderer-image registry, separate from X transaction identity. Do not assume the high bit is free: current forward and reverse helpers accept every u64. |
| Store share? | Enforce the existing global image ceiling plus per-role/per-epoch reservations. Reserve source, destination and overlap before admission; no eviction of live X or shell images to admit lock work. If a reservation cannot be made, retain the cover or previous image and refuse the offer. |
| Acceptance point? | Admitted reserves bounded work. Accepted means immutable validated content is available: a checked sealed mapping, or a completed renderer-owned capture. Presentation remains a later candidate outcome. SourceReleased and ResourceReleased have separate meanings. |

The standalone capture decision replaces the draft's flip-dependent acceptance
path. Requiring Accepted before a candidate while waiting for that candidate's
flip to finish capture would create a cycle. DMA-BUF work therefore remains
behind t289's renderer storage/ownership decision; the sealed-memory path need
not wait for a new GPU capture implementation.

## Proposed contract text for review

### Discovery, negotiation and connection custody

Add a read-only `image-channel` object with its own format version. It describes
supported source kinds, the endpoint basename, current connection epoch, channel
generation, message/FD limits and per-output formats generation. The endpoint is
resolved only within the component endpoint directory containing its role's 9P
socket (the supervised lock-provider socket directory for lock). Keep the existing `api`
line byte-for-byte compatible; its `fd_transfer=none` continues to describe the
9P stream. The new object explicitly describes the separate channel.

New records require the corresponding negotiated capability and existing content
permission. DMA-BUF also requires the independently granted render device;
sealed memory does not grant or require GPU execution. Shell r9 must extend the
bar/launcher/dock profile matrix explicitly, preserving their existing minimum
requirements; a higher revision cannot turn one role into another. Lock r2
retains the rendering-only role. Old clients continue using old revisions and
byte records. Unknown or required-but-unavailable capabilities remain a refusal.
An application that permits byte fallback may select it after discovery; the
SDK must never silently remove a required capability. ENOENT means the optional
object is absent; access, staleness and malformed-object failures do not.

The channel's first packet is Hello, with no descriptors, carrying the protocol
version, role, 9P connection epoch and channel generation. The receiver checks
supervised peer credentials and liveness against the same launched assignee,
then joins them to the already authenticated 9P principal and current grants.
Recheck custody on admission of each offer. A legacy unauthenticated comparison
connection cannot acquire this authority. One channel is active per connection;
a second live channel is refused, never a replacement of the first. After Hello
is checked, a ChannelReady event on the 9P journal names the channel generation.
The client consumes that event before ImageBegin; otherwise the two sockets
could race during setup.
Unauthorized connection attempts are rejected without disabling another peer's
valid channel or spending its channel generation.

A 9P disconnect, epoch replacement, revocation or assignee death stops new offers
and closes the channel. A channel-only failure disables descriptor admission for
that epoch; the still-authorized 9P byte path may continue. Channel loss withdraws
interest in every live offer. Unsent or synchronously held sources release with
disposition 4; validation already in flight retains custody until its worker
returns, and submitted GPU work releases with disposition 5 only after proven
safe cleanup. Socket closure alone never proves that cleanup.
A new 9P epoch gets a new channel generation
and cannot resume an old offer.
This deliberately makes recovery conservative: a transient failure of the
authorized channel disables descriptors for the rest of that 9P epoch. The SDK
uses bytes, where the caller permits fallback, and leaves a full 9P reconnect to
the application. ChannelClosed on the still-live journal names the generation;
ImageBegin also refuses with `image_channel_unavailable` while disabled. Reserve
one ChannelClosed event slot when admitting Hello. Offers admitted but unsent
are cancelled with a pre-use release; already submitted GPU work drains safely.

### Offers and the two-stream ordering barrier

A new `ImageBegin` control record uses the ordinary 9P transaction/submit/ACK
path. It names source kind, resource ID and generation, output/allocation and
topology generation, dimensions, format, declared bytes and a fresh offer ID.
Its acceptance as a transaction is not resource acceptance. The owner reserves
budget and journal space, then reports ImageAdmitted with the offer ID and exact
metadata. Only then may the producer send the descriptor packet. Wire order on
one socket says nothing about delivery order on the other socket.

Offer IDs must increase strictly within a channel generation. Reject ImageBegin
with an ID no greater than the highest admitted ID; advance that high-water mark
only on admission. Zero is reserved and wraparound requires a new connection.
Keep that mark and the bounded live-offer set. A packet above the mark is
unadmitted and faults the channel. An ID at or below the mark that is no longer
live is stale: drain and close its FDs without an event or channel fault. This
also discards skipped IDs; the mark cannot prove that every lower ID was admitted.
A second Source for a still-live offer faults the channel. This distinction
allows expiry to race with delivery without retaining an epoch-long ID history.
Both SDKs must allocate descriptor offer IDs under this rule. Existing lock
resource transaction IDs are caller-supplied, so increasing submission IDs alone
do not establish compatibility; the source check below records the distinction.

The descriptor packet repeats the admitted identity and metadata. There is at
most one successful packet per offer. A duplicate closes its received descriptors
and cannot replace the first source, advance custody or reset a deadline. Unknown,
expired or cross-epoch offers are refused. Late packets cannot attach to a reused
resource ID. A second Source for a live offer is a protocol fault; a retired ID
is discarded by the high-water rule above. Retrying after an uncertain send requires an authoritative outcome
or a new offer; it is never blind replay.

For the first version, the channel carries only Hello and image-source packets;
all acknowledgements, admitted-offer refusals, cancellation and releases use the
9P journal. A packet above the admission high-water mark is closed and counted
without a per-packet journal event; the first such protocol fault closes the descriptor
channel. A duplicate or late packet cannot generate another release event.
Each packet has a fixed little-endian header with total length, version, kind,
role, connection epoch, channel generation, offer ID and descriptor count. A
sealed source carries one FD, zero mapping offset, dimensions, tight row stride
and byte length. A DMA-BUF source carries one to four plane FDs, format/modifier
and checked offsets/strides; fence count and all reserved fields are zero.
The proposed offset tables and provisional kind numbers appear below. Golden
records must follow those tables, not Rust struct layout.

Receive with close-on-exec and bounded ancillary storage. Reject truncated data
or ancillary data, unexpected ancillary types, extra/missing descriptors and
noncanonical lengths. These packet-shape errors, foreign packet identity and
metadata that differs from the live offer's admitted metadata fault the joined
channel with ChannelClosed reason 2. Zero offer IDs and disagreement between
header and metadata offer IDs are shape errors. Check packet shape before the
stale-ID drain; an expired ID cannot hide malformed framing or plane rows.
Close every received FD on every refusal path. Do not
recover a malformed packet by guessing which FD belongs to which offer. Reserve
terminal journal capacity before ImageAdmitted: two event slots per offer, for
final ResourceStatus and SourceReleased, in addition to ordinary admission and
transaction events and the channel's reserved ChannelClosed slot. If these
cannot be reserved, refuse new admission. Do not stop receiving merely because an offer expired: consume
and close late packets within the receive budget, or close the faulty channel
and let socket teardown release its queued rights. Expiry must not leave FDs
pinned in an unread backlog indefinitely. Bound both socket backlog and
per-dispatch packet/validation work so another role can make progress.

### Sealed memory first

The source is one sealable file of exactly the admitted tight BGRA8 byte length.
Require F_SEAL_WRITE, F_SEAL_GROW, F_SEAL_SHRINK and F_SEAL_SEAL before mapping.
F_SEAL_FUTURE_WRITE alone is insufficient. Validate seals and size before creating
a read-only mapping, use checked width/height/stride arithmetic and enforce the
existing premultiplied-pixel rules. Once a packet has valid shape and matches its
admission, missing seals, unsupported seal queries, wrong backing size or invalid
pixels refuse that offer with a rejected ResourceStatus and SourceReleased
disposition 3; they do not fault the channel. Missing or extra descriptors are
packet-shape errors, not this source-content refusal. Validate in bounded chunks,
without a whole
image scan on the input/lock owner thread. The validation worker processes
64 KiB units under a wall-clock deadline; these units are not paced one per
Session pass. `max_packets_per_turn` bounds receive work, not validation progress.
The worker queue and retained mappings remain bounded by admitted offers.
A read-only mapping alone is not an
immutability proof. No extra copy is promised until the storage representation
actually supports a retained sealed mapping.

After validation, transfer the mapping into the resource's immutable backing and
report Accepted. Retire or revocation prevents new references; outstanding
renderer leases keep the mapping and its charge alive until released. Closing a
producer FD does not remove Sophia's mapping. SourceReleased means Sophia needs
no further producer action and holds its own sealed backing reference; it does
not mean the file has become writable. A sealed file is never a reusable mutable
render target. ResourceReleased marks the eventual end of resource custody.

The SDK creates a fresh sealed source per immutable resource, or uses a separate
mutable scratch buffer and copies into it. Both costs belong in the benchmark;
sealing must not be presented as a free reusable-buffer optimization. Measure
page faults, fresh-page zeroing and both processes' mapping/population work;
do not infer those costs or fault counts from image size alone.

### DMA-BUF completion and release

The first phase requires provider-completed rendering, same-device import and
an exact advertised format/modifier pair. A source remains producer-owned and
must not be modified while offered. Copying into Sophia-owned storage isolates
future composition from later producer writes; it does not authenticate pixels
or prevent a malicious producer racing the capture. Such a race must not change
geometry, ownership or memory bounds, and a resulting image cannot bypass pixel
format/premultiplication requirements.

The capture worker holds source descriptors and destination reservation until a
real GPU completion or proven safe rollback. It then publishes the immutable
renderer image and sends Accepted plus SourceReleased in journal order. The
standalone job must not require a candidate or a display flip to complete.
Timeout can reject presentation interest, but is not proof that GPU reads ended.
A timed-out GPU job retains a bounded quarantine charge until safe cleanup; once
that bound is reached, stop admitting descriptor work and keep fallback/input
alive. Do not busy-wait for completion or free backing still used by the GPU.

After a pre-import refusal, no GPU read exists and release can be immediate.
After submission, cancellation, supersession, topology change and revocation
still wait for safe source release, even though they forbid publication. If the
journal connection has died, no release event can be promised. The SDK reports
unknown source custody, suppresses replay and never reuses that mutable buffer;
it may drop its own reference, but a socket close is not a completion signal.

SourceReleased is recorded once for every admitted offer on a live journal,
including one cancelled or expired before a source arrived. A pre-use release
permanently fences that offer: a later packet is closed without reading pixels,
importing or generating another release. Maintain this fence with monotonic,
never-reused offer IDs and bounded live-offer state, rather than an unbounded
tombstone list. ResourceReleased
is separate and occurs only after all immutable resource consumers are gone.
A resource rejection alone never authorizes mutable source reuse.

### Budget, fairness and failure invariants

Keep shell and lock resource budgets separate. A shell component cannot borrow
lock capacity or use a descriptor to bypass the shell's resource-size limits.
Charge immutable sealed bytes across staging, resident and retiring ownership,
transferring rather than accidentally duplicating a charge when ownership moves.
Charge DMA-BUF source references, pending capture and owned destination overlap
until each is released. The global renderer table remains bounded; a role quota
is a subdivision, not additional capacity. Logical byte accounting is not a hard
quota on a GPU-granted client's VRAM or on opaque driver allocations.

Use at most the existing role transfer count for outstanding offers and no more
than four FDs per offer in the first phase. The role's existing transfer deadline
bounds descriptor arrival and validation; packet retries never renew it. Numeric
renderer reservations, capture deadlines and per-pass work budgets must be
published in the proposed limits object and justified against supported topology
before implementation. Zero available budget means refusal and byte fallback,
not eviction of another role's live image. Covering and unlock work never wait
for an image reservation, renderer job or provider acknowledgement.

## t337 delivery and review order

Acceptance evidence: Rust `8f7dc5d` and C `b2ca17d` passed independent lifecycle
review; the spec/evidence adoption is Rust `8ad08de` and C `ca61d21`. Their
signed archives are pinned by this change. The 54 exact vectors are retained in
`protocol/golden/sophia-image-channel-v1.tsv`; the shared contract records their
source hashes and the namespace audit. Public codec and runtime support remains
the first t338 implementation step, not a result attributed to these fixtures.

1. Review the nine recommendations and lifecycle above against both existing
   contracts. Resolve the renderer's safe-completion and bounded-worker seam with
   t289 before admitting DMA-BUF implementation. Keep the sealed path separate.
2. Specify exact layouts for discovery, limits, ImageBegin, ImageAdmitted,
   SourceReleased and descriptor packets, including refusal codes, event-reserve
   accounting and optional-discovery compatibility. Produce positive and negative
   golden records outside the normative KDL until the decision is accepted.
3. Review independent C/Rust APIs for owned/borrowed FD lifetime, send EAGAIN,
   ambiguous send, cancellation, terminal status, reconnect and idle wake. Keep
   low-level C clocks caller-owned. Record public C layout changes and rebuild
   requirements. Native-client tests belong with the clients, not Sophia.
4. Record acceptance in ADR qujwzrdz and both normative contracts together with
   the ResourceStatus reason bound. Preserve exact reviewed layout vectors;
   t338's first slice transcribes the normative tables into complete KDL rows
   and public codecs before runtime receive-and-refuse. Promote that work
   separately; a design note alone does not satisfy the reviewed-fixture exit.

### t338 first slice and ownership

Codex owns the shared contract, shell/lock schema and protocol codecs, SDK spec
copies, vendor pins, Session endpoint wiring, integration and pushes. Under the
owner's parallel implementation direction, Claude owns the independent C SDK
producer channel and role adapters, reviews Rust, and now owns the lock runtime
files. Muse owns the Rust role-neutral producer; role adapters follow separately.
Codex owns the shell export, journal reservations, authenticated socket admission
and Session wiring. Shared checkout edits remain separated by file ownership.
The [current resumption](#current-resumption) records the live state and remaining
gates. The following checkpoints preserve what each earlier slice proved; their
then-outstanding work is superseded by that resumption.

#### Earlier implementation checkpoints

The receipt prerequisite is published in Rust `93fe07b` and C `a208c95`:
explicit shell r9 and lock r2 entry points carry ImageBegin in Submitted while
plain APIs retain their legacy vocabulary. Both Rust protocol suites and strict
Clippy pass, as does the full C check. Rust coherence now checks caller context
and revalidates public typed limits/formats. C producer `db8b24a` plus `69a5d80`
is published; BAR adapter `963f399` passes integration checks. Rust producer
and the remaining role adapters are incomplete.
The runtime shell test now uses real 9P and SEQPACKET with supplied afid authority
to prove Hello/ChannelReady, clean ImageBegin refusal and same-connection byte
fallback. It does not prove launch-domain or Factotum authentication. Bad Hello,
legacy successor epochs and held terminal journal capacity have regression
coverage. Session does not enable image configuration yet. The kernel sealed
mapping primitive is published as `28ce4a776`. The next store/worker checkpoint
reserves existing resource credits without a byte buffer, validates off-thread
in bounded units, and retains mappings through leases and the ResourceReleased
journal handoff. Tests cover those seams, including a supplied validated mapping
retired over the live wire; actual Source admission and Session enablement remain.
Worker validation honors the published positive chunk budget, capped at 64 KiB
between deadline/cancellation checks. Budgets that split pixels have independent
component-coverage tests; the six store and four worker cases and strict Clippy
pass. This is worker/store evidence, not end-to-end Source acceptance.

The next coordinated pins are Rust `8618793` and C `c3daefb`. Lock
Negotiated carries an explicit selected revision, with image capabilities valid
only at r2; plain entry points remain r1. Native lock schema/selection tests,
the complete Rust SDK gate, the C snapshot gate and full upstream C checks pass.
Runtime lock export/service, C lock-client interoperability and shell image
transport checks also pass. The C pin includes the sealed producer and opt-in
BAR adapter; Rust producer/adapter parity remains incomplete. Neither the pins
nor the native codec change enables lock r2 or Session image admission.

The shell journal now supports nine independent held records: ChannelClosed
and two terminal records per admitted offer. Handles are epoch-scoped and never
reused; aggregate holds fit inside the terminal reserve, and each spend leaves
the other holds intact. Saturation, out-of-order completion, stale handles and
byte-budget exhaustion have regression coverage. All 118 active runtime library
tests and the live image transport test pass (one pre-existing library test is
ignored). Four legacy test helpers were corrected for the store-aware drain
signature introduced in `8b08e3f71`; they had not been built by that checkpoint's
integration-only rerun. The reservations are a prerequisite, not Source admission.

Claude's custody review found two completion hazards before Source activation:
retired epochs had no resource-completion route, and an early grant/clock error
could drop a returned mapping while retaining its charge. The registry now
routes completion to the exact active or retired grant. The store borrows an
optional result and takes it only once the transfer is identified; callers keep
the result on an early error. Tests cover a real worker completion after
disconnect, clock-error retry, and reconnect cycles beyond the retired-epoch
limit. The worker can reserve its credit and deadline before ImageAdmitted,
then accept exactly one Source against that reservation. Unsent credits can be
returned; submitted credits survive cancellation until completion is collected.
The eight sealed-store and seven worker tests pass, alongside the runtime library
suite. Source receipt and admission wiring still remain.

The registry's sealed-image admission operation now checks the live content
grant, facts generation, output and allocation before reserving worker/resource
capacity. Resource descriptions inherit the allocation's scale and existing
content limits. Exhausted workers do not consume a resource generation, and
failed resource reservations return their worker credit. The allocation suite
passes 13 cases and targeted strict Clippy passes. This operation is not yet
called by the Source reactor: the export must first enforce its published image
budgets, increasing offer ID and terminal journal reservations, then retain the
admitted metadata and publish ImageAdmitted before accepting an FD.

First add all seven image kinds and exact body/row codecs independently in C
and Rust, with role-specific schema tests, literal vectors and negative cases.
The Go oracle must gain the same revision-aware ResourceStatus rule. Rust's
shared reason helper must not widen other reason fields; reason 13 belongs only
to ResourceStatus Rejected under shell r9. Keep all older revisions at <=12.
Lock ImageBegin must be explicitly classified Candidate; the existing lock
kind classifier otherwise defaults to Event. Update role capability profiles
without enabling support in production before the authenticated endpoint works.

The receive-and-refuse slice must complete authenticated Hello and publish
ChannelReady before ImageBegin can be refused with image_channel_unavailable.
Alternatively, honest capability absence permits no Hello at all; it cannot
claim to test ImageBegin fallback. Malformed/denied discovery and Hello timeout
remain failures. Clean refusal or absence selects bytes only under caller policy.
Discovery kinds, positive limits and granted capabilities must agree. Bound
ancillary reception and close every refused FD. Prove byte upload still works
on the same connection and refused offers retain no reservation or upload slot.

Only after this slice passes may t338 add seal validation, worker-backed mapping,
retained backing budgets and production cancellation/retirement. Finish with
paired panel evidence and rerun t331's four unchanged gates. DMA-BUF remains
behind t339/t304 and t289's real safe-completion proof.

Planning is allowed before t252 and t304 close: making completed t304 a dependency
of the design that t304 needs would be circular. Lock renderer implementation
still depends on the shared design and t289; shell/lock release gates stay intact.
The user-selected planning work does not make t331 complete.

## Measurement and red controls

Keep the frozen byte/IPC pilot and its gates. Descriptor qualification is a new
comparison against the same byte path on identical content, dimensions, cadence,
output and hardware. Report producer plus Sophia CPU/frame, allocations/frame,
bytes transported, source-copy/seal/readback time, GPU copy/upload work, end-to-end
latency, input responsiveness, memory and FD high-water marks. CPU per uploaded
MiB alone is meaningless when pixel bytes no longer cross the socket. Use
balanced repeated trials; distinguish paired-run variance from counter bounds.
For real shell/lock clients include rendering and fallback costs, not just the
synthetic peer. Idle/wakeup and x13 power comparisons remain t336's measurement
scope; this pilot did not measure them.

Required controls include wrong principal/epoch/generation, duplicate and late
offers, short/extra FDs, ancillary truncation, unsealed or wrong-size memory,
existing writable mappings, overflow and malformed pixels, EAGAIN without replay,
terminal status during capture, topology replacement, GPU stall, exhausted
budgets/journal, replacement connection, and release exactly once. Assert no
cross-role resource alias and no lock/input delay. Run old/new peers and equivalent
C/Rust cases independently. Device-free fixtures establish ownership, not GPU
completion or real-device performance; those need separate attended evidence.

## Source checks and connections

- At pinned Rust SDK `01474a9` and C SDK `5246f96`, lock resource transaction
  identities come from the caller: Rust `upload::Upload::begin` retains the
  supplied `LockResourceBegin`, and C `sophia_lc_upload_begin` copies its supplied
  begin record. End and Cancel reuse that identity. Separately, Rust
  `submission::Submission::admit` and C `sophia_lc_internal_queue` allocate
  increasing outer submission IDs and refuse exhaustion. Neither allocator
  assigns the resource transaction identity. Descriptor APIs must allocate or
  enforce increasing offer IDs in both SDKs without changing legacy byte-upload
  callers; include repeated/decreasing IDs and exhaustion in their fixtures.
- Claude independently decoded all 32 draft vectors against the offset tables
  and found no remaining proposed-layout findings. This validates the draft
  bytes, not real SCM_RIGHTS, seals, SDK custody or renderer completion.
- `crates/sophia-runtime/src/lock_files/transport.rs::poll_accept` now has an
  authenticated path; the old draft's pidfd-only description is incomplete.
- `crates/sophia-backend-live/src/presentation.rs` maps every raw transaction ID
  directly to/from a renderer image. A high-bit partition needs a registry change.
- `crates/sophia-renderer-live/src/native_scanout/owned_mixed_export.rs` currently
  propagates capture failure; it is not a per-role nonfatal worker seam.
- [Renderer ownership](../../renderer-import-boundary.md#generational-renderer-image-residency)
  requires bounded immutable captured images and safe generation transitions.
- [Linux sealing semantics](https://man7.org/linux/man-pages/man2/F_GET_SEALS.2const.html):
  F_SEAL_WRITE cannot be installed while a shared writable mapping exists;
  F_SEAL_FUTURE_WRITE permits existing writable mappings and is not a substitute.
- [Unix ancillary data](https://man7.org/linux/man-pages/man7/unix.7.html)
  documents descriptor transfer and ancillary truncation; receiving a file
  reference is not an authorization or GPU-completion proof.
- [t304 lock plan](r8z5a3sk-negotiated-dma-buf-lock-images-with-bounded-capture-and-release.md).
- [Measured B2/B3 result](../investigations/2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md#b2b3-native-pilot--october-17).

## Proposed byte layout, version 1

This is a review draft, not an allocated production namespace. All integers are
unsigned little-endian; unknown versions, flags, reserved values and kinds are
refused. Proposed role capability bits are shell 13 (sealed) and 14 (DMA-BUF),
gated at r9, and lock 2 (sealed) and 3 (DMA-BUF), gated at r2. Existing role
header/API version 1 remains unchanged. Descriptor support is never inferred
from a higher role revision alone.

### Channel packet header

| Offset | Type | Field |
| --- | --- | --- |
| 0 | u32 | total bytes: 40 for Hello, 200 for Source |
| 4 | u16 | channel version = 1 |
| 6 | u16 | kind: 1 Hello, 2 Source |
| 8 | u16 | role: 1 shell, 2 lock |
| 10 | u16 | descriptor count: 0 for Hello, 1–4 for Source |
| 12 | u32 | reserved = 0 |
| 16 | u64 | authenticated 9P connection epoch, nonzero |
| 24 | u64 | channel generation, nonzero |
| 32 | u64 | offer ID: 0 for Hello, nonzero for Source |

Hello is exactly the header. Source appends the 96-byte metadata below and four
16-byte plane rows. Every unused plane row is zero. The header descriptor count,
metadata descriptor count, plane count and actual SCM_RIGHTS count must all
agree; any mismatch is refused. No fence FD is permitted. FD order is the plane-row order,
and each used row's fd_index equals its zero-based row number. Repeated backing
objects are allowed only through separately received references that are each
counted and closed; they do not create extra resource budget.

### Common image metadata

Offsets in this table are relative to the metadata, not the channel header.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | u64 | offer ID; equals the Source header ID |
| 8 | u64 | resource ID, nonzero |
| 16 | u64 | resource generation, nonzero |
| 24 | u64 | shell content-grant epoch or lock epoch at admission |
| 32 | u64 | output ID, nonzero |
| 40 | u64 | allocation ID, nonzero |
| 48 | u64 | topology generation, nonzero |
| 56 | u32 | source kind: 1 sealed memory, 2 DMA-BUF |
| 60 | u32 | source format: sealed = 1 (premultiplied BGRA8); DMA-BUF = advertised fourcc |
| 64 | u32 | width in pixels, nonzero |
| 68 | u32 | height in pixels, nonzero |
| 72 | u32 | sealed row stride = width × 4; DMA-BUF = 0 (per-plane rows supply strides) |
| 76 | u16 | plane count: sealed = 1, DMA-BUF = 1–4 |
| 78 | u16 | descriptor count = plane count |
| 80 | u64 | declared logical image bytes = checked width × height × 4 |
| 88 | u64 | modifier: sealed = 0; DMA-BUF = advertised modifier |

First-phase DMA-BUF formats are advertised 32-bit RGB formats with a defined
alpha/premultiplication rule; planar YUV and arbitrary conversions are excluded.
An alpha-bearing format cannot be advertised until validation of the captured
owned pixels proves the same premultiplication rule as the byte path. Validating
the mutable source before copying is insufficient. An opaque XRGB format can
avoid that scan only if its sampling semantics force alpha to one. Until the
alpha-validation seam is proved, transparent clients keep the byte or sealed
path; no client feature is disabled to obtain a faster result. A sampled pixel
check is insufficient. Alpha validation must cover the complete owned image,
for example with a GPU reduction and bounded result readback, and must not
reintroduce a full pixel readback. Until that exists, translucent bars and docks
are outside the DMA-BUF first phase. For XRGB, forcing alpha to one happens in
the capture job and belongs in its measured cost.
A modifier's allocation footprint may exceed logical image bytes. The renderer
must bound and reserve its actual owned allocation separately before capture;
logical size is not an estimate of driver VRAM use. Unknown allocation bounds
mean refusal, not an unchecked reservation.

A plane row is `offset:u64 @0`, `stride:u32 @8`, `fd_index:u16 @12`,
`reserved:u16 @14 = 0`. Sealed memory uses offset zero and the metadata stride;
its file length equals declared bytes exactly. DMA-BUF offsets and strides must
pass checked arithmetic and the advertised import validator. Metadata must match
the admitted record byte-for-byte. A topology change cancels a pending offer;
an already accepted resource keeps the existing role's lifetime rules, and any
later candidate must still satisfy current allocation/epoch checks.

### Proposed 9P records

Use the role's existing 32-byte record header and its normal transaction, submit,
journal and ACK rules. Provisional kind numbers below are for draft vectors only;
check the complete namespace again when making the KDL normative.

| Kind | Direction | Body | Meaning |
| --- | --- | --- | --- |
| 28672 (0x7000) ImageBegin | candidate | 96-byte metadata | Reserve one descriptor offer; no source FD exists yet. |
| 28673 (0x7001) ImageAdmitted | event | exact 96-byte metadata | Reservation and terminal journal space exist; source may now be sent. |
| 28674 (0x7002) SourceReleased | event | 32 bytes below | The admitted offer no longer needs producer cooperation, whether or not its source arrived. |
| 28675 (0x7003) ChannelReady | event | 16 bytes: channel generation u64 @0, granted source-kind mask u32 @8, reserved u32 @12 = 0 | Hello passed; ImageBegin may follow. |
| 28676 (0x7004) ChannelClosed | event | 16 bytes: channel generation u64 @0, reason u32 @8, reserved u32 @12 = 0 | Descriptor admission disabled; no source release is implied. |
| 28677 (0x7005) ImageLimits | object | 104 bytes below | Fixed descriptor limits for this connection. |
| 28678 (0x7006) ImageFormats | object | 32 bytes plus format rows below | Format snapshot tied to topology. |

Existing ResourceStatus reports accepted/rejected/cancelled with its existing
meaning; ResourceReleased remains the last-consumer event. For lock records,
the metadata offer ID is the resource transaction identity used by subsequent
ResourceCancel and status records. It is distinct from the outer submission ID.
A descriptor offer has no ResourceEnd: validation or capture determines its
completion. Sending ResourceEnd against it is refused without changing custody.
ResourceRetire applies only after immutable acceptance, as on the byte path.

SourceReleased body: offer ID `u64 @0`, resource ID `u64 @8`, resource generation
`u64 @16`, disposition `u32 @24`, reserved `u32 @28 = 0`. Dispositions are
1 retained immutable backing, 2 capture complete, 3 refused/expired before use,
4 cancelled before import, 5 drained after GPU submission and withdrawal of
interest. Disposition 5 includes explicit cancellation, channel loss, expiry and
capture-interest timeout after submission; none permits release before proven
safe cleanup. A successful uncancelled capture uses disposition 2.
A live journal promises exactly one release for every admitted offer, including
one whose packet never arrived. Losing the journal leaves release unknown to
the producer. Disposition does not replace ResourceStatus or ResourceReleased.
ChannelClosed reasons are 1 peer disconnect, 2 protocol fault, 3 Hello timeout,
4 descriptor grant revoked. If 9P itself is gone, no ChannelClosed delivery is
promised. Closing the channel does not prove that a capture has finished.

The discovery file is bounded ASCII with a fixed first line
`sophia-image-channel version=1`, followed by one key/value per line: role,
connection_epoch, channel_generation, endpoint, source_kinds,
max_packet_bytes=200, max_fds=4, max_offers, max_packets_per_turn=1,
hello_timeout_ms=250, validation_timeout_ms=2000,
validation_chunk_bytes=65536, capture_interest_timeout_ms=500, and
formats_generation and limits_generation. Integers are unsigned decimal, source_kinds is a numeric
bitmask (bit 0 sealed, bit 1 DMA-BUF), and endpoint is a single basename with no
slash or dot component. Refuse duplicate, missing or unknown fields in version 1.
The separately versioned formats object publishes only this assignee's admitted
outputs/device, with at most 256 exact format/modifier rows per output.
The 250 ms Hello and 500 ms capture-interest values are provisional design
defaults, not qualified bounds; accept them only after loaded-startup and
supported-topology review. Exceeding a capture-interest deadline never asserts
GPU completion.

`max_offers` is min(4, the role's transfer-slot count). Those offers include
quarantined captures: a timeout never returns the slot until cleanup is safe.
Advertise DMA-BUF only when a renderer quota and safe-completion seam exist;
unavailable capacity is not hidden behind a promise to allocate later. Role
resource/live/retiring limits continue to apply. A separate immutable limits
object must publish per-role renderer entry/byte reservations before DMA-BUF is
advertised; its numeric grant is assigned from actual admitted topology and
remaining global capacity, not a new fixed global allowance.

### Discovery and companion objects

These layouts are proposed additions, not changes to the existing role objects.
Expose three read-only files in the authenticated role root: `image-channel`,
`image-limits` and `image-formats`. No file grants authority by being readable.
Legacy exports may omit all three; ENOENT on `image-channel` selects the byte
path only when the caller permits fallback. Once it exists, a missing companion,
malformed object or access error is a failure, not optional discovery.

`image-channel` is at most 2048 bytes, ASCII with LF line endings and a final LF.
The first line is exactly `sophia-image-channel version=1`. Remaining lines use
`key=value`, in this order, once each, with no blank lines, spaces or comments:

```
role=1
connection_epoch=1
channel_generation=2
endpoint=image.sock
source_kinds=1
max_packet_bytes=200
max_fds=4
max_offers=4
max_packets_per_turn=1
hello_timeout_ms=250
validation_timeout_ms=2000
validation_chunk_bytes=65536
capture_interest_timeout_ms=500
formats_generation=9
limits_generation=1
```

These values form the sealed-only shell fixture, not a universal capacity grant.
Lock changes role to 2 and uses its own limits. Integers are canonical unsigned
decimal (no signs or leading zeros except `0`), checked in their destination
width. `role` is the channel contract-family code (1 shell, 2 lock), not the
profile-name value in the adjacent `api` file. Role is u16; epochs and generations are nonzero u64; all other integers
are u32. `source_kinds` is a nonzero subset of bits 0 and 1. Endpoint is 1–64
bytes from ASCII letters, digits, `_`, `-` and `.`, excluding `.` and `..`.
It is resolved beneath the component endpoint directory containing the role's
9P socket, never as an arbitrary path. Version 1 requires max_packet_bytes=200
and max_fds=4 exactly; a reader refuses any other values.
Unknown, reordered, missing or duplicate fields are refused in version 1.

The three files are immutable snapshots per open fid. Reads at different offsets
on that fid cannot mix versions. Discovery names the limits and formats
generations; their connection epochs and body channel generations must match
discovery. The authenticated role root supplies the contract family; the common
record header does not repeat it. After a
mismatch, close all three and wait for the next caller drive pass. Acquire at
most one complete set per pass, including the initial acquisition,
under the existing connection/startup deadline. Do not spin or send Hello from
an incoherent set. A completed snapshot is discovery, not an admission promise:
ImageBegin checks current permission, topology, format support and budget again.
An output change does not authorize reading another role's outputs.

#### Image limits, 136 bytes

All integers are little-endian. Exact length is required. Reserved fields must
be zero. Use the existing 32-byte role record header: total=136, API version=1,
kind=0x7005, connection epoch, submission=0 and sequence=0. This grant and its
limits_generation are fixed for the entire connection; topology that requires
larger limits needs a new connection. Counters below are ceilings, not current
free-space reports. Runtime admission still reserves within them.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | 32 bytes | common role record header |
| 32 | u64 | channel generation |
| 40 | u64 | limits generation |
| 48 | u64 | max logical bytes per resource, no greater than the role limit |
| 56 | u64 | max retained sealed backing bytes, including retiring leases |
| 64 | u64 | max pending DMA source logical bytes |
| 72 | u64 | max owned renderer allocation bytes, including retiring images |
| 80 | u32 | max owned renderer entries |
| 84 | u32 | max offers, matching discovery and bounded by role transfer slots |
| 88 | u32 | max validation jobs, queued plus running |
| 92 | u32 | max capture jobs, queued plus running plus quarantined |
| 96 | u32 | max admitted outputs, 1–16 and no greater than role limit |
| 100 | u32 | max format rows per output, 1–256 |
| 104 | u32 | max width, no greater than role limit |
| 108 | u32 | max height, no greater than role limit |
| 112 | u32 | validation timeout in ms, matching discovery |
| 116 | u32 | capture-interest timeout in ms, matching discovery |
| 120 | u32 | validation chunk bytes, matching discovery |
| 124 | u32 | max packets per turn, matching discovery |
| 128 | u64 | reserved = 0 |

Epochs, generations, resource bound, dimensions and max offers are nonzero.
Validation jobs must be between 1 and max offers when sealed memory is advertised;
sealed backing capacity must then cover at least one maximum-size resource.
Otherwise both sealed fields are zero. DMA-BUF requires positive source capacity,
renderer byte/entry reservations and capture jobs (at most max offers); otherwise
those four fields are zero. These are separate bounds, not additive permission
to exceed the global image ceiling. Source bytes are logical accounting only;
driver allocations are not bounded by this number. Reserve actual destination
allocation before capture. Retiring resources can exhaust capacity even when
no offer is pending. Publication must refuse inconsistent grants.

#### Image formats, 64-byte header plus 40-byte rows

The first 32 bytes are the common role record header, with total=64+40*rows,
API version=1, kind=0x7006, connection epoch, submission=0 and sequence=0.
The next 32 bytes describe the snapshot. This reuses the normal object framing;
only ASCII discovery has a separate grammar.

| Offset | Type | Field |
| --- | --- | --- |
| 0 | 32 bytes | common role record header |
| 32 | u64 | channel generation |
| 40 | u64 | formats generation, matching discovery and role topology generation |
| 48 | u64 | limits generation, matching discovery |
| 56 | u32 | row count, 0–4096 |
| 60 | u32 | reserved = 0 |

Each row is `output:u64 @0`, `device_major:u32 @8`, `device_minor:u32 @12`,
`fourcc:u32 @16`, `flags:u32 @20`, `modifier:u64 @24`, `max_width:u32 @32`,
`max_height:u32 @36`. Output, fourcc and dimensions are nonzero. Device numbers
identify the already granted composing device; they grant no right to open one.
Flag 1 means opaque with alpha forced to one during capture; flag 2 means full
owned-copy premultiplied validation is supported. Exactly one flag is required;
phase one advertises only 1. Width and height cannot exceed the limits object.

Rows are ordered by unsigned `(output, device_major, device_minor, fourcc,
modifier)`, with no duplicate key. Enforce both per-output and total bounds from
the limits object, checked multiplication, and exact length `64 + 40 * rows`
(maximum 163904 bytes). Sealed-only discovery has zero rows: sealed BGRA is
defined by the source layout, not a fabricated GPU format. DMA-BUF rows describe
only authorized outputs on the granted device. An empty DMA list means no
currently usable import pair, not permission to assume LINEAR. A format change
requires a new role topology generation; formats_generation equals that value.
Publish the updated format snapshot before announcing the topology through the
role's existing topology notification. Even a format-only change advances that
topology generation. Clients re-read discovery and formats on that notification;
an already-open snapshot stays unchanged. Fixed limits do not change during this
retry. There is no independent unannounced formats update or new ObjectPublished
vocabulary in this draft. A stale ImageBegin topology is refused as shell Stale
(reason 1) or lock topology (reason 7), never as malformed. A client re-reads
after that refusal too, since its offer may race the topology notification.

This coupling has a cost. A format-only topology change makes shell components
fetch current output facts and rebuild/resubmit candidates. While locked, it
stales the provider's earlier allocations and requires new whole-output images
and presentation. The proposal accepts that disruption for renderer/device
reinitialization: format rows belong to outputs on the granted composing device,
so a changed import set must be handled as a topology-level event. This is a
design requirement, not evidence that every driver format change currently
produces such an event. Do not bump topology for an unchanged format set.

On acceptance, amend both contracts as well as the KDL resource-reason bound:
the shell contract's output-facts publication and candidate-validation rules
must include import-format changes; the lock contract's Lock-object publication,
allocation staleness and forwarded-candidate revocation rules must cover them
too. The current normative text does not yet grant this broader trigger. Test a
format-only change while shell candidates and lock images are live, including
retention of the lock cover throughout replacement.

#### Admission refusal

Propose `image_channel_unavailable` as ResourceStatus Rejected reason 13 for
shell r9 and reason 9 for lock r2. These numbers are unallocated draft values;
the normative codec change must extend only the negotiated revision's allowed
resource reasons, not every existing field that uses a reason code. A disabled
channel refuses ImageBegin before ImageAdmitted, so no SourceReleased is owed.
Other permission, size, stale-allocation and budget refusals retain their role's
existing meanings. Malformed transaction bytes keep ordinary submit errors.
ChannelClosed is the prompt state-change notification; this rejection covers
the race between observing readiness and submitting the next offer.

### C and Rust custody fixture contract

These are required independent fixture cases; the kernel-only results below
do not establish the whole matrix.
Use local SEQPACKET pairs and real SCM_RIGHTS/sealable files without a GPU or
live desktop. A protocol fixture represents the owner; it must not impersonate
an authenticated production endpoint. Run the same cases in each SDK's own
test suite, with separate codecs and no C dependency on Rust. Build new fixture
support only in those repositories or Sophia's existing generic conformance
seams, not in a named desktop client's harness.

| Case | Required observation in both SDKs |
| --- | --- |
| Optional discovery | ENOENT permits caller-authorized bytes; malformed, stale and denied discovery do not silently downgrade. |
| Snapshot replacement | Force two consecutive generation mismatches; mixed snapshots never produce Hello, each drive pass attempts at most one fresh set, and the startup deadline terminates repeated mismatch. One open fid remains consistent across multiple reads. |
| Readiness race | No ImageBegin before matching ChannelReady; old-generation Ready/Closed cannot control the new channel. |
| Duplicate channel / bad Hello | Refuse a second live channel without disturbing the first. A foreign/stale epoch or generation cannot join a channel or close another connection's channel; reject that socket without emitting ChannelClosed for the valid channel. An already joined channel's protocol violation uses ChannelClosed reason 2. |
| Offer IDs | Descriptor APIs allocate or enforce increasing IDs; retries retain identity, a fresh attempt uses a greater ID, and exhaustion requires reconnect. Legacy byte APIs remain unchanged. |
| Source send | No send before ImageAdmitted. A successful packet send transfers a reference, not proof of acceptance or release. EAGAIN retains the unsent offer and enables a writable-interest wake. |
| Ambiguous send | After an indeterminate send outcome, no blind replay, mutable reuse or invented acceptance; await authoritative disposition or report unknown custody. |
| Kernel seals | Missing WRITE/GROW/SHRINK/SEAL, FUTURE_WRITE alone and wrong size cannot produce Accepted. On the producer, an outstanding shared writable mapping makes adding F_SEAL_WRITE fail with EBUSY; the receiver observes the missing seal, not the producer's mappings. |
| Ancillary ownership | Missing/extra FDs, control truncation and malformed packets close all received references; receiver FDs have close-on-exec. Valid mapping lifetime survives producer close. |
| Expiry race | Released-before-arrival packets are drained and closed with no second release or retained bytes. Skipped lower IDs are discarded; above-mark packets fault. |
| Cancellation | Before packet receipt, after receipt, and during validation each end in one terminal status and one release on the live journal; no early mutable reuse. |
| Channel loss | Closed switches descriptor availability off for that epoch and withdraws interest in every live offer; release follows actual custody and safe cleanup, never socket closure alone; byte fallback follows caller policy. |
| Journal loss | No implied release from EOF or socket closure; no mutable reuse without authoritative release. |
| 9P epoch end | Stop admission and close channel/queued references; no events on the dead journal. Received references held by submitted GPU work survive only until proven safe cleanup, then close. No teardown step invents SourceReleased or frees an in-use source. |
| Capacity | Four offers cannot create a fifth reservation; terminal event capacity is reserved before admission; quarantine retains its slot. |
| Sealed backing | Release does not make the sealed source writable; resource retirement waits for final consumer leases. On a live journal, retain backing, slot and charge until ResourceReleased fits; retry on journal ACK. A dead journal owes no event. |

C low-level service remains clockless. The caller supplies deadline expiry and
poll readiness; timeout/interest accessors must prevent a stalled unsent packet
from sleeping forever. Rust must expose equivalent progress and ownership states,
even if its ownership types differ. Receiving SourceReleased once settles the
offer once; duplicated delivery cannot double-close or release another source.
Public C layout changes require consumer rebuilds and an API-PARITY entry.

GPU completion, safe rollback and quarantine cannot be proved by these local
fixtures. A fake completion signal tests state transitions only. Real DMA-BUF
acceptance stays behind t289's safe-completion seam and attended GPU evidence.

#### Sealed lifecycle peer checkpoint — October 17

Rust SDK `8f7dc5d` and C SDK `b2ca17d` are signed, published test-only candidates.
Claude's final cross-review and Muse's C review found no blockers. Rust's
`descriptor_custody` target and targeted strict Clippy pass. The producer and
owner exchange real 200-byte Source packets and SCM_RIGHTS for both role codes.
They exercise nonblocking send pressure and writable wake, cancellation and
expiry around receive/validation, malformed and late packets, unknown-send
no-replay, journal loss, separate terminal events and retained backing leases.
Wrong sizes, missing seals, unsupported seal queries and invalid premultiplied
pixels refuse through the owner model. Independent budget controls isolate offer,
byte and terminal-journal limits.

Muse's read-only review of `49d5ddf` found three fixture errors: cancellation used
the expiry disposition, admitted metadata was reconstructed rather than stored,
and status/release were collapsed into one model event. The follow-ups separate
those cases and events. A source offer also ends independently of its retained
resource backing; late packets are discarded after source release even while a
consumer lease holds the backing. Final review also fixed producer send deadlines,
shape checks before stale-ID lookup, and deferred ResourceReleased when the
journal is full. The proposed text now distinguishes packet faults from source
content refusal and states release rules after channel loss.

These are explicit producer/owner state models. Control admission, journal
delivery and worker completion are fixture calls; discovery retry uses immutable
owned bytes rather than real open fids, and authentication is a supplied predicate.
SourceReleased has a test-side record decoder. Neither an authenticated runtime
endpoint, cross-process owner, real worker nor GPU completion is established.
C independently exercises all sixteen matrix cases for shell and lock, with
real ancillary transfer, scripted discovery parsing, same-process SO_PEERCRED,
synchronous validation and structured journal events. Its full `check-files`
suite passes; Claude reports clean Clang ASan/LSan/UBSan runs and 29 rejected
mutants. Codex reran both C programs successfully. Rust's custody target and
targeted strict Clippy pass; Claude independently reran the custody target.
Rust's producer is single-offer, and journal-capacity recovery explicitly invokes
collection after consumption. Concurrent producer scheduling and a production
ACK wake are not established by that model.

The independent peers and cross-review satisfy the fixture condition for the
owner's acceptance in principle. Acceptance still requires the coordinated
normative changes below; this checkpoint does not make the layouts normative.
C is integrated on `development/t318-namespace-discovery`; Rust remains on
`performance/t331-reply-storage`. Muse's Sophia lane stays closed until acceptance.
No public API, vendor pin or installed desktop changed.

#### Kernel prerequisite results — October 17

Signed Rust SDK `05bf018` and C SDK `9bca0db` add independent Linux tests using
real SEQPACKET, SCM_RIGHTS and sealed memfds. Both pass required-seal checks,
FUTURE_WRITE insufficiency, producer-close survival, receiver CLOEXEC, immutable
bytes, missing/extra rights, ancillary truncation and bounded nonblocking
backpressure. Each process returns to its initial FD-table count; this does not
observe kernel-held in-flight references. C also verifies EBUSY when adding F_SEAL_WRITE with a shared
writable mapping; Rust retains the workspace unsafe-code prohibition and tests
the seal requirement without creating a mapping.

Review follow-up Rust `ad9120c` / C `bb402f9` asserts zero delivered FDs on
control truncation and socket writable readiness after draining the bounded
burst. SO_SNDBUF is bounded; ETOOMANYREFS is a valid distinct stop because the
per-user in-flight rights cap can precede socket EAGAIN. A descriptor-free send
then proves socket progress without assuming a global rights credit. Production's
four-offer ceiling bounds its own contribution but cannot reserve another user's
or process's rights allowance. C additionally proves a read-only sealed mapping
survives closing both source FDs and that shared writable mapping is refused.
Both revised targets pass; targeted strict Rust Clippy passes again. These checks
do not measure the release time of kernel references invisible in the FD table.

Rust's `cargo-slot sdk-rs cargo test -p sophia-shell-client --offline -q` passes,
as does strict Clippy for the new `descriptor_kernel` target. C `make check`
passes, including the Linux-only `descriptor_kernel_test`. The fixtures live in
the SDK test trees, with matching API-PARITY scope notes. No public API, normative
contract or Sophia vendor pin changed; no GPU or live session was accessed.

These are kernel-prerequisite results, not an implementation of the proposed
channel. Full draft codec, authentication/admission, cancellation, journal
custody, timed retries and cross-process owner fixtures remain outstanding.
The probes do not establish performance, GPU completion or design acceptance.

#### Draft decoder results — October 17

Rust SDK `47397e5` and C SDK `c6d7837` independently decode the 53 draft packet
and object cases in test-only code. Each also refuses every truncated prefix of
a valid case and an appended byte. The cases are literal test-support inputs,
not normative spec copies. Both use the same admitted sealed-only context;
arbitrary grants, full DMA format lists and production negotiation are outside
this coverage. No public codec or runtime feature is exported.

On Linux, each decoder checks a real sealed file against the Source packet's
declared length: sizes 0, 7 and 9 refuse a declaration of 8, while 8 passes.
Unsupported seal queries fail closed. This is a local FD check, not a complete
authenticated receive/admit exchange or pixel validation. Rust's new target and
strict Clippy pass; C `make check` passes. Lifecycle fixtures for readiness,
admission, cancellation, source release and journal loss remain the next work.

Follow-up Rust `7977ba7` and C `90307ac` add expected refusal categories to
all 53 cases. These are test diagnostics, not new wire reason codes. Snapshot
generation values are now decoded independently and compared between discovery,
limits and formats. For each generation axis, matching changes to generation 3
pass; discovery-only changes and each participating object-only mismatch fail
as incoherent. The grant/allocation and sealed-only scope remain fixed.
Both targets and strict Rust Clippy pass. The C target also passes Clang
ASan/UBSan; GCC's sanitizer link was unavailable because its runtime libraries
are not installed. This checks decoding/coherence, not the timed retry policy
or lifecycle peer behavior, which remain outstanding.

Rust `745272f` and C `1516463` refine the formats diagnostic order: exact
`64 + 40 * rows` length precedes capacity and sealed-only support. The 54-case
corpus adds one row declared in a 64-byte object (Length); the earlier short
4097-row case also reports Length. A separately constructed complete 4097-row
object reports Capacity. Both targets, strict Rust Clippy and C Clang ASan/UBSan
pass. This changes fixture diagnostics only.

### Draft golden cases

The first encode/decode review uses these canonical values: shell role 1,
connection epoch 1, channel generation 2, offer 3, resource 4, generation 5,
content epoch 6, output 7, allocation 8, topology 9, a 2 × 1 sealed BGRA image,
stride 8 and declared length 8. Hello has no ancillary data. The Source packet
has one sealed 8-byte FD and one used plane row. Equivalent lock cases change
role to 2 and interpret metadata epoch 6 as the lock epoch; no layout changes.

Required negative byte vectors independently alter total length, version, role,
epoch, generation, offer, source kind, reserved fields, plane/FD count and used
plane row. Ancillary fixtures separately test actual FD count/truncation and
seals; hex bytes alone cannot establish SCM_RIGHTS or kernel seal behavior.
Thirty-two draft hex vectors, including ChannelReady, ChannelClosed, malformed
Hello packets and sealed-layout arithmetic cases, are retained in
`~/.local/state/sophia/development-evidence/t337-design-20261017/draft-vectors.json`,
SHA-256 `d76d9b3f84854dabb04a223c522bd6001af2b85a0fc73b2987de16e1d6b6e71a`.
The metadata encoding agrees between contiguous packing and an independent
offset-by-offset construction, and packet lengths match these tables. This is
a layout check, not independent SDK conformance or actual ancillary/seal testing.
The C/Rust codec and custody fixtures remain required before implementation can
be adopted; the draft vectors allocate no normative opcodes.

The earlier companion `draft-object-vectors.json` in the same evidence directory
contains 19 independently reviewed cases for the superseded magic framing, SHA-256
`9ea7c2eee0808241c572a2d936a99119802fb5f69b1851581d88b11671f72d84`.
It remains historical evidence. Current common-header layouts use 21 cases in
`draft-object-vectors-r2.json`, SHA-256
`8444e92f1f1db24165f2576aa02376a4f3f46825169cc4792fc517325771775b`.
The two added cases refuse changed version-1 packet and FD constants.
The format-row example checks byte shape only: it does not advertise that device
or format on this machine. The sealed-only fixture has no GPU rows. Negative
cases cover grammar, generation disagreement, length/reserved values and bounds.
These object vectors now pass the independent test-only C/Rust decoders described
above; general production codecs remain outstanding. Shell and
lock binary objects now have identical fixture bytes because the common header
uses the authenticated role context, not an extra role field.
