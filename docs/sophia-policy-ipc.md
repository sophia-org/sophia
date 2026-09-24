# Sophia Native Protocol Family

**Role:** normative common wire, lifecycle, and evolution contract for
replaceable native desktop roles.
**Status:** the 24-byte frame envelope and `sophia_wm_v1` major 1 revision 3
are stable. Other interfaces retain the individual status recorded below; this
document does not promote an experimental role by grouping it into the family.

Sophia exposes replaceable desktop components through local, language-neutral
IPC. The protocol family is the public extension point. Hagia, shells, and
later authorities are ordinary clients of separately authorized interfaces;
none has a private Engine entry point. Legacy X11 WMs are ported to the native
roles rather than hosted through a compatibility bridge.

This is the developer entry point for the shared protocol contract. A role
specification defines which facts and proposals may cross one authority
boundary. It inherits the envelope, negotiation, identity, transaction,
transfer, outcome, recovery, and evolution rules here instead of inventing a
parallel transport model. One family does not mean one socket: roles retain
separate endpoints, capabilities, disclosure budgets, and protection domains.

## Interface Status

| Interface | Authorized role | Current status | Role specification |
| --- | --- | --- | --- |
| `sophia_wm_v1` major 1 revision 3 | metadata-blind spatial policy | stable | [Sophia Window Manager API](sophia-wm-api.md) |
| `sophia_shell_v1` major 1 revision 8 | metadata-bearing shell | experimental descriptor and admitted content workflows; r7 native launcher and r8 persistent catalog are separate supervised profiles | [Sophia Shell Interface Direction](sophia-shell-v1-direction.md) |
| `sophia_output_v1` major 1 revision 1 | exclusive output policy | experimental schema, codec, authenticated transport and live output owner; no stability promise | [Output Authority v1](sophia-output-v1.md) |
| `sophia_control_v1` major 1 revision 1 | explicitly admitted host administration; no role authority | experimental Linux endpoint; policy actions and confirmed restart | [Sophia Control v1](sophia-control-v1.md) |
| later broker, portal, and session families | separately authorized services | not specified | future role specifications |

A shared envelope or implementation crate does not give one interface the
status or compatibility promise of another. Every stable role names its own
major, revision, capability set, retained corpus, and independent implementation
evidence.

## Contract And Source Of Truth

Authority and disclosure rules come from Sophia's normative architecture. This
document owns the common transport and lifecycle semantics. A role specification
owns that interface's permitted facts, proposals, state machine, and recovery
behavior. The checked-in KDL schema for a role owns its binary layouts, message
kinds, fixed limits, and wire-level enum values.

Golden and malformed corpora pin the schema's bytes and rejection surface; they
are evidence, not an alternate specification. Generated codecs, headers, wire
tables, reference clients, generators, and check scripts are conveniences and
conformance tools. They cannot add behavior or authority absent from the
normative prose and role schema. Any disagreement among those sources is a
release-blocking specification defect rather than permission for an
implementation to choose whichever form it prefers.

## Common Developer Lifecycle

The public [scripting interface](scripting.md) is a separate
session-owned control service. Its callers are not supervised role peers,
and commands do not tunnel WM, shell, or broker packets through another
endpoint. Existing role negotiation and authority boundaries remain intact.
The experimental [control v1 specification](sophia-control-v1.md) and
[schema](../protocol/sophia-control-v1.kdl) define its separate wire surface
(kinds 128–134), with generated tables, vectors, and an independent example.
The control service and CLI implement policy actions and confirmed restart. Generic shell commands
still require a negotiated extension; control adds no role capability.

Role interfaces use the same conceptual lifecycle even when their frozen
message names differ:

1. The session creates the role-specific owner-only endpoint and supervises the
   separately authorized peer.
2. The client sends `ClientHello` with its supported revision range and required
   or requested capabilities. `ServerWelcome` selects the revision, admitted
   capabilities, connection epoch, and effective bounds. WM/output intersect
   requests; shell requires its requested set and may add its documented baseline.
3. The session sends one complete immutable fact set or snapshot for the
   transaction and applicable authority generations.
4. The client returns one bounded complete candidate or proposal tied to those
   exact identities. It never mutates Engine state directly.
5. The owning authority validates the whole candidate and reports an explicit
   prepared, presented, committed, stale, rejected, superseded, timed-out, or
   role-specific failure outcome. A role admits only the subset its schema
   defines.
6. Disconnect or replacement advances the connection epoch, discards incomplete
   old-epoch work, and preserves the last coherent committed or presented state.
   A replacement begins again from a complete current fact set.

“Candidate” is the common prose term, not a wire rename: the stable WM family
uses projection messages, while the experimental shell family uses candidate
messages. Role specifications map their exact messages onto this lifecycle.

A conforming client must be implementable from the published normative
documents, checked-in role schema, and ordinary Unix IPC using fixed-width
primitives. It must not require Sophia's Rust crates, a generator, generated
bindings, implementation-source inspection, or knowledge of which crate handles
a message. Independent bindings and clients prove this property; they do not
create it.

## Ownership

The session runtime owns endpoint creation, peer admission, process lifetime,
and bounded transport I/O. Engine owns physical input, authoritative scene
state, proposal validation, atomic commit, rendering, and scanout. A policy
client owns only its private model and may submit bounded proposals.

Interfaces are role-specific, but endpoint separation alone is not an
isolation boundary. A **protection domain** is the set of processes that can
exchange authority through ambient IPC, shared writable memory, inherited
descriptors, debugging, or equivalent unsupervised channels. The session may
grant several compatible roles to one domain only when their composition does
not violate a blindness invariant.

The spatial-policy role may not share a protection domain with a
metadata-bearing shell, metadata/portal broker, or application frontend. A
desktop may reuse one executable, repository, or language by launching
separate supervised, sandboxed processes with no ambient cross-domain IPC;
those facts never combine capabilities. An Engine-rendered tier-0 indicator is
not a shell endpoint and does not create this conflict.

The spatial-policy interface never exposes XIDs, protocol objects, namespaces,
titles, classes, PIDs, paths, client pixels, raw input, renderer handles, portal
payloads, tags, or policy-private view identities. A shell interface may later
receive broker-approved presentation data, but that grant cannot confer focus
or placement authority.

## Language Neutrality And Wire Authority

The wire specification is independently implementable. A client does not need
to link a Sophia library, use Rust, run a generator, or adopt Wayland, CBOR, or
another serialization runtime.

Role protocols are also independent of application frontends and UI toolkits.
`sophia_shell_v1` uses its admitted native role endpoint without an X11 or
Wayland connection. Future application frontends must translate into Engine's
protocol-neutral boundary; they do not redefine the shell interface. X11 is
the sole active application frontend. Quickshell and other downstream adapters
may supply requirements and conformance evidence, but their object models,
rendering APIs, and dependencies have no wire authority.

Sophia retains its fixed 24-byte little-endian envelope:

```text
u32 magic              "SOPH"
u16 frame_version      1
u16 message_kind
u64 transaction_id
u32 payload_len
u32 reserved           0
[payload bytes]
```

The frame version describes only this envelope. Each role negotiates its own
interface family and revision inside bounded handshake packets. Unknown frame
versions, message kinds, closed enum values, nonzero reserved fields, excessive
counts, truncated frames, and trailing bytes fail closed. A role must explicitly
identify an open diagnostic field; output revision 1's reason code is one.
The payload ceiling is 65,536 bytes, excluding the 24-byte header.

Every published interface layout has one checked-in declarative schema.
Repository tooling may generate Rust and C codecs, documentation tables, and
golden vectors, but generated outputs are checked in and normal builds do not
run the generator. The schema vocabulary contains only fixed integers, opaque
IDs, strict booleans, enums, flags, bounded vectors, bounded bytes or UTF-8
text, optionals, records, and tagged unions. It has no maps, recursion, floats,
implicit defaults, or unbounded fields.

The current public role schemas are:

- `protocol/sophia-wm-v1.kdl` for stable `sophia_wm_v1` revision 3;
- `protocol/sophia-shell-v1.kdl` for experimental `sophia_shell_v1`
revision 8, retaining revisions 1–7 under their capability gates. Revision 7
adds the native launcher and revision 8 the persistent catalog; neither grants
arbitrary component roles or general service access. See the
[capability map](native-desktop-capabilities.md) for source-backed scope; and
- `protocol/sophia-output-v1.kdl` for experimental `sophia_output_v1` revision 1.

The separate scripting service uses `protocol/sophia-control-v1.kdl` for
experimental control major 1 revision 1. It shares this family's envelope but
has its own host-admission, sequencing, and settlement contract; it is not a
supervised role schema.

Output's schema generates wire tables and valid/malformed samples independently
of its handwritten codec. `output_schema` tests their equivalence. Extraction
does not promote the experimental interface or supply independent lifecycle
evidence by itself.

The WM [wire tables](generated/sophia-wm-v1-wire.md), Rust and C99 codecs, and
valid and malformed corpora are one generated unit. The shell schema is checked
against its maintained codecs, independent clients and corpora; it is not a
second fully generated codec. `tools/check_policy_protocol.sh`
and `tools/check_shell_protocol.sh` exercise the current role implementations
against their shared bytes. A single family-level conformance entry point
remains a roadmap gate; the separate scripts do not define separate protocol
semantics.

### Role lifecycle differences

These are intentional specializations, not interchangeable packet layouts.
The role contracts and schemas own the details; the table is an audit map.

| Dimension | WM r3 | Shell r1–8 | Output r1 |
| --- | --- | --- | --- |
| Revision selection | Range must contain stable 3 | Descriptor selects 1–6; supervised launcher/catalog profiles select exactly 7/8 | Range must contain 1 |
| Capabilities | Requested intersection; startup requires profile activation | Required set, dependencies and supervisor grant; descriptor baseline may add bits | Requested intersection, observe mandatory, configure optional |
| Effective bounds | Welcome outputs/surfaces/bindings/chunk bytes | Welcome descriptors/labels/activations; separate content limits and fixed schema ceilings | Welcome heads/groups/modes/members; fixed label ceiling |
| Epochs | Connection, snapshot and opaque object generations | Connection plus broker, grant, resource and presentation generations | Connection, topology, head and output generations |
| Transaction identity | Exact request/snapshot/projection correlation; independent control namespaces | Operation-specific correlation plus exact candidate/action/resource identity | Publication and proposal IDs independent; proposal binds base topology epoch |
| Complete transfers | Begin/chunk/end, exact counts and ordinal; capability-gated extension suffix | Whole descriptor frames; counted content/allocation transfers | Whole nested snapshot/proposal frames |
| Outcomes | Committed/stale/invalid/timeout/disconnected; distinct control outcomes | Prepared and Presented distinct; resource and candidate outcomes separate | Validated/committed/stale/rejected/rolled-back/failed; open diagnostic reason |
| Recovery | Reject incomplete/stale work, preserve committed layout, fresh snapshot after reconnect | Revoke interaction immediately, retain backing only through exact leases, fresh grants | Abandon old connection work; preserve or roll back coherent topology, fresh snapshot |
| Evolution | Frozen r3, explicit outbound capability gates and uncounted extension records | Explicit experimental revisions and supervisor profiles; no WM extension area | No extension area; no silent additions to r1 |

Welcome field order is role-specific: shell places connection epoch before
capabilities, while WM and output place capabilities first. Their message kinds
and schemas make this distinction explicit. Shared framing is not permission
to decode every role with one handshake payload struct. None of these audited
differences changes the frozen WM revision.

`tools/check_control_protocol.sh` checks schema-derived control tables and
vectors against an independent Python example. This is offline wire/client
evidence only; it does not exercise an implemented service or prove admission,
owner settlement, or fairness under live traffic.

## Endpoints And Admission

The Sophia session creates one socket per role beneath a mode-0700 session
directory in `$XDG_RUNTIME_DIR`. The WM path is advertised as
`SOPHIA_WM_SOCKET`; the experimental shell path uses `SOPHIA_SHELL_SOCKET`. Sockets
are owner-only. The session accepts only the expected supervised peer with
matching credentials and permits one active client for each exclusive role.

The client sends `ClientHello` first, carrying its minimum and maximum supported
revision and its requested capability bits. The server returns `ServerWelcome`
with the selected revision and capabilities, a server-owned connection epoch, and
effective limits. No other message is legal before this exchange succeeds.

`ClientHello` deliberately carries **no interface-family field**. The family is
already determined twice over — by the role socket the client connected to and by
the message kind itself — so a family field would add a permanent wire field with
no information gain. This is a settled decision, not an omission: adding one after
a revision freezes is impossible, so it is recorded here rather than left to be
rediscovered.

The connection epoch distinguishes process incarnations. The transport worker
tags every decoded item with that epoch, and every client proposal repeats it.
Closing or replacing a connection invalidates all partially assembled and
queued work from the earlier epoch.

Matching an expected UID and PID authenticates an endpoint; it does not prove
protection-domain separation. Before any metadata-bearing shell role is
installed, session supervision must also enforce the forbidden role
compositions above, close unneeded inherited descriptors, and apply the
project's chosen process-isolation mechanism.

The role socket enforces the first of those. A metadata-bearing role -- shell or
metadata broker -- refuses admission on a supervised PID alone and takes instead
the launch evidence its supervisor produced, which must carry that role's
protection-domain role. A supervisor that built no domain has no evidence to
offer and cannot admit. Naming an expected PID at bind time is refused for the
same reason, so the constructor is not a second door into the rule. The metadata
broker transport publishes no PID-only call at all, which makes the omission a
compile error rather than a quiet admission.

This requirement lives at the socket because the forbidden-composition check
lives in `ProtectionDomainSpec`, where it fires only for a caller that builds a
domain. Building none used to produce no boundary and no complaint.

Evidence is a passive record whose fields any caller can write. It is therefore a
declaration the supervisor makes, not a proof the socket verifies: it closes
silent omission, not deliberate misreporting.

The blind spatial-policy and output roles still admit on a supervised PID.
Requiring a domain for every role has to answer for hosts with no `bwrap`, and
that decision stays separate from this rule rather than arriving as a side
effect of it.

## Versioning

The first public spatial-policy family is `sophia_wm_v1`. Revisions within an
interface are additive. A server sends only messages and record content admitted
by the selected revision and capabilities. An incompatible redesign uses a new
interface family rather than changing an existing layout.

### The Forward-Compatibility Rule

When `sophia_wm_v1` freezes, three clauses govern every later addition. They are
stated together because each one only holds if the others do.

1. **The frozen revision is final for record layouts and enum vocabularies.**
   Field order, field width, record size, and the set of admitted discriminants in
   any server-to-client enum or bitfield cannot change unconditionally. A new
   discriminant requires an explicit outbound gate on the selected capability,
   including a refusal before any frame bytes are written. Unknown discriminants are
   rejected, not skipped, and an enum value sits at a fixed offset inside a
   fixed-width record where no side channel reaches it. Client-to-server
   discriminants remain additive, because a server accepting more never breaks an
   older client.
2. **New WM-side facts arrive as capability-gated extension chunks.** An extension
   chunk carries a record kind from the reserved extension range **`0xFF00` through
   `0xFFFF`, in both the snapshot and projection transfers**, and is *not* counted
   by any `*Begin` count field, which is what keeps it out of the frozen message
   layouts. Ordinary record kinds stay sequentially allocated from 1, so the two
   ranges cannot collide. An extension chunk must carry at least one item, must
   append last so chunk ordinals stay dense, and must be sent only to a client that
   negotiated the governing capability. Receivers continue to reject unknown record
   kinds; that fail-closed behavior is preserved precisely because gating
   guarantees they are never sent one they did not ask for.

   The schema generator enforces this partition: an ordinary record declaration in
   `0xFF00`+ is rejected before bindings, facts, or corpora can be regenerated.
3. **New authorities take new interface families.** Shell, broker, portal, and
   session interfaces get their own family, their own role socket, and their own
   revision line. They never appear as placeholder messages inside the WM
   interface. The 24-byte envelope is role-neutral, so this costs the WM interface
   nothing.

#### Broker-issued classifications are extension-chunk content

Clause 2 is not hypothetical: it has one named first user. Broker-issued surface
classifications — the reduced, opaque form of what a host's window rules ask for —
cross as `(surface, classification)` records in a capability-gated extension chunk,
never as fields of `SnapshotSurface`.

The retained first use is concrete: capability bit 10 (`launch_placement`) gates
snapshot record kind `0xFF00`. Each record is 16 bytes — `surface_index: u32`,
`surface_generation: u32`, and opaque nonzero `classification: u64`. The trusted
registered-launch coordinator issues at most one classification, for the first
surface it observes from that launch. The session retains the grant across stale
responses and policy reconnect, and consumes it only when that surface's manage
projection commits.

The reason is that such rules are mostly parametric. A default workspace, a column
proportion, a named scratchpad, and a floating position are values, not classes, and
no bitfield or enum carries them. `SnapshotSurface.kind` and `capability_bits`
therefore stay reserved for what a surface *is*, and remain free for that purpose.

Two consequences follow, and both are the point:

- The classification vocabulary is not frozen with the revision. A rule family
  recognized after the freeze is a new record in the chunk, gated by the capability
  a client already negotiated.
- A client that negotiates nothing receives no classifications and behaves exactly
  as it did before they existed. Placement rules are advisory to policy; a WM that
  ignores them still produces a coherent desktop.

Ordinary record kinds stay sequentially allocated from 1 so they cannot collide with
the reserved range. Classifications never carry title, app ID, PID, path, or the
match expressions that produced them.

The rule depends on outbound capability gating. A producer that ignores the
negotiated capability set can send a frozen client something it must reject, which
would make clause 2 unsound and clause 1 unenforceable. Outbound gating is
therefore a prerequisite of the freeze, not an optimization, and one pinning test
must assert that the default capability set produces a byte-identical stream.

Because gated-off chunks are elided rather than emitted empty, adding a gated
extension chunk must leave the default-capability byte stream unchanged. That is
the property the pinning test checks.

Golden corpora pin the default capability set plus each capability in isolation.
They do not enumerate capability combinations; without that bound the corpus grows
multiplicatively with every new bit.

Once an interface revision is declared stable, later Sophia releases continue
to accept it. Old wire records are decoded at the session boundary and reduced
to the current internal model; Engine does not accumulate version branches.
Retiring a stable revision requires an explicit security amendment to the
project specification.

The former Rust WM API v7 was an experimental implementation contract and has
been removed. Its message-kind numbers remain unassigned; they are not a
compatibility surface.

## Optional presented pointer focus

Revision 3 retains its existing layouts and default vocabulary. The optional
`pointer_focus` capability (bit 13) permits `ProjectionRequest.cause_kind = 4`.
Unlike an ungated enum expansion, the actual socket send path refuses this cause
unless the peer selected bit 13. Existing clients receive their existing byte
stream; unnegotiated requests are rejected before any frame bytes are written.
This explicit outbound gate is the compatibility requirement for this addition.

The `action` u64 slot contains the nonzero destination output ID, which must be
in the affected-output set. The optional target is absent only when both target
fields are zero. A present target must have a nonzero generation and an index
other than `u32::MAX` (index zero is valid). Activation serial and all interaction
fields are zero. The target, when present, must be a current focusable surface
on that output. No coordinates, devices, buttons, or application metadata cross
this boundary.

Hagia requests the capability only when its `policy.focus-follows-mouse` setting
is enabled. Omission and explicit false preserve click/keyboard activation.
An enabled profile fails admission if the server cannot negotiate the capability.
A reduced observation is an input to policy, never an instruction that bypasses
policy or Engine validation. Focus and active output change only on commit;
rejection, timeout and connection replacement preserve the last committed state.

## Bounded Transfers

One frame's payload remains limited to 64 KiB. Complete role fact sets and candidates
may therefore use strict begin/chunk/end transfers. A role specification maps
its frozen message names onto those phases. Every chunk repeats the transaction
and connection identity and carries an exact ordinal. The begin record declares
all category counts; the end is accepted only after those counts and ordinals
match exactly. Output revision 1 and shell descriptor messages carry complete
bounded values in single frames instead; socket fragmentation does not create
begin/chunk/end messages. Shell content/allocation and WM transfers use their
own schema-defined chunk headers and explicit totals.

Each direction permits one transfer in flight and one coalesced latest pending
fact set. Partial, duplicate, reordered, excessive, or timed-out transfers are
discarded without changing committed or presented state. Each role schema
publishes hard ceilings, and the welcome packet may select tighter effective
bounds. Stable `sophia_wm_v1` currently permits at most 16 outputs, 1,024
manageable surfaces, and 256 binding registrations; those values are not
automatically shell or output limits.

Coalescing applies to replaceable scene refreshes, adjacent presented pointer-focus
observations, and continuous reduced interaction geometry. Non-idempotent action activations use a bounded ordered
queue and are never merged merely because they carry the same opaque action
token.

## WM Interface

Stable `sophia_wm_v1` specializes this family for metadata-blind spatial
policy. Its complete snapshot, projection, logical-output, action, interaction,
settlement, and recovery semantics live in the [Sophia Window Manager
API](sophia-wm-api.md). Those rules are not common shell or output semantics and
are not duplicated here.

The production session, Rust reference client, independent
Hagia implementation, and immutable archived C99 client all enter through the
same revision-3 role socket and canonical Engine reducer. None is a privileged
alternative to the public protocol.

## Output Authority Interface

`sophia_output_v1` is a separate exclusive role socket. Session supervision may
grant it to the supervised WM or shell process, but possession never widens
`sophia_wm_v1` or `sophia_shell_v1`: the peer negotiates the output role on
`SOPHIA_OUTPUT_SOCKET`, and exactly one authenticated supervised PID owns it at
a time.

The session sends a bounded complete capability snapshot. Head and mode IDs are
opaque session identities; labels are bounded connector-neutral display labels.
The snapshot contains current and preferred modes, transform and VRR capability,
current logical groups, and a topology epoch. It contains no DRM card, CRTC,
plane, framebuffer, render target, connector name, or native resource handle.

The authority replies with one complete candidate, not incremental connector
commands. Every enabled head independently names its mode, transform, and VRR
policy. Every logical group completely names its members: one member is an
extended output and several members are a mirror group. Groups name logical
rectangles, so one proposal can mirror selected monitors while extending others.
Omitted connected heads are disabled. Current logical IDs may be preserved; an
invalid output ID asks Engine to mint a new one.

Engine validates the candidate against the exact capability and topology epoch
and settles one active proposal plus one latest complete successor. The physical
owner prepares every target and first frame before apply. A partial apply enters
rollback, never a degraded commit. The old policy-visible topology remains
published until every new logical output has presented once. Outcomes are
explicitly validated, committed, stale, rejected, rolled back, or failed.

The schema, Rust codec, authenticated transport, live-session role assignment
and native apply/rebuild path are implemented. The
[role contract](sophia-output-v1.md) specifies the intentional differences in
transaction correlation, complete frames and open diagnostic reasons.
Independent lifecycle and retained compatibility evidence remain necessary
before the interface revision can be called stable.

## Shell Interfaces

The protocol family reserves a distinct shell role and endpoint, not placeholder
shell messages in the WM interface. Experimental `sophia_shell_v1` revision 1
provides the title-only descriptor switcher and bounded reservations; revision
2 adds persistent tab descriptors, revision 3 adds reference sheets, revision
4 adds the application catalog and launcher, revision 5 assigns the separately
admitted content vocabulary, and revision 6 adds view indicators and their
activations. Revision 7 adds the native launcher and revision 8 the persistent
catalog. Production can admit explicit content component profiles under their
protected grants; the legacy descriptor entry point still denies content.
Wire support alone is not a runtime grant, a rendering path or GPU access.
The [content design](notes/decisions/6ndjwffd-content-capability-design-for-sophia_shell_v1.md)
and [implementation record](lom-content-implementation.md) name the remaining
content lifecycle and admission gates.

The [Sophia Shell Interface Direction](sophia-shell-v1-direction.md) specializes
the common negotiation, complete-fact-set, bounded-candidate, explicit-outcome,
epoch, and recovery lifecycle here. It does not define a shell-only transport
or an alternate route into Engine.

Its minimum boundary is already fixed: Engine retains composition and physical
hit-testing, and renders the current descriptor chrome;
brokers retain metadata sanitization; the shell receives only authorized
presentation facts and emits bounded shell proposals or opaque actions. Shell
authority cannot set application placement or focus, and WM authority cannot
acquire shell metadata.

Revision 1 sends complete bounded snapshots of opaque slots, sanitized labels,
trust, attention, and issuer-scoped action capabilities. It never sends a
surface identity, coordinate, icon, PID, path, class, namespace identity, or
raw input. The shell returns ordering, selection, and visibility; Engine
privately resolves and renders the candidate, owns hit testing and capture, and
delivers an exact presented activation. Prepared and Presented are separate
outcomes. Disconnect and queue saturation revoke interaction immediately even
when Engine retains the last pixels until a later complete presentation.

Shell reservations also cannot race policy projection as independent commits.
The target transaction chain is shell candidate and reservation, exact Engine
work-area generation, exact WM snapshot and projection, then one coherent
logical presentation. A stale or unready member rejects the candidate while
the prior presented bundle remains intact. Security surfaces use their own
preemptive authority path and do not wait for an optional desktop shell or WM.

## Evidence Before Stability

Each role earns stability independently. Before a role freezes, its normative
prose, checked-in schema, generated outputs, and valid and malformed corpora
must agree. Rust and at least one independently implemented non-Rust client
must decode the same bytes, and a black-box client must complete the role's
minimum negotiate/facts/candidate/outcome/recovery lifecycle through the
canonical owning authority. A stable role retains an immutable old client as a
compatibility gate for every release.

`sophia_wm_v1` revision 3 has met that gate. The Rust reference client,
standalone Nim Hagia client, and archived C99 client exercise
the same role socket and reducer. Their retained coverage includes negotiation,
capabilities, complete and chunked transfers, actions, interactions, geometry,
constraints, visibility, multi-output moves, focus, stale rejection, atomic
failure, timeout, crash, restart, and last-layout preservation.

Hagia has no Triad history or River/Wayland runtime dependency. Its independent
envelope and record decoder pass the retained valid and malformed corpus, and
its proof client completes an authenticated snapshot/request/projection/outcome
cycle without linking Sophia's Rust protocol implementation. That proves the
stable WM role's language neutrality; it does not promote the experimental
shell or output roles.

Every later stable role owes the same kind of independent full-lifecycle proof,
adapted to that role's vocabulary and authority boundary. The repository's
current per-role conformance scripts must converge behind one family-level
entry point before shell stabilization so a contributor can validate the
common contract and all stable role specializations without discovering a
tool-specific protocol.

## Optional child-launch origin

Capability bit 14 (`launch_origin`) gates two uncounted extension records:
`ProjectionLaunchContext` (`0xff05`, policy to session) and
`SnapshotLaunchOrigin` (`0xff06`, session to policy). Each is 24 bytes:
surface index and generation (u32 each), connection epoch and token (u64 each).
Index zero is valid; generation, epoch and token must be nonzero. Both transfers
reject duplicate subjects, mismatched epochs and more than 1024 records. Their
chunks follow the ordinary counted prefix and do not change its begin/end counts.
Neither direction sends these records unless the capability was negotiated.

Policy publishes a context for each live managed top-level, including hidden
windows. The opaque token denotes immutable logical output and tag membership;
Sophia neither interprets nor edits that destination. Contexts become available
to Sophia only after Engine commits the corresponding projection. They are
session-local, bounded and invalidated when the WM connection epoch changes.
Policy retains old destinations while space permits; an unknown, evicted token
or destination whose output has disappeared falls back to normal placement.
Contexts never enter checkpoints.

At a new X connection, Sophia authenticates the connector using socket peer
credentials and a socket-derived pidfd, captures its process start identity,
and verifies a bounded ancestry chain (at most 64 links). The closest live
ancestor with a managed surface supplies the context. If that process owns
multiple windows, recorded successful keyboard focus selects the most recently
focused one; absent evidence means no origin hint. Missing, unstable, inaccessible
or excessive ancestry also means no hint. The session keeps all process and
namespace evidence private. Only the opaque context crosses into policy.

The context is frozen at connection admission and applies to that process's
first new managed top-level. Switching output or workspace before it maps cannot
rewrite the context. The pending hint survives a rejected candidate and is
consumed only after a committed projection admits the surface. Source teardown
after capture does not erase a frozen hint; WM restart invalidates all unconsumed
hints from the previous epoch. Existing windows are never relocated by a hint.
Single-instance forwarding without a provable new child connection uses normal
placement; this mechanism does not swallow the launching terminal.

Placement precedence is restored placement or valid transient ownership,
explicit registered-launch classification, child origin, then ordinary policy.
Registered classifications require process provenance matching the supervised
launch, so concurrent unrelated surfaces cannot consume them. A child sent to a
background origin does not switch the active output, view or keyboard focus.
Debug admission diagnostics report context availability without application or
process identifiers.

## Output-bound catalog launches

Capability bit 17 (`output_launch_context`) requires `launch_origin` and adds
`ProjectionOutputLaunchContext` (`0xff08`): output ID, output generation, WM
connection epoch and token, four little-endian u64 fields. All are nonzero;
at most sixteen unique outputs are admitted. The complete set replaces the
previous set only when the corresponding policy projection commits. A missing
output has no catalog launch destination; rejected projections publish nothing.

Hagia publishes each output's current view/tag destination even when empty.
Output bookmarks are immutable within the connection epoch, reused for equal
destinations and protected from eviction within the shared 1024-token budget.
Capacity exhaustion omits new destinations rather than recycling old tokens.
Tokens are not checkpointed. Ordinary inherited window bookmarks retain their
existing bounded eviction behavior.

Session validates a dock/menu event against its exact presented content and
captures the committed output bookmark at launch-queue admission, before ACKing
the effect. Shell-content and WM output generations have distinct scopes: the
current logical output joins them, not numerical generation equality. Pending
launches retain their captured token while focus or workspace state changes.
The WM epoch and output generation must still be current before spawning.
Missing capability/context or stale identity refuses before execution; there is
no global-focus fallback. No shell wire change or shell-supplied workspace is
needed.

After exact registered-process attribution, Session echoes the captured token
through `SnapshotLaunchOrigin` for the first managed top-level. It remains
pending through policy rejection until committed admission. Existing placement
precedence and transient-parent behavior remain unchanged. Output removal or WM
restart after spawn may make the origin unavailable; the application survives
under ordinary WM placement, with an explicit degraded-placement diagnostic.
Neither launch admission nor process creation proves committed placement.

## Explicit output actions and stable policy keys

Revision 3 has two optional capabilities, `output_actions` (bit 15) and
`output_policy_keys` (bit 16). Existing messages and ordinary snapshot records
retain their exact layouts. A client that does not negotiate them receives
neither the new message nor the extension.

`OutputActionRequest` (kind 53) carries the target output and its current
output generation independently of `affected_outputs`. Coverage is not focus or
an action target: its order may change during topology reconciliation. Session
captures the target when it admits a published shell action, revalidates it
before dispatch, and never falls back to the active output. The Engine checks
target generation and coverage before issuing the request; normal scene and
transaction validation guards proposal settlement. A removed/replaced target
withdraws that exact queued activation. A WM without the capability cannot
admit these shell actions; ordinary keyboard `Action` requests remain supported.

`SnapshotOutputPolicyKey` (extension kind 65287) binds a nonzero, operator-chosen
key to an exact output/generation. Session resolves configured connector names
locally and exports only the numeric key. The extension follows counted
snapshot chunks and does not change their revision-3 counts. Keys must be
unique; an ambiguous logical mirror is refused rather than assigned the first
connector's identity. This key is policy affinity, not a device identity or an
input grant. Changing established keys requires a new session; hotplug may
change runtime handles without changing the configured keys.

Hagia can assign globally numbered workspaces to these keys. Lom continues to
render labels and return the opaque published action; it does not manufacture
workspace numbers, select another monitor, or interpret connector names.
