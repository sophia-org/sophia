# Shell extension allocations

Authoritative identifier decision, 2026-10-29, following Mason's approval to
resolve W2 allocation. This ledger assigns identifiers; it does not declare a
codec, SDK, host policy or release integrated. Existing measured binaries and
vendored snapshots retain their historical contracts and evidence.

## Assignment

| Shell revision | Shell capability bit | Owner | Status |
| --- | --- | --- | --- |
| 10 | 15 (`OWNED_RING`, `0x8000`) | Owned-ring CPU source | Existing mainline contract |
| 11 | 16 (`DRAW`, `0x10000`) | t341 bounded drawing | Adopt the identifier assignment from the unmerged t341 contract; integration remains deferred |
| 12 | 17 (`CONTENT_RETIRE_ON_PRESENT`, `0x20000`) | W2 predecessor retirement on presentation | Allocated here; production integration pending |
| 13 | 18 (`FRAME_TRANSACTIONS`, `0x40000`) | Sophia Frame Transactions (SFT), native CPU profile | Allocated for default-off integration; codecs, owner/producer integration and qualification pending |
| 13 | 19 (`FRAME_TERMINAL_ONLY`, `0x80000`) | Terminal-only SFT notices; requires bit 18 | Allocated explicit owner/peer opt-in; qualification pending |

Revision 11 / bit 16 remains owned by draw even while t341 is deferred. Its
DrawSettled event kind 54 also remains assigned to draw. Deferral does not make
these identifiers available for reuse. This adopts the branch's assignment;
it does not assert that mainline already implements that branch's protocol.

The parked overview proposal has **no allocation**. The older “currently
r11/bit 16” and t341's “next available candidate is r12/bit17” text are superseded
by this ledger. Revival requires a fresh later allocation. The earlier W3
standing-credit proposal has no independent allocation. The SFT amendment below
assigns r13/bit 18 to the complete frame protocol, not the parked W3 wire sketch.
Capability masks are scoped to their protocol: WM/policy bit 17 for output launch
context is unrelated to shell bit 17 and is not a collision.

## W2 wire assignment

W2 uses the existing whole `Candidate` submission, kind 262. It assigns bit 0 of
the candidate body's u16 at offset 78 (the earlier candidate grammar's reserved word) to “retire intent
present”. With permission, that flag appends exactly three little-endian u64s
**after all surface, placement and target rows**: retire transaction, resource
id, resource generation. All three are nonzero. There is at most one intent.
Unknown flag bits, truncation, trailing bytes and a flagged record without the
explicit grant are refused. Flag zero preserves the old bytes and admits no
trailer. The existing complete-record size cap includes the 24-byte trailer.
The neutral codec omits the outer transaction word; its offsets must not be
confused with the complete file body above.

No new event kind, release reason, file name, image source kind or envelope
version is assigned to W2. CandidateOutcome remains kind 35; ResourceReleased
remains kind 34 and retains its grant/resource/transaction identity and last-lease
release obligation. DrawSettled kind 54 is not used for W2. Shell envelope
`api_version` remains 1. The lifetime/admission contract and shared golden corpus
must accompany production integration; this ledger alone is not that proof.

## Negotiation and exact mappings

The wire grammar is cumulative. A revision never grants a capability. Every
capability use requires an explicit grant and existing role/admission authority.
A required capability cannot disappear: choose a compatible supported revision
inside the client's range, or refuse. An unavailable combination must be refused,
not made available merely because its grammar is known.

These are explicit target rows for integration, not a `>=` rule:

| Selected shell revision | Owned-ring image mapping when bit 15 is granted | Draw vocabulary when bit 16 is granted | W2 intent when bit 17 is granted |
| --- | --- | --- | --- |
| 10 | Image revision 10, source kind 4, existing D4 layouts | Not available | Not available |
| 11 | Image revision 10, source kind 4, existing D4 layouts | t341 draw-v1 and DrawSettled 54 | Not available |
| 12 | Image revision 10, source kind 4, existing D4 layouts | Same t341 draw vocabulary; optional explicit grant | Candidate flag/trailer above |
| 13 | Image revision 10, source kind 4, existing D4 layouts | Existing r11 assignment; optional explicit grant | Existing r12 assignment; optional explicit grant, separate from SFT's own predecessor field |
| Unallocated later revision | No mapping established | No mapping established | No mapping established |

No bit 15 means no owned-ring source, including on a draw-only connection.
Bit 17 does not imply bit 15 or bit 16; the initially qualified W2 host profile
still requires the bar's content grant and owned-ring capability separately.
The first production W2 profile must refuse bit 17 without bit 15 until a W2
lifecycle without owned-ring is separately qualified.
Other W2 roles, sources or draw+W2 combinations require their own admission and
lifecycle qualification before being offered. Implementations may refuse those
combinations meanwhile. A W2-only r12 grant does not disclose draw-v1 or accept
DrawSettled. r12 grammar must not erase the r11 draw vocabulary when draw is
explicitly supported and granted.

Image mapping, shell envelope framing and record availability are separate.
Do not generalize image/header checks to `>= 10`, treat selected shell revision
as image revision, or widen unrelated r9 ImageBegin/ResourceStatus/Submitted
rules by raising a decoder's maximum. Each codec/record family needs an exact
revision-and-grant table with refusal cases. Retired sealed ingress stays retired;
DMA retains its independent grant, availability and synchronization contract.

## Evidence and integration gates

The measured W2 SDK `1951d597` and host `a6f16af50` are experimental predecessors,
not a conforming cumulative r12 implementation. Their image helper excludes r11,
the low-level intent helper accepts `>= 12`, and live negotiation chooses exactly
r10 or r12 and refuses bit 16 as unknown. Preserve those trees, releases and packages. Do not
rename their measurements as production-r12 qualification after this assignment.

The inspected t341 heads were Sophia `06ffcd007` (qualification) and `c3c591591`
(vello-draw), with Rust SDK `7b62524` and C SDK `2c4d1bc`. These local branches
remain unmerged; their old informal overview pointers must be replaced when
forward-integrated. No branch's stale “next available” prose overrides this ledger.

Remaining production work, in order:

1. Use fresh integration branches; forward-integrate the current SDK baselines
   and reconcile t341/W2 using the explicit rows above. Keep measured branches
   untouched. An implementation need not enable an unavailable combination.
2. Complete the authoritative KDL/prose lifecycle amendment, C/Rust APIs and
   byte-identical golden cases together. Replace stale overview pointers with
   “fresh later allocation”, including current master
   `docs/sophia-shell-files.md:1219` and t341
   `docs/sophia-shell-files.md:1333` at the inspected revisions. Test r10/r11/r12,
   required/granted masks, unknown
   revisions/flags, both handshakes, draw coexistence/refusal, image mappings,
   release ordering, cancellation, revocation and replacement grants.
3. Publish signed, reviewed SDK commits to the intended upstream so both pins
   are reachable, then vendor through the existing xtasks and run both contract
   checks and independent-client tests. Publication is a separate action.
4. Review host integration and qualification before any release/default change.
   Allocation alone changes no feature flag, owner permission or desktop profile.

This ledger is separate from the SDK-mirrored byte contracts so the allocation
can be settled without editing immutable vendor archives or making their current
contract checks inconsistent. During coordinated SDK integration, carry this
assignment into the authoritative byte contracts and their checked SDK copies.

## SFT assignment, November 4

The approved native frame architecture owns r13 / bit 18. Its exact initial
grammar is [sophia-frame-transactions-v1.kdl](sophia-frame-transactions-v1.kdl),
with lifecycle/admission rules in [the SFT wire contract](../docs/sophia-frame-transactions-wire.md).
Candidate kinds 279/280/281 are FrameStreamOpen, FrameStreamClose and FrameCommit;
event kinds 55/56/57/58 are FrameStreamOpened, FrameStreamClosed, FrameRefused and
FrameOutcome. DrawSettled 54 is untouched. Shell envelope api_version remains 1;
there is no new transport or source kind.

SFT requires explicit bit 18 and owned-ring bit 15 with the existing content-role
authority, selecting exactly r13. It grants neither draw nor W2. Its integrated
predecessor intent reuses W2 ownership semantics, but does not authorize a W2
trailer on ordinary Candidate. An unsupported combination refuses. Neither
knowing r13 grammar nor accepting a required range authorizes silently dropping
a required capability. First integration remains default-off; this assignment
alone changes no offered revision, client default, SDK pin or launch profile.

The [CPU critical path](../docs/notes/plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
tracks implementation order. The frozen W2 note is
`cpu/w2-prototype:docs/notes/investigations/qyktai1j-w2-retire-the-predecessor-on-presentation.md`;
its historic “not an allocation” labels remain correct for those experiments.

## Terminal-only SFT capability, November 6

Shell capability bit 19 (`FRAME_TERMINAL_ONLY`, `0x80000`) is allocated as an
additive, explicitly required option at exactly r13. It requires bit 18 and
all existing SFT prerequisites; it is not a new revision or record layout.
The owner must separately permit this option for this authenticated SFT handshake. A request
without bit 19 retains Prepared notices. A host without this option refuses
the required bit, rather than accepting it and silently keeping or removing
notices. An unsolicited bit 19 grant is invalid.

Only the informational Prepared FrameOutcome is omitted. Internal preparation,
its deadline, native completion, terminal credit and independent source and
predecessor releases are unchanged. Admission initially reserves the same two
candidate response records. Preparation consumes its obligation without a
queued record or frame-credit return. See the terminal-only section of the
SFT wire contract. Allocation does not enable SFT by default or qualify CPU
savings, latency or installed-desktop behavior.
