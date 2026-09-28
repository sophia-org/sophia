---
id: 4oapm903
date: 2026-09-28
kind: adr
status: proposed
tags: [adr, shell, protocol]
---
# Carry descriptor families as native shell file records

## Context and proposed decision

The existing descriptor socket carries functions that the content file roles
do not replace: sanitized window descriptors, tab groups, shortcut catalogs,
reference sheets, the descriptor launcher and work-area reservations. Retiring
the socket must preserve those functions. Use whole typed objects and native
candidates/events within the existing shell 9P export, with a separately
admitted descriptor role and the same authority owners.

This is a design proposal reviewed against Sophia `6403c80e9`. It is not yet
served or vendored into either SDK, and does not authorize removing a socket
test. The <a href="4oapm903-descriptor-layout-proposal.kdl">proposed KDL
fragment</a> fixes every byte offset. It is not an extension of the currently
implemented <a href="../../../protocol/sophia-shell-files-v1.kdl">shell file
contract</a>. Adoption requires the implementation and evidence below.

## Alternatives

- Dropping descriptor clients would remove existing features and is outside
  the requested migration.
- Wrapping socket frames in files would preserve the dependency being retired
  and leave stream-fragment assembly inside an immutable-object interface.
- Forbidding combined descriptor/content grants would simplify the adapter
  but remove an already supported, explicitly authorized combination.
- A separate descriptor SDK would duplicate transport and custody machinery.
  Both language SDKs instead expose this role alongside the existing roles.

## Consequences

The file adapter needs native codecs, feed accounting, typed owner intake and
terminal-response credits for every family. These are additions to transport;
they do not move disclosure, activation, presentation or work-area authority.
The current socket remains until equivalent file coverage and client migration
are established. The proposed byte layouts deliberately remove transfer
fragment headers, so they require independent vectors rather than renamed
socket tests.

## Scope and identity

Preserve descriptor switching and work-area reservations, tabs, shortcuts,
reference sheets and the revision-4 launcher. Preserve a combined descriptor
and content client when Session explicitly grants both. The baseline product
is Narthex master 50b9014d96f675f515b5e092c071427fb8e34423; the unmerged
overview branch is outside this contract. SDK APIs and conformance peers remain
generic. Narthex consumes the C SDK through thin Nim bindings; no second Nim
protocol implementation is introduced. Both language SDKs receive the role.

Session assigns `role=descriptor` before negotiation. A client cannot acquire
that role by a capability bit, file name, qid, attach name or body field.
Bit 0 on an ordinary content bar remains inert for descriptor disclosure.
The new fixed nodes use distinct qid indices 15/16/17; the implementation's
reserved node span becomes 32. Numeric qid values are not client ABI.

Object kinds: Descriptors=5, Tabs=6, Shortcuts=7. Event kinds 46–53 and
candidate kinds 273–278 are as listed in the KDL. `Submitted.candidate_kind`
and `ObjectPublished.object_kind` must accept the new declared values; old
record layouts remain unchanged. No IPC frame or transfer fragment is a file
body. Tab groups precede a contiguous descriptor array, partitioned in group
order by each group's entry_count. There are no repeated per-entry group,
output, snapshot or transfer identities. Group order is meaningful.

## Admission, disclosure and readiness

The role uses the existing revision bounds and capability dependencies from
the shell negotiation policy. Negotiate has minimum_revision,
maximum_revision and required_capabilities; there is no separate client offer
mask. Select min(maximum_revision, 8), refusing zero/descending bounds or a
minimum above 8. Require descriptor bit 0. Unsupported required bits refuse
negotiation; a revision number alone does not select an optional family.

| Bit | Minimum revision | Selection for this role | Dependency |
| --- | --- | --- | --- |
| 0 descriptor switcher | 1 | Required; always selected | Session descriptor admission |
| 1 work-area reservation | 1 | Always selected, as in the existing owner | Descriptor admission; individual reservations still obey the profile limit |
| 2 tab groups | 2 | Only when requested | Bit 0 |
| 3 shortcut catalog | 3 | Only when requested | Bit 0 |
| 4 reference sheet | 3 | Only when requested | Bit 3 |
| 5 application catalog | 4 | Only when requested | Descriptor admission authorizes this feed |
| 6 application launcher | 4 | Only when requested | Bit 5 |
| 7 content surface | 5 | Only when requested and content policy grants it | An admitted registry grant |
| 8 discrete content input | 5 | Only when requested and input policy grants it | Bit 7 |
| 9 view indicators | 6 | Only when requested | Existing descriptor negotiation permits this without a content request |
| 10 indicator activation | 6 | Only when requested | Bit 9 |
| 11 native launcher, 12 persistent catalog | — | Refused for descriptor role | Separate pre-admitted component profiles |

This table follows `shell_transport/negotiation_policy.rs` at `6403c80e9`.
Do not impose an exact required mask of bits 0 and 1: r1 clients historically
request only bit 0 and receive both. Content denial preserves refusal reason
1 for policy denial and 4 for unavailable implementation or registry budget;
denied_capabilities is the requested content mask, or only bit 8 when input
alone is denied. A pre-reserved content grant without a content request
refuses, as in the current owner. The new role marker separates admission
from the content bar's inert bit 0; it does not tighten valid metadata requests.

Without explicit content admission, Negotiated has limits_published=0. The
client becomes ready after its bootstrap custody and negotiation are consumed;
it must not await a Limits object that will never exist. Limits, outputs and
upload nodes and content submissions remain inaccessible. Negotiate (256)
is the bootstrap candidate, not a forbidden content request.

With explicit content policy and a successfully reserved ContentEpochRegistry
grant, the combined role publishes Limits and exposes only the selected
content families, plus authorized descriptor feeds. Readiness then waits for
the complete valid Limits object as on existing content roles. Resource stores,
discrete-input permission and every content credit still belong to their normal
owners. Descriptor capability alone cannot allocate or read content. The API
and Negotiated record must agree on the selected profile; no wire sniffing or
fallback is allowed.

Descriptor feed is present with bit 0; tabs with bit 2; shortcuts with bit 3
(required by reference sheets); catalog with bit 5, even without bit 6;
indicators with bit 9, even without bit 7. Indicator reads and activation
remain tied to their actual capability checks, not the presence of Limits.
The catalog reuses the plain whole Catalog object, not r8 identity disclosure.
Absent feeds refuse walks. Publishing any feed follows the existing fit,
new-qid, immutable-pin, announcement and acknowledgement-hold rules.

## Values and validation

All fields are little endian. Boolean fields are u16 0/1. Reserved fields and
padding bytes are zero. Text uses u16 byte length, reserved u16 zero, then a
fixed-capacity UTF-8 area with zero bytes after the declared length. Exact
record size is required: neither truncation nor trailing bytes is accepted.
Body connection_epoch must equal the nonzero envelope epoch. Transactions and
identity fields marked nonzero in the KDL are nonzero.

Text character rules are deliberately family-specific. Descriptor labels,
when present, are nonempty, at most 128 bytes, and contain no Unicode control
characters; bidi override/isolate characters are not newly prohibited here.
This preserves the existing descriptor validator. Shortcut, reference and
launcher text additionally refuses U+202A–U+202E and U+2066–U+2069, matching
their existing validators. A launcher query may be empty. Optional shortcut
label/group have explicit presence flags; absent means empty padded storage,
present means nonempty valid text. None is preserved rather than manufactured
as an empty Some. The old shortcut validator already refuses Some("").

DescriptorEntry's label_present=false requires label_redacted=false and empty
text. Present labels retain redacted state. Trust values are 0 unknown,
1 trusted, 2 untrusted, 3 isolated; attention is 0 none, 1 notice, 2 critical.
Descriptor slots and generations are nonzero; slots are distinct within a
snapshot. Different slots may have the same generation.
Each action token and epoch is nonzero; target_slot equals the descriptor slot,
target_generation equals its generation, recipient_epoch equals the connection,
and issuer epochs match the Descriptors prefix. Tabs may carry distinct broker
issuers across entries; their action identities remain per entry. No raw
application surface identity is disclosed.

Tab group slots are nonzero and distinct; output IDs are nonzero. Descriptor
slots are distinct across the entire tab snapshot. selected_slot=0 precisely
when a group is empty; otherwise it names one entry of that group. focused is
a boolean, not an extra authority check. The sum of group entry counts equals
the prefix count, at most 2048, with at most 1024 groups. No 16-entry per-group
limit is added: the old codec imposed 16 only on standalone snapshots.

DescriptorCandidate entry slots are nonzero and distinct. Their generations
are nonzero but may repeat for different slots, as in the source snapshot.
visible is true precisely when entries are nonempty; a visible candidate's
selected_slot names an entry. Hidden candidates have no entries,
selected_slot=0 and no reservation. Edge 0
requires thickness 0; edges 1 top, 2 bottom, 3 left, 4 right require thickness
1..512. The owner separately checks its profile's reservation limit.

TabsCandidate group slots are nonzero and distinct. Its exact ordered group
list, snapshot identity and rising candidate generation are owner checks.

ReferenceCandidate: columns 1..4; body size 8..32; title size 8..48; padding,
key gap and column gap <=64; row gap <=32; border <=16; margin <=128. The title,
keys and labels are nonempty valid text. Entry slots are nonzero and distinct.
All colors except background have alpha 255. A page number is a request to the
owner's projection; the codec does not impose a guessed page count.

LauncherCandidate retains <=32 distinct nonzero slots <=4096, selected=0 or
one of those slots, font size 10..32 and opaque colors except background.
Do not add the descriptor candidate's visibility/selection equivalence here:
the launcher validator does not impose it.

Outcome kinds are 1 Prepared, 2 Presented, 3 Rejected, 4 Superseded. All
Presented outcomes require a nonzero presentation epoch. DescriptorOutcome
requires zero epoch for every other outcome. ReferenceOutcome and
LauncherOutcome retain their looser existing rule for non-Presented epochs;
writers normally emit zero, but the codec must not invent a tighter rule.
ReferenceOutcome requires pages>0 and page<pages. Descriptor activation
dispositions are 1 Consumed, 2 RejectedStale. LauncherActivation and echoed
ack/outcome identities are exact, nonzero, with slot 1..4096; consumed is 0/1
and launch status is 1 Started, 2 Rejected, 3 Failed.

## Custody, outcomes and work-area commit

Structural errors return EINVAL before Submitted. Wrong connection authority
or epoch is refused before custody and cannot affect another owner. An absent
capability is EACCES. Capacity refusal is EAGAIN before changing owner state;
the established same-ID retry and acknowledgement ordering rules apply.

After Submitted, stale semantic work is answered by its family, rather than
being treated as malformed bytes. The following owner outcomes are the chosen
file semantics; places where the socket adapter instead returned a transport
error must gain explicit regression tests when callers are converted:

| Input | Semantic refusal after custody | Result |
| --- | --- | --- |
| DescriptorCandidate | no matching request; stale snapshot/output or non-rising generation; a slot/generation absent from the requested snapshot; reservation beyond profile | DescriptorOutcome Rejected; no work-area or visual mutation |
| TabsCandidate | stale/missing transaction or snapshot, wrong ordered groups, non-rising generation or generation using the reserved high bit | DescriptorOutcome Superseded, preserving existing tab behavior |
| ReferenceCandidate | cancelled request | ReferenceOutcome Superseded, no presentation |
| ReferenceCandidate | stale/missing request, catalog or output; non-rising generation; invalid owner projection | ReferenceOutcome Rejected, no presentation |
| LauncherCandidate | cancelled or stale request/catalog/output/generation | LauncherOutcome Superseded, preserving the request's retirement |
| Activation acknowledgements | stale/unknown activation or mismatched echoed grant | no action or launch; consume custody only and record unmatched input |

Acknowledgements never create activation authority. A matching descriptor ack
continues through the existing broker capability check. A matching launcher
ack continues through the existing reserved launch-queue slot; no action is
replayed after reconnect. Wrong authority is always refused, even if a stale
candidate could otherwise receive a benign outcome.

Prepared is not Presented. Reservation admission and Engine's presentation
commit remain distinct; the visible descriptor and resulting work area commit
together. A withdrawn reservation is a later candidate through the same path.
Submitted proves custody only. Add an independent C SDK peer control that
reads the work area before preparation, after Prepared and after actual commit;
it must remain unchanged until the final step. Test supersession, withdrawal,
stale activation and reconnect as well as the normal sequence.

## Bounds

Full record maxima follow from the attached layouts. Tab candidates use 8260
bytes; reference candidates use 52488. These are explicit per-kind maxima,
not a raised common 8192-byte content candidate cap. Both fit the existing
65536-byte transaction buffer. Whole object caps including their header are
4096 descriptors, 1048576 tabs and 131072 shortcuts. At maximum cardinality
their actual full sizes are 3232, 426048 and 104512 bytes respectively.

Metadata-only descriptor journal: 256 records, 131072 bytes; largest record
LauncherRequest=352 bytes, terminal reserve 64 records / 22528 bytes. This is
a conservative reserve: requests can be larger than terminal outcomes. The
snapshot budget is twice the sum of disclosed feed caps plus one 4 MiB build
scratch. With descriptors, tabs and shortcuts it is 6561792; adding catalog
makes 14950400. Indicators, when selected, add another 65536 (two 32768-byte
objects): the maxima become 6627328 without catalog and 15015936 with it.
Derive the charge from the selected feeds, rather than charging an undisclosed
feed or omitting a disclosed feed's retention. This is snapshot custody only,
not a total connection-memory limit; journal, transaction staging, queued typed
records and any content stores have separate finite charges.

A combined descriptor/content grant charges the union of feeds and the
normal content registry/scratch, not just the metadata estimate. The descriptor
role's largest event remains LauncherRequest (352 bytes); the base content
AllocationResult is 200 and Action is 152. NativeInput belongs to the separate
native-launcher role, whose 430-byte event budget is unchanged; it is not
selected by a combined descriptor/content grant. A new profile must not shrink
existing owner resource, activation or terminal response limits to fit the adapter.

Every accepted candidate must reserve its terminal response capacity before
mutation. Descriptor/tab/reference/launcher publication must reserve enough
credit for Prepared and its one terminal outcome as appropriate. Existing
activation, request, presentation and launch deadlines remain bounded and
unchanged. File acknowledgement releases retention only; pin holds protect
unfetched objects. An unacknowledged Submitted prevents the next transaction
open. There is no implicit acknowledgement or object-fetch skip.

## Required evidence before retirement

Both SDKs: strict codecs, all family boundaries, malformed and literal vectors,
metadata readiness without Limits and combined readiness with them, refusal
of content methods without a grant, object pins/ack holds, partial reads and
bounded custody. C independently encodes the same contract; the conformance
peer uses that SDK, not a second ad hoc C protocol stack.

Sophia: typed in/out owners on both paths during migration; no frame encoding
inside the file adapter; production export tests, reservation-at-commit with
the independent C SDK peer, complete host proof/serve/bar-proof, loss and
supersession, bounded journal pressure and quiescence. Literal malformed
peer writes remain useful controls. Rust using the same SDK as the server
is not independent encoding evidence.

Then migrate Narthex through the C ABI and gate every current r1–r8 family;
preserve its policy behavior. Only after that remove --shell-process and its
default launcher plumbing, descriptor IPC and the compatibility tests. No
current green content gate substitutes for these descriptor exits.

## Review evidence and acceptance

The layout fragment was parsed with the repository's KDL library and checked
independently of its disposable drafting script: 17 kind declarations,
24 bodies/prefixes/rows, contiguous field offsets, exact sizes, unique kinds,
full-record maxima and transaction/object caps. Mutating a field offset,
reducing the reference candidate cap to 8192 or duplicating a kind each failed
the checker. Evidence is in `ipc-retirement/t271-kdl-layout-check.log` and
`t271-kdl-layout-mutants.log` under the development-evidence directory.

This validates syntax and arithmetic only. No descriptor file codec, SDK
session, production export or independent peer has passed this proposal yet.
The capability table was read against `shell_transport/negotiation_policy.rs`;
the bounds and text rules were compared with the existing packet types and
socket validators. Acceptance must also resolve the chosen semantic refusals
listed above with regressions through the real owners. Task status remains in
`todo.md`; the full exit is t271, not this document's publication.

Related records:

- [IPC retirement inventory](../investigations/1lty2tzb-what-ipc-code-remains-after-the-desktop-moved-to-9p2000-l.md)
  owns the migration's evidence and task mapping.
- [Shell file contract](../../sophia-shell-files.md) remains authoritative
  for the implemented export.
- [9P desktop role plan](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md)
  owns the broader acceptance criteria.
