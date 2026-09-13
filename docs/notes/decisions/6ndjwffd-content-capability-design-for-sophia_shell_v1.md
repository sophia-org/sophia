---
id: 6ndjwffd
date: 2026-09-12
kind: adr
status: proposed
tags: [adr, shell, protocol, content, graphics]
---
# Content capability design for sophia_shell_v1

Execution amendment, accepted 2026-09-13:
[Separate shell presentation from GPU execution permission](mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md)
removes the mandatory hard GPU-quota/kernel prerequisite. It retains this CPU
wire, complete-candidate lifecycle and finite content budgets. It grants no
implicit device access and changes no record layouts. The context below records
the original proposal; the [implementation record](../../lom-content-implementation.md)
is the source for current coverage and remaining gates.

## Context

[The content shell proposal](../../content-shell.md) describes a shell that owns
pixels rather than supplying descriptors. It is unimplemented, assigns no wire
types, and closes with a gate:

> Before implementation, a separately admitted design must select transport,
> numeric budgets, pixel semantics, capability assignments, wire records, release
> and pacing messages, and a conformance corpus. The lifecycle and authority
> invariants must be modeled and checked under the project's evidence policy.

This is that design. It is a selection, not an implementation: no kind is
allocated in a checked-in schema and no capability bit is reserved until this
record is admitted and the implementation lands.

The immediate motivation is that nothing admitted can draw. Narthex owns no
pixels by construction, and the Quickshell panel is an X11 fixture that
[does not acquire shell_v1 authority](../../quickshell-x11-panel.md). On two
outputs, focus can walk onto an empty DP-2 with nothing on screen to say so.

The original driving client was an ironbar fork in `sophia-org`. The operator
subsequently selected Lom, an Xilem/Masonry/Vello client preserving the
Ironbar-inspired workflow, with GPU readback into this CPU-byte contract.
The [implementation record](../../lom-content-implementation.md) distinguishes
implemented boundaries from remaining GPU admission and native gates.

## Decision

### 1. Transport — bounded byte frames, no fd passing

Content is transferred as **immutable CPU pixel resources in bounded byte chunks
over the existing authenticated shell-role Unix stream**. No SCM_RIGHTS, no
shared mappings, no DMA-BUF, no second socket, no private Engine endpoint.

`content-shell.md` already forecloses the alternatives for a first experiment:
shared memory, fd passing and DMA-BUF "remain candidates requiring measured need
and a separate specification for ownership, synchronization, format, damage,
lifetime, fallback, and renderer failure."

This is also the cheap path. `crates/sophia-runtime/src/shell_transport.rs` is a
plain `UnixStream` with byte frames and has no ancillary-data path at all, so fd
passing would be a genuine transport change rather than a message addition.

Engine owns its accepted copy. Client staging bytes are reusable once written
into owned transport storage, so this contract needs no acquire/idle fence — and
claims no zero-copy property. Upload and copy costs are to be measured, not
assumed.

### 2. Capability assignment and negotiation

| Bit | Name | Meaning |
| --- | --- | --- |
| **7** | `content_surface` | The indivisible content lifecycle: resources, allocation, candidates, release, pacing. |
| **8** | `content_discrete_input` | Discrete actions and outside dismissal. **Requires bit 7.** |
| 1 | `work_area_reservation` | *Existing.* Additionally required for a **nonzero** reservation. |
| 0 | `descriptor_switcher` | *Existing.* Remains required for this first combined client. |

Revision **5**. Bits 0–6 are taken, so content starts at 7. No effect, keyboard,
or host-service bit is assigned.

Effective content permission is *implementation support* AND *client request* AND
*operator grant*. Holding descriptor capabilities is not evidence of content
permission; the default is no content permission.

**Anchoring is not reservation.** A panel may be edge-anchored without reserving
work area. `edge` stays explicit for a non-reserving panel, and
`reservation_extent = 0` means no reservation rather than no placement. Only a
nonzero extent requires bit 1, reuses the existing 512 px ceiling, and must also
satisfy any tighter profile bound.

`ClientHello` (96) and `ServerWelcome` (97) stay **byte-for-byte unchanged**. The
existing Hello carries only `required_capabilities`, so a client chooses its
workflow before Hello; unavailable permission is an explicit refusal, never a
silent downgrade. A descriptor fallback is a fresh connection with a
descriptor-only Hello. There is no host or X11 fallback.

A successful content welcome is followed by `ContentLimits` **before** any other
content record. Limits are immutable for that grant. Revoking or replacing a
grant closes the admitted epoch rather than mutating live limits, and does not
wait for a peer acknowledgement or a writable socket.

### 3. Kind allocation — 160–180

Kinds are **one shared namespace**, not per-family. The audit:

| Range | Owner |
| --- | --- |
| 3–8, 32–52, 64–68, 80–83 | portal, `wm_v1`, others |
| 96–122 | `shell_v1` r1–r4, dense |
| 123–127 | free — only five kinds |
| 128–134 | `control_v1`, in the schema and guarded at `ipc/control_v1.rs:181`, **absent from `IpcMessageKind`** |
| 135–159 | free |
| 65280–65535 | `wm_v1` extension records |

Content takes **160–180**, twenty-one kinds. 123 is not taken but is unusable:
content needs about twenty kinds and only five are free before `control_v1`.
Starting at 160 leaves 135–159 as growth room for `control_v1`, the family that
would otherwise be boxed in by its neighbours.

The `0xFF00` extension-record range is **not** a convention to follow here. It
exists because `wm_v1`'s counted-record path is *frozen*
(`protocol/sophia-wm-v1.kdl:118`), and uncounted appended chunks are how a frozen
revision still carries new facts. `shell_v1` is not frozen; it allocates ordinary
dense `message kind=N since=N gate=…` records, as tabs, reference and launcher
all did. Content follows that.

### 4. Pixel semantics

**One format.** Tightly packed `B, G, R, A`, premultiplied, sRGB non-linear, no
ICC, no padding, no compression, no deltas. One wire `pixel_format = 1` names
that exact contract and every other value is rejected. Premultiplied because that
is what the compositor blends with, so nothing is reconverted between acceptance
and presentation; one format because the corpus then has one golden encoding and
Engine has one upload path. Adding a format is a later revision, not a runtime
negotiation.

**Implicit stride.** Stride is exactly `width * 4`, and
`total_bytes == width * height * 4` is validated with checked arithmetic before
any allocation. The client does not choose stride: a client-supplied stride is a
second length that can disagree with width — a whole class of malformed frames to
specify, test and reject — and with a fixed 4-byte pixel there is no padding term.

**Resources declare their scale.** A resource carries a reduced nonzero
`rendered_scale` rational, which must match the allocation's granted scale when
referenced. Dimensions alone do not carry scale.

**Device pixels, and rejection rather than resampling.** Resource dimensions are
physical device pixels. A candidate binds the output's allocation and scale
generations; if those are stale the candidate is **rejected**, not resampled.
Engine retains the last coherent presentation, which it is already permitted to
do, and the shell re-renders. Resampling a bar makes text blurry and, worse,
would pair new pixel geometry with input targets computed at the old scale —
exactly the prohibited mixing of new pixels with old target meaning.

**Two coordinate spaces, never aliased.**

| Space | Used by | Unit |
| --- | --- | --- |
| Shell-local logical | `ContentAllocationRequest` desired extents and margins | Logical units the shell proposes |
| Allocation-local physical | Placement destinations, target bounds | Device pixels within the granted allocation |

Engine resolves the first into the second and **publishes exact pixel extents**.
The guarantee this buys is narrow and worth stating exactly: **the shell never
has to guess its raster size.** It is *not* a claim that no rounding occurs in
the client. A client laying out in logical units — ironbar and GTK both do —
still converts its own widget geometry, and at an admitted scale of 5/4 a target
one physical pixel from the origin sits at 4/5 of a logical unit, which an
integer logical rectangle cannot name. No fractional coordinate is ever encoded;
that is a different and weaker statement than no rounding.

The anchor path is therefore specified so the client is never asked to
reconstruct an integer logical rectangle from a physical position. **A popout's
anchor is a `RectP` in parent-allocation-local physical pixels**, bound by the
existing `parent:A` and `parent_presentation_epoch` fields. That is the only
anchor form in r5: a target-identity alternative was considered and dropped
because carrying both would need a tagged union and a second identity payload for
no benefit the first workflow can use. Engine performs every physical-to-placement
conversion. Logical desired extents and margins remain for the panel, where no
physical position is being named, and a panel or a release request encodes a zero
anchor.

Engine's quantization of logical proposals is normative, so client and Engine
never disagree. Rectangles are half-open and quantize **outward from their
endpoints**, never by scaling the extent:

```
pixel_start  = floor(logical_start * n / d)
pixel_end    = ceil (logical_end   * n / d)
pixel_extent = pixel_end - pixel_start
```

Scaling the extent instead of the end under-covers: logical `[1, 5)` at 5/4 wants
`[1.25, 6.25)`, and `floor(1x5/4)` with `ceil(4x5/4)` yields `[1, 6)`, losing the
last quarter-pixel. The rule above yields `[1, 7)`. Arithmetic is checked and
widened, with mathematical floor and ceil for signed coordinates. A scalar
reservation thickness anchored at an exact integer edge keeps its `ceil` rule;
right- and bottom-anchored rectangles derive from their edge endpoints by the
same formula. A negative margin applies the rule to its magnitude and negates. Placement and target validation use the **acknowledged** allocation
transform and generation, never a newer scale that arrived after presentation.
Conformance must include the `[1, 5)` at 5/4 case above, a one-physical-pixel
target at 5/4, and negative-margin edge cases at 3/2 and 7/4.

r5 admits reduced scale rationals with denominator ≤ 4, and a distinct record
type is used for each space rather than one ambiguous rectangle alias.

**Tiles: a complete ordered placement list.** A **resource** is an immutable
rectangle `(id, generation, width, height, rendered_scale, bytes)`. A **surface**
is an allocation-level row carrying role, edge, margins, anchor and reservation.
A **placement** binds one resource to a destination offset within one surface.

Two rules make tiles safe, and they are why this model is chosen over one
complete resource per surface:

1. **Deterministic composition order.** The placement list is ordered; later
   placements compose over earlier ones. There is no implicit z-order to infer.
2. **A transparent-black base.** Each allocation is rebuilt every candidate from
   transparent black plus the complete ordered list. An omitted tile therefore
   *disappears* rather than silently revealing an older image — so a partial list
   can never act as an undeclared delta.

This keeps static content genuinely reusable: a 2560x32 panel background uploads
once as six chunks, and a 120x32 clock tile that ticks costs one 15360-byte chunk
instead of re-uploading 320 KiB. Distinct placements may reuse one resource, and
every reference is validated and pinned. Deltas remain out of scope, since a
delta would have to name and validate a base generation and an ordered complete
list does not.

**Row-aligned chunking.** `usable_payload = min(max_chunk_bytes,
max_frame_payload - 48)` = 65488. Then `row_capacity = floor(usable_payload /
(width * 4))`; every non-final chunk carries exactly `row_capacity` rows and the
final chunk the remaining rows, so `chunk_count = ceil(height / row_capacity)`.
Fixing the shape this way stops arbitrary one-row fragmentation from defeating a
count bound. `offset` and `byte_count` must both be multiples of `width * 4`.

Worked cases, which are also exact corpus arithmetic:

| Resource | Row bytes | Rows/chunk | Chunks |
| --- | --- | --- | --- |
| 2560x32 panel | 10240 | 6 | 6 — five of 61440 bytes, one of 20480 |
| 120x32 clock tile | 480 | 32 (capacity 136) | 1 of 15360 bytes |
| 8192 wide | 32768 | 1 | the 4 MiB cap allows at most 128 rows |

A profile may not advertise `max_width_px = 8192` while lowering
`max_chunk_bytes` below 32768; one maximum-width row must always fit one chunk.

### 5. Numeric budgets

All advertised in `ContentLimits` and immutable for the grant. They are **joint**
constraints: 8192 x 4096 is not admissible because `max_resource_bytes` caps the
rectangle at 4 MiB, and the corpus must test both that corner rejection and
exact-cap acceptance.

| Field | Value | Basis |
| --- | --- | --- |
| `max_frame_payload` | 65536 | Existing `SOPHIA_IPC_MAX_PAYLOAD_LEN`. |
| `max_chunk_bytes` | 65488 | Frame cap less the 48-byte chunk prefix. |
| `max_width_px` | 8192 | One row is 32 KiB and always fits one chunk. |
| `max_height_px` | 4096 | Bounds chunk count per transfer. |
| `max_resource_bytes` | 4 MiB | Panel 320 KiB, popout 469 KiB. A storage bound only — it is **not** what excludes full-screen content. |
| `max_staging_bytes` | 8 MiB | Charged while transfers are incomplete. Four open transfers cannot all reserve 4 MiB; that is a deliberate joint constraint. |
| `max_resident_bytes` | 16 MiB | Accepted content. Bytes move from staging to resident on acceptance and are never charged to both. |
| `max_retiring_bytes` | 16 MiB | Per connection, for generations whose references are still draining. |
| `max_session_retiring_bytes` | 64 MiB | Session-global across dead epochs, so repeated reconnects cannot grow process memory while a renderer holds old frames. |
| `max_live_resources` | 64 | Simultaneously live **generations**; old and new coexist while references drain. |
| `max_resource_ids` | 4096 | High-water replay state, a separate table. Never evicted. |
| `max_open_transfers` | 4 | |
| `max_outputs` | 16 | Matches existing `SOPHIA_SHELL_MAX_DESCRIPTORS`; `ContentOutputFacts` cannot span frames. |
| `max_allocations` | 16 total, 4 per output | Encoded as two separate fields; one number cannot express both. |
| `max_pending_allocation_requests` | 8 | |
| `max_candidate_surfaces` | 8 | Per candidate. |
| `max_candidate_placements` | 32 | Per candidate. |
| `max_candidate_targets` | 64 | A bar carries many buttons. |
| `max_candidate_bytes` | 8192 | **Data only**, excluding Begin, End and frame headers. The structural worst case `40 + 64x8 + 32x32 + 48x64` = 4648 bytes is an upper bound that is not jointly attainable, since `max_allocations` permits only 4 surfaces on one output. One data chunk always suffices in r5. |
| `max_open_candidates` | 2 total, 1 per output | **Assembly in progress**: Begin sent, End not yet. Encoded as two fields, like every other total/per-output pair. Distinct from the row below. |
| `max_pending_candidates` | 8 global, 1 per output | **Accepted but not yet submitted.** The phase a new candidate may supersede. |
| `max_pending_actions` | 16 | Matches existing `SOPHIA_SHELL_MAX_PENDING_ACTIVATIONS`. |
| `max_frame_demands` | 1 standing demand per output | Plus at most one granted-but-unconsumed permit per output, which is the second slot an earlier draft conflated with a second demand. |
| `max_control_records` | 64 | |
| `reserved_control_queue_bytes` | 65536 | Bulk chunks cannot consume outcome, release or input capacity. |
| `max_input_queue_bytes` | 131072 | Whole frames including headers. A single legal frame can reach 65560 bytes (65488 data + 48 prefix + 24 header), so the old 65536 could not hold one. | |
| `max_output_queue_bytes` | 262144 | Whole frames including headers. |
| `max_frames_per_service_tick` | 16 | Input and revocation stay serviceable during bulk transfer. |
| `max_panels_per_output` | 1 | Multiple panel strips are not admitted in r5. |
| `max_popouts_per_output` | 3 | |
| `max_panel_extent` | 512 | Thickness of a panel along its anchored axis. |
| `max_popout_extent_px` | 1024 per axis | A popout was otherwise unbounded; "anchored to a parent" does not stop an output-sized rectangle. |
| `max_content_coverage_percent` | 50 | Checked on **resolved** allocations, summed across the output. |
| `max_reservation_extent` | 512 | Existing `SOPHIA_SHELL_MAX_RESERVATION_THICKNESS_PX`. Must be ≤ the panel extent. |
| `max_margin_logical` | 512 | An unsigned magnitude denoting the admitted range −512..+512 in **logical** units, matching what the request actually carries. `i16` representability is not authorization. |
| `max_scale_numerator` / `max_scale_denominator` | 32 / 4 | Reduced rationals, denominator ≤ 4. |
| `allocation_timeout_ms` | 1000 | |
| `transfer_timeout_ms` | 2000 | Absolute Begin-to-terminal, on the owner's monotonic clock. |
| `transfer_idle_timeout_ms` | 500 | Defeats slow-drip retention. |
| `candidate_timeout_ms` | 1000 | Begin to End. |
| `preparation_timeout_ms` | 1000 | |
| `presentation_timeout_ms` | 2000 | |
| `action_ack_timeout_ms` | 1000 | |
| `permit_timeout_ms` | 250 | Admission freshness, not a frame clock. |
| `peer_write_timeout_ms` | 2000 | |
| `max_candidate_rate_millihz` | 120000 | DP-1 runs at 120 Hz. |
| `pixel_format_mask` | 1 | Exactly one legal value. |
| `effect_mask` | 0 | No effect vocabulary is admitted. |

**Full-screen content is excluded by a resolved-coverage check, not by image
size and not by the role label alone.** Four tiles each under 4 MiB can cover
2560x1440 within the resident cap, so the per-resource bound proves nothing. Nor
does the role: `max_panel_extent` bounds only a panel's thickness, an output
shorter than 512 logical units could fit a full-height "panel", and a popout has
no inherent size. The binding rules are therefore all of: one panel per output,
at most three popouts, a bounded popout extent, and an explicit check that the
**summed resolved allocation area stays within `max_content_coverage_percent` of
the output**. A role enum is not a placement policy.

**Popout placement is an algorithm, not a preference.** Engine places a popout on
the side of its anchor with the most room, breaking ties in the order away-from-
panel-edge, then start-edge. If the popout cannot fit wholly inside the output
after margins, the allocation is **rejected** rather than clipped or shifted onto
another output. Sibling popouts on one output order by their surface index, which
is why that index is explicit rather than inferred. `Surface` repeats the exact
acknowledged allocation tuple so desired and granted geometry never blur.

**Candidate slots are counted by phase, not by arrival order.** Each output has
at most one *presented* candidate, at most one *submitted* candidate that the
renderer owns and which is non-cancellable, and at most one *pending* candidate
that has not been submitted. A new candidate may supersede only the pending one.
If every slot is in a non-cancellable phase, the new candidate is rejected with
backpressure rather than displacing work in flight. Every admitted candidate
still receives exactly one terminal outcome, and its referenced generations stay
pinned.

`max_live_resources` x `max_resource_bytes` exceeds `max_resident_bytes` by
design: both are enforced, and resident bytes is the binding constraint.

`transfer_timeout_ms` is set to 2000 because the live scheduler already uses that
figure, but they are different waits and the equality is not evidence. Treat it
as a **prototype upload limit to validate** against 4 MiB and slow-peer copy
benchmarks, and expect to change it.

Exhaustion of any limit rejects or supersedes *through the lifecycle*. It never
grows memory and never silently drops accepted work.

### 6. Wire records

All at `since=5`. Every kind requires bit 7 except 160. Kinds 179–180 also
require bit 8. Directions: **S** session/Engine to shell, **C** shell to session.
Responses echo the request's frame-header transaction; server-origin events use
an independently increasing server-namespace transaction.

| Kind | Message | Payload |
| --- | --- | --- |
| 160 | `ContentAdmissionRefused` S | `transaction=0`. `reason:u16`, `reserved:u16=0`, `denied_capabilities:u64`. permission_denied=1, unsupported=2, invalid_dependencies=3, unavailable=4. Close after sending. The **sole** pre-grant content record; never sent to old or descriptor-only peers. |
| 161 | `ContentLimits` S | `transaction=0`. The §5 table plus `limits_generation:u64`. Sent before any other content record. |
| 162 | `ContentOutputFacts` S | `facts_generation:u64`, `output_count:u32`, `reserved:u32=0`, then per output `{output, local_width:u32, local_height:u32, scale_numerator:u32, scale_denominator:u32, scale_generation:u64}`. Complete replacement; empty is a valid no-output state. 16 outputs is a 672-byte payload, so it never spans. No global origins. |
| 163 | `ContentAllocationRequest` C | `output`, `allocation_request_id:u64`, `operation:u16`, `role:u16`, `edge:u16`, `reserved:u16=0`, `prior:A`, `parent:A`, `parent_presentation_epoch:u64`, `anchor_parent_rect:RectP`, `desired_width:u32`, `desired_height:u32`, then margins `top,right,bottom,left:i16`. operation acquire=1/replace=2/release=3; role panel=1/popout=2. A popout parent must be presented on that output. |
| 164 | `ContentAllocationResult` S | `allocation_request_id:u64`, `status:u16`, `reason:u16`, `output`, `allocation:A`, `parent:A`, `scale_generation:u64`, logical and **exact pixel** extents, scale rational, `allowed_reservation_extent:u32`, and the acknowledged margin tuple. granted=1/rejected=2/released=3/invalidated=4. A grant authorizes candidate *proposals*, not presentation or reservation changes. The allocation generation changes whenever margins, placement, scale, dimensions or parent binding change. |
| 165 | `ContentResourceBegin` C | 64 bytes. `R`, `width_px:u32`, `height_px:u32`, `rendered_scale_numerator:u32`, `rendered_scale_denominator:u32`, `pixel_format:u16=1`, `reserved:u16=0`, `chunk_count:u32`, `total_bytes:u64`. No stride field. |
| 166 | `ContentResourceStatus` S | `R`, `status:u16`, `reason:u16`, `next_ordinal:u32`, `admitted_bytes:u64`. transfer_admitted=1/resource_accepted=2/rejected=3/cancelled=4. Admission reserves staging bytes and a resident slot *before* authorizing chunks, and is not acceptance. |
| 167 | `ContentResourceChunk` C | 48-byte prefix (`C` 16, `R` 16, `ordinal:u32`, `byte_count:u32`, `offset:u64`) then bytes. Dense ordinals, contiguous offsets, whole rows, nonzero data, no duplicates, overlaps or holes. |
| 168 | `ContentResourceEnd` C | `R`, `total_bytes:u64`, `chunk_count:u32`, `reserved:u32=0`. Must match Begin and received totals exactly. Local-stream integrity, not a content hash as identity. |
| 169 | `ContentResourceCancel` C | `R`. Cancels an admitted *incomplete* transfer. Cancelling an accepted resource is rejected; use Retire. |
| 170 | `ContentResourceRetire` C | `R`. A promise to stop introducing new references. Candidates not yet accepted that reference `R` are rejected; already-accepted candidates may finish. |
| 171 | `ContentResourceReleased` S | `R`, `reason:u16`. Sent **exactly once** per accepted resource, only when no candidate, upload, render, scanout or retained-image reference can consume its storage. Release is not a candidate outcome. |
| 172 | `ContentCandidateBegin` C | 80 bytes. `candidate_generation:u64`, `output`, `facts_generation:u64`, `pacing_permit:u64`, `interaction_generation:u64`, `surface_count:u32`, `placement_count:u32`, `target_count:u32`, `effect_count:u32=0`. Candidate generations are strictly increasing across the complete content grant, including different outputs; `Chunk` and `End` omit the output and therefore require one grant-wide identity namespace. Begins a complete replacement of the content cohort on that one output. |
| 173 | `ContentCandidateChunk` C | 40-byte prefix, then `Surface[]`, `Placement[]`, `Target[]` in that order. All r5 data fits one chunk. |
| 174 | `ContentCandidateEnd` C | 40 bytes. Counts must equal Begin and the summed tables. All-zero lists withdraw the whole per-output cohort. A surface with zero placements is explicit transparent content, not preserved old content. |
| 175 | `ContentCandidateOutcome` S | `candidate_generation:u64`, `output`, `kind:u16`, `reason:u16`, `presentation_epoch:u64`, `work_area_generation:u64`, `wm_commit_generation:u64`. prepared=1/presented=2/rejected=3/superseded=4. Only presented carries a nonzero presentation epoch. |
| 176 | `ContentFrameDemand` C | `output`, `allocation:A`, `demand_id:u64`, `reason:u16` dirty=1/animate=2/withdraw=3. One coalescible standing demand per output. No client deadline or compositor timestamp. |
| 177 | `ContentFramePermit` S | `output`, `demand_id:u64`, `permit_id:u64`, `state:u16`, `reason:u16`, `ttl_ms:u32`, `max_candidate_bytes:u32`. Permission for **one** candidate Begin. A consumed or expired id is never recycled. |
| 178 | `ContentFrameDemandCancel` C | `output`, `demand_id:u64`, `permit_id:u64`. Cancels pacing demand only, never an accepted candidate. |
| 179 | `ContentAction` S | `output`, `candidate_generation`, `presentation_epoch`, `interaction_generation`, `allocation:A`, `target_id`, `target_generation`, `action_id`, `event_id`, `kind:u16` activate=1/outside_dismiss=2/cancel=3. **No coordinates, device ids, timestamps, motion traces or application identity.** Dismiss names a presented popout with target fields zero. |
| 180 | `ContentActionAck` C | Echoes the action identity exactly, plus `disposition:u16` consumed=1/rejected_stale=2. One-use exact-match acknowledgement. Local state changes still require a new complete candidate. |

Table rows inside `ContentCandidateChunk`:

| Record | Size | Fields |
| --- | --- | --- |
| `Surface` | 64 | `A`, `scale_generation:u64`, `role:u16`, `edge:u16`, margins `top,right,bottom,left:i16`, `reservation_extent:u32`, `parent_surface_index:u16` (65535 = none), `reserved:u16=0`, `anchor_parent_rect:RectP`, `flags:u32=0`. A panel requires no parent; a popout names a panel row in this candidate. Margins belong to the surface, never to a tile. |
| `Placement` | 32 | `R`, `surface_index:u16`, `flags:u16=0`, `destination_x_px:i32`, `destination_y_px:i32`, `reserved:u32=0`. Destination size comes from the immutable resource. Every tile fits wholly inside its allocation; no implicit scaling or cropping. |
| `Target` | 48 | `surface_index:u16`, `action_kind:u16`, `target_id:u64`, `target_generation:u64`, `action_id:u64`, `bounds_px`, `flags:u32=0`. `local_activate=1`. Bounds lie wholly inside the named allocation. Identities are nonzero and client-local; duplicates are rejected. No keyboard, motion or absolute-pointer subscription. |

Shared bounded reason enum for post-grant records: none=0, stale=1, budget=2,
malformed=3, unauthorized=4, incomplete=5, timeout=6, output_lost=7,
allocation_lost=8, renderer_failed=9, superseded=10, cancelled=11, revoked=12.
Success requires reason 0. Broken framing or uncorrelatable identity closes the
connection rather than emitting an unbounded stream of error replies.

Kinds 123–127 and 135–159 remain unallocated by this record. The sibling
revision-6 indicator vocabulary now occupies 181–186 and bits 9–10 in the
checked-in schema. Content remains at 160–180 and bits 7–8; implementing it
does not downgrade the family's current revision or reallocate indicator kinds.

### 7. Resource identity, retention and release

Ownership key is **`(connection_epoch, content_grant_epoch, resource_id,
resource_generation)`**. A bare numeric id, a hash, a client pid or an OS address
is never authority.

Resource ids strictly increase; reusing a logical id requires generation exactly
previous+1, including after a rejected Begin, and generation 0 is invalid. A new
Begin for an old key is rejected, never aliased. High-water entries are retained
up to `max_resource_ids` and **not evicted**, because evicting a tombstone would
permit replay; exhaustion means new generations or a fresh connection.

After acceptance, bytes, format and extent cannot change. Engine's copy is
independent and read-only, and each generation is a distinct stored object, so
reusing an id can never overwrite an older generation's render references.

A candidate outcome and a resource release are **independent obligations** —
rejected or superseded does not imply storage may be freed, and an accepted
cached resource may stay resident indefinitely while its owner reuses it. Forced
release after output loss is possible only once every reference drains, including
references from another output.

### 8. Pacing — one Engine clock

Static content uploads once, produces a candidate under one permit, then goes
idle; no heartbeat is sent to an idle shell. Dirty and animate demands coalesce
per output. Engine grants a one-use permit when a candidate slot and render
capacity exist, at its own cadence and no faster than
`max_candidate_rate_millihz`.

A permit is not presentation feedback, and its TTL is admission freshness rather
than a client frame clock; the receiver's monotonic deadline is authoritative.
Continuous animation uses this demand/permit cadence — never a private compositor
timer, and never a promise that every dirty frame appears. Multiple local changes
may be rasterized into one permitted generation.

Withdrawal is prioritized and must not starve behind animation. Security
dismissal and revocation require no permit at all, and do not wait for peer
cooperation.

### 9. Lifecycle and authority invariants

Modelled in `validation/tla/ShellContentLifecycle.tla`, registered in
`tools/check_tla.sh`. Three existing models are **reusable patterns, not
automatic inheritance**: the model evidence
must name each mapping and assumption explicitly, especially the cross-model
work-area and native-retirement boundary.

- **`ShellDescriptorLifecycle.tla`** states the retention-versus-revocation
  shape: *"Engine may retain the last pixels across ordinary shell loss, but
  targets, capture, and activations belong to exact shell and broker epochs."*
- **`StableBackingLease.tla`** models what the renderer's copy contains when
  updates arrive *while presentations still hold the bytes*, with lease
  incarnations.
- **`ShellWorkAreaCoordination.tla`** proves reservation and WM projection commit
  as one coherent bundle or not at all.

What is genuinely new is **residency and composition**: a descriptor shell
uploads no pixels, so nothing existing bounds accepted resident bytes.

1. **NoPartialPresentation** — no presented candidate references a resource that
   is merely `transfer_admitted` rather than accepted, and no candidate is
   presented with an incomplete surface, placement or target table. A partial
   transfer can never become visible or contribute an input target.
2. **ImmutableAcceptedContent** — the bytes behind an accepted key never change;
   there is no update-in-place operation, and distinct generations are distinct
   stored objects.
3. **ResourceAdmissionFreshness** *and* **CandidateReferenceValidity** — two
   separate checks. A *new* `ResourceBegin` must advance the id's high-water
   generation. Re-referencing an *already accepted* older generation stays valid
   until Retire or Released — otherwise static reuse and pinned generations would
   contradict the invariant. Candidate, interaction, output, allocation and
   pacing identities each carry their own exact epoch and generation check.
4. **BoundedResidency** — staging, resident, per-connection retiring and
   session-global retired bytes are bounded *separately*, and every generation
   holding a render or cache reference counts, including superseded and
   disconnected ones. Decrementing on Retire, Superseded or Disconnect before the
   last lease retires would satisfy the bookkeeping while leaking real memory.
5. **NoOrphanedStorage**, split into safety and liveness. *Safety:* every
   accepted storage generation has explicit counted ownership until Released, and
   Released implies zero remaining references. *Liveness:* once retirement is
   requested or the epoch is revoked, drain and release eventually follow — under
   stated renderer-progress and scheduling-fairness assumptions. An indefinitely
   stalled GPU cannot be freed safely merely because the model wants release, so
   the eventual clause is not a state invariant and watchdog failure and
   quiescence are modelled separately.
6. **CoherentBundle** — one presented candidate generation binds the entire
   ordered placement list, its surfaces and parent allocations, the exact output
   and scale generations, the margin and placement intent, the reservation with
   its work-area and WM commit, and the complete interaction snapshot.
   Composition starts from transparent black, so an omitted tile cannot become a
   hidden delta. Reusing an unchanged resource is allowed precisely because the
   new candidate names it explicitly.
7. **InputRevocationDominates** — peer loss, output loss, shell replacement,
   capability revocation and security takeover invalidate input rights and queued
   activations immediately. The revocation generation is re-checked at
   retirement, so a late Prepared, Presented or Ack cannot clear it. Retained
   pixels stay inert. Timeout scope is named rather than global: a resource
   upload timeout does not revoke otherwise healthy shell input.
8. **NoCoordinateDisclosure** — a wire property: `ContentAction` carries no
   outside coordinates, device ids, timestamps, motion traces or application
   identity.
9. **NoClickThrough** — a *dispatch* property, deliberately separate from 8. The
   dismissing event must be consumed before application routing, with capture and
   dismissal ordering tested. An action-only wire record does not by itself prove
   nothing replayed into an underlying X window.
10. **NoAuthorityAmplification** and **ExactPresentedActivation** — no
    `action_id` grants execution, a WM policy action, application focus or
    synthetic input, and an action names its exact presented candidate,
    allocation, target and generation rather than a bare integer.
11. **NoAcceptedObligationLost** — every accepted transfer, candidate, permit and
    action settles exactly once or is accounted under peer loss. A saturated
    queue rejects; it never silently drops an outcome.

**Revalidate at commit; do not trust readiness.** Readiness latches when
content, reservation and the WM answer agree, and content can stop being live
between that latch and presentation. Presentation therefore rechecks that the
candidate's resources are still accepted and rejects rather than committing a
bundle whose pixels have gone. The rule is here because the composition model
violates `PresentedBundleHasLiveContent` without it — a latch that neither the
content nor the work-area model was watching.

**The renderer transition is non-cancellable.** Once Engine hands a composed
bundle to the renderer it completes or fails as a unit. Prepared means a
validated complete candidate was submitted for rendering, not merely parsed;
Presented requires real applicable-output retirement.

**Deadlines are recovery triggers, never permission for unsafe reuse.** The
transfer deadline reclaims staging bytes only; a watchdog cannot manufacture a
GPU completion. Refusing new work is preferable to exceeding the process budget
while a stalled renderer holds memory.

### What the model checks, and what it does not

TLC checks invariants 1, 3, 4, 5 (safety half), 6 in part, 7 and 11, over
47,979 distinct states to depth 30. Two negative controls prove the invariants
bite rather than merely holding: disabling the retire-during-assembly rejection
violates `CandidateReferenceValidity`, and letting an assembly deadline settle
without an outcome violates `NoAcceptedObligationLost`. Each control encodes a
defect a design review actually found.

Not covered, and not claimed:

- **ImmutableAcceptedContent** is structural. No update-in-place operation
  exists, so the model has nothing to violate.
- **CoherentBundle** is checked across both halves by
  `ShellContentBundleComposition.tla`, which models the seam rather than merging
  two large models. It found an assumption neither model states: the work-area
  model latches `candidateReady` and treats it as opaque, while content can be
  retired or revoked afterwards, so **presentation must revalidate content
  liveness at commit rather than trust the latch**. Without that revalidation a
  presented bundle pairs live geometry with dead pixels, and its control proves
  the check catches exactly that.
- **NoClickThrough** is a dispatch property about event routing, not a property
  of this state machine.
- **The liveness half of NoOrphanedStorage** needs fairness assumptions about
  renderer progress. The model treats a stalled renderer as a reference that has
  not drained, which is the honest encoding; it does not assert release must
  eventually happen.

Bytes are abstracted to one unit per generation, so the model checks
conservation across staging, resident and retiring — not the arithmetic in §5.

### 10. Conformance corpus

The following is the full required implementation corpus. The implementation
record names the portions that exist; this table is not a claim that every
transport lifecycle or native workflow is implemented.
`sophia-shell-v1.kdl` remains the checked-in role schema, gaining `since=5` and
gate assignments; kinds 96–122 and all golden bytes are preserved.

| Artifact | Role |
| --- | --- |
| `protocol/golden/sophia-shell-content.frames` | Valid vectors: negotiate, limits, facts, allocation, transfer, panel+popout candidate, outcomes, retire/release, pacing, action/ack. |
| `protocol/golden/sophia-shell-content-malformed.frames` | Malformed vectors with case, decoder and expected error: overflow, invalid counts and enums, truncated and trailing records, zero and foreign identities. |
| `protocol/golden/sophia-shell-content.lifecycle.json` | Bounded scenario table with expected transitions and exact ownership counts; no host times. |
| `crates/sophia-protocol/examples/shell_content_corpus.rs` | Generates both vectors from fixed fixtures. Not normative. |
| `crates/sophia-protocol/tests/shell_content_wire.rs` | Decode/re-encode golden equality and malformed rejection per message. |
| `crates/sophia-runtime/tests/shell_content_transport.rs` | Negotiation gates, fragmentation, transfer and permit bounds, stale identities, terminal obligations, slow peer, disconnect. |
| `crates/sophia-runtime/examples/shell_content_conformance_host.rs` | Engine-owned black-box lifecycle host, independent of any X frontend. |
| `bindings/c/tests/sophia_shell_content_client.c` | Independent C client from the admitted prose and KDL: no Sophia libraries, no generated codec, no toolkit. |
| `protocol/archive/sophia-shell-v1-r4/` | Retained pre-extension descriptor C source, fixtures and `SHA256SUMS` with a baseline commit. A compatibility fixture, not a declaration that r4 is stable. |

New Rust modules are `packets/shell_content.rs` and `ipc/shell_content.rs`, with
runtime state in a `shell_transport` child module — not a second role endpoint.

`tools/check_shell_protocol.sh` keeps every existing invocation and adds, in
order: corpus generation compared with `cmp` against retained goldens without
regenerating them; the new Rust tests with content features explicitly enabled,
since a feature-gated skip must never count as passing; the C client compiled
under the existing warnings-as-errors discipline and run against **both** vectors
so it rejects malformed input independently rather than asking Rust to
prevalidate; the archived r4 client run descriptor-only after checksum
verification; and the Lom adapter proof invoked by absolute path, reported
**distinctly as unavailable** rather than silently skipped.

The script also gains an explicit `shell_reference` invocation, which it
currently lacks despite listing base, tabs and launcher. There is no
`shell_reference_corpus.rs` generator; the retained artifacts are
`protocol/golden/sophia-shell-reference.frames` and
`crates/sophia-protocol/tests/shell_reference.rs`.

**Format-mask cases.** The corpus must carry these explicitly, so a later
revision cannot quietly widen the admitted pixel contract: `pixel_format_mask` of
0, a wrong singleton, and a mask with a second bit set; and a
`ContentResourceBegin` naming an unadmitted `pixel_format`. Each is a malformed
vector with its expected error, not a general category.

**Non-opted-in proof.** The matrix has three outcome classes, and the zero-byte
assertion applies to exactly one of them — an earlier draft demanded zero content
frames universally, which no implementation could satisfy, since a granted
content peer must receive the content lifecycle.

| Connection | Required raw-socket outcome |
| --- | --- |
| r1 descriptor, r2 tabs, r3 reference, r4 launcher, r5 descriptor-only | **Zero kinds 160–180**, ever |
| r5 requesting bit 7, operator permission denied | Kind **160** only, then closure |
| r5 bit 8 without bit 7 | Legacy refusal and closure, **no r5 bytes** — the 160 gate requires bit 7, and this keeps that ratified gate rather than widening it |
| r4 or earlier requesting an unknown content bit | Legacy refusal path, **no r5 bytes** |
| r5 bit 7 granted, and r5 bits 7+8 granted | The exact gated content lifecycle |

For the zero-byte class, drive source changes during each connection and assert
no content kind is emitted at the raw socket, with full equality of
deterministic old-flow bytes under fixed epochs and transaction ids. Compare
old-byte invariance under a fixed negotiated revision — an r5 welcome is not
required to equal an r4 welcome. Outbound gating must be asserted before encode
and enqueue on *every* path, including facts, asynchronous invalidation,
timeouts, permits, release and input — not only the happy-path sender. Inject
content frames from descriptor clients and require refusal and closure. Combine
an archived pre-r5 reader with a raw byte capture, so a reader that silently
ignores unknown bytes cannot hide an illegal send.

Narthex stays descriptor-only and must not be made to request content merely to
exercise new corpora; its unchanged Nim tests are the independent witness that
non-opted-in peers still work, including kind 98's reserved/trailing rejection
and the `TabsEntry` embedded payload.

## Appendix A — Normative wire definitions

Everything is little-endian. This appendix exists so the independent C client can
be written from this record alone; nothing here depends on an unpublished draft.

### A.1 Envelope and aliases

The existing 24-byte `sophia_ipc` frame header is unchanged and content adds no
envelope field.

| Alias | Size | Layout | Unit |
| --- | --- | --- | --- |
| `C` | 16 | `connection_epoch:u64`, `content_grant_epoch:u64` | Grant identity |
| `O` | 16 | `output_id:u64`, `output_generation:u64` | |
| `A` | 16 | `allocation_id:u64`, `allocation_generation:u64` | |
| `R` | 16 | `resource_id:u64`, `resource_generation:u64` | |
| `RectL` | 16 | `x:i32`, `y:i32`, `width:u32`, `height:u32` | **Logical**, half-open |
| `RectP` | 16 | `x:i32`, `y:i32`, `width:u32`, `height:u32` | **Physical pixels**, half-open |

`RectL` and `RectP` share a layout and are deliberately distinct types; a decoder
must not accept one where the other is specified.

**`C` prefixes every post-grant content record exactly once**, immediately after
the envelope, and every payload size quoted in §6 includes it. Kind 160 carries
no `C`, because it is pre-grant.

**Epoch issuance.** The session issues `connection_epoch` at admission and
`content_grant_epoch` when it grants content. `ContentLimits` (161) is the first
record carrying the new grant epoch and thereby establishes it; every later
record repeats that exact `C`, and one that does not match the live grant is
rejected without side effects. Both epochs increase strictly, are never reused,
and exhaustion requires a new connection rather than a wrap.

**Null sentinels.** `A` and `R` encode `0/0` only where expressly inapplicable —
a panel's `parent`, a release request's proposal fields, an output-cohort frame
demand. `O` is never null. A zero id with a nonzero generation, or the reverse, is
malformed. `parent_surface_index` uses `65535` for "none", because `0` is a valid
surface index.

**Reserved fields** are always zero on send and rejected when nonzero. Every
record ends with `cursor.finish()`, so trailing bytes are rejected exactly as
kind 98 already does.

### A.2 Payload expansions

Records whose §6 row is a summary are given in full here. Sizes shown are
payloads including `C`.

**161 `ContentLimits`** — `C`, `limits_generation:u64`, then `u64`:
`max_resource_bytes`, `max_staging_bytes`, `max_resident_bytes`,
`max_retiring_bytes`, `max_session_retiring_bytes`, `pixel_format_mask`,
`effect_mask`. Then `u32` in this exact order: `max_frame_payload`,
`max_chunk_bytes`, `max_width_px`, `max_height_px`, `max_live_resources`,
`max_resource_ids`, `max_open_transfers`, `max_outputs`,
`max_allocations_total`, `max_allocations_per_output`, `max_panels_per_output`,
`max_popouts_per_output`, `max_candidate_surfaces`, `max_candidate_placements`,
`max_candidate_targets`, `max_candidate_bytes`, `max_pending_allocation_requests`,
`max_open_candidates_total`, `max_open_candidates_per_output`,
`max_pending_candidates_total`, `max_pending_candidates_per_output`,
`max_pending_actions`, `max_frame_demands_per_output`, `max_control_records`,
`reserved_control_queue_bytes`, `max_input_queue_bytes`,
`max_output_queue_bytes`, `max_frames_per_service_tick`, `max_panel_extent`,
`max_popout_extent_px`, `max_reservation_extent`,
`max_content_coverage_percent`, `max_margin_logical`, `max_scale_numerator`,
`max_scale_denominator`, `allocation_timeout_ms`, `transfer_timeout_ms`,
`transfer_idle_timeout_ms`, `candidate_timeout_ms`, `preparation_timeout_ms`,
`presentation_timeout_ms`, `action_ack_timeout_ms`, `permit_timeout_ms`,
`peer_write_timeout_ms`, `max_candidate_rate_millihz`, `reserved:u32=0`.

Global and per-output ceilings are **separate fields**; a single number cannot
express "16 total, 4 per output".

**162 `ContentOutputFacts`** — 32-byte prefix (`C`, `facts_generation:u64`,
`output_count:u32`, `reserved:u32=0`) then `output_count` x 40-byte records
(`O`, `local_width:u32`, `local_height:u32`, `scale_numerator:u32`,
`scale_denominator:u32`, `scale_generation:u64`). At `max_outputs` = 16 that is
672 bytes, so it never spans a frame.

**164 `ContentAllocationResult`** — `C`, `allocation_request_id:u64`,
`status:u16`, `reason:u16`, `reserved:u32=0`, `O`, `allocation:A`, `parent:A`,
`scale_generation:u64`, `logical:RectL`, `pixel:RectP`, `scale_numerator:u32`,
`scale_denominator:u32`, `allowed_reservation_extent:u32`, margins
`top,right,bottom,left:i16`, `acknowledged_anchor:RectP`, `reserved2:u32=0`. Rejected and released results
zero every geometry field. An asynchronous `invalidated` carries
`allocation_request_id = 0` and a server transaction.

**172 `ContentCandidateBegin`** — 80 bytes: `C`, `candidate_generation:u64`, `O`,
`facts_generation:u64`, `pacing_permit:u64`, `interaction_generation:u64`,
`surface_count:u32`, `placement_count:u32`, `target_count:u32`,
`effect_count:u32=0`.

**173 `ContentCandidateChunk`** — 40-byte prefix: `C`,
`candidate_generation:u64`, `chunk_ordinal:u32`, `surface_count:u32`,
`placement_count:u32`, `target_count:u32`; then the `Surface`, `Placement` and
`Target` tables in that order, with the layouts in §6. Chunk ordinals are dense
from zero. Where several chunks are used, table indices are dense concatenations
across ordered chunks, and references are resolved only at End.

**174 `ContentCandidateEnd`** — 40 bytes: `C`, `candidate_generation:u64`,
`surface_count:u32`, `placement_count:u32`, `target_count:u32`,
`reserved:u32=0`. Counts must equal Begin and the summed chunk tables. A missing
or duplicated chunk ordinal is rejected.

**177 `ContentFramePermit`** — `C`, `O`, `demand_id:u64`, `permit_id:u64`,
`state:u16` (granted=1, expired=2, cancelled=3, rejected=4), `reason:u16`,
`ttl_ms:u32`, `max_candidate_bytes:u32`, `reserved:u32=0`.

**179 `ContentAction`** — `C`, `O`, `candidate_generation:u64`,
`presentation_epoch:u64`, `interaction_generation:u64`, `allocation:A`,
`target_id:u64`, `target_generation:u64`, `action_id:u64`, `event_id:u64`,
`kind:u16`, `reason:u16`, `reserved:u32=0`.

**180 `ContentActionAck`** — the identical field sequence through `event_id`,
then `disposition:u16`, `reserved:u16=0`, `reserved2:u32=0`. Every identity field
must match the action exactly; a mismatch in any one is `rejected_stale`.

**160 `ContentAdmissionRefused` gating.** It is emitted only after a
syntactically valid `ClientHello` whose revision range admits 5 **and** whose
requested capabilities include bit 7. Malformed or pre-negotiation traffic never
authorizes these bytes; it takes the existing fail-closed disconnect path.

## Appendix B — State transitions

Invariant names are not a state machine. These are the transitions an
implementation and the model must agree on.

### B.1 When a candidate is accepted

A candidate is **accepted** at the moment `ContentCandidateEnd` validates
successfully. Acceptance is what pins its referenced resource generations and
grants visual and input authority.

**A response is owed earlier than that.** Once a well-formed
`ContentCandidateBegin` consumes an issued permit, Engine reserves one control
response and **guarantees a correlated terminal result**: an incomplete, timed-out
or invalid End produces `ContentCandidateOutcome` *rejected* with the applicable
reason. "No accepted render candidate yet" is not the same as "no response owed",
and a deadline that only adjusts server bookkeeping is not a recovery protocol —
without this the client, which by §B.2 waits for a terminal outcome before
demanding another frame, would wait forever.

Pins and authority still begin only at successful End. `Prepared` follows
acceptance; `Presented` requires real output retirement. Unidentifiable or
abusive framing still closes the connection instead of replying.

### B.2 Permits

A granted permit reserves the pending-candidate slot, and that reservation is
**carried through** rather than released at Begin: grant reserves it, Begin
transfers it to the assembly, successful End transfers it to accepted pending
work, and it is released only on rejection, expiry or settlement. Releasing at
Begin would hand the same slot to a second promise. `max_open_candidates` is
checked both before a permit is granted and before a Begin is admitted, so the
conservation holds rather than resting on counter limits alone.

**At most one outstanding permit per output**, so permits cannot be issued
unbounded under one demand. Consuming a permit clears the standing demand; a
client that still has work raises a new demand after its terminal outcome. Expiry
and cancellation are reported as `ContentFramePermit` states, not silently. A
Begin naming a consumed, expired, foreign or wrong-output permit is rejected. On
assembly timeout the candidate and its permit and slot bookkeeping are explicitly
discharged, so the peer can safely demand another frame.

### B.3 Retire, cancel and their races

`ContentResourceRetire` after acceptance does not cancel accepted candidates;
they finish and their pins release normally. A candidate still **assembling**
that names the retired generation is **rejected**, because Retire forbids new
references and an unaccepted candidate has not earned its pins. Model checking
is what put this sentence here: the rule was stated in §6's kind 170 row but not
in this section, which is where race resolution is supposed to live, and the
first model written from this appendix violated `CandidateReferenceValidity`
within eight states. A duplicate, unknown or
out-of-order Retire receives an explicit rejected `ContentResourceStatus` and
creates **no second release obligation**. `ContentResourceCancel` racing a
successfully validated End is rejected, because the transfer is no longer
incomplete. Release of an old epoch's storage is internal accounting; a dead peer
receives no notification and none is owed.

### B.4 Actions and dismissal

`activate` and `cancel` link through `event_id`. A `cancel` references the
`event_id` of an unacknowledged action and is itself **not** acknowledged.

On `action_ack_timeout_ms` Engine revokes interaction for that target rather than
waiting. Outside dismissal follows the same bound: if the shell does not withdraw
the popout within `action_ack_timeout_ms`, **Engine performs the coherent
withdrawal itself** and revokes interaction, rather than leaving an interactive
popout on screen pending peer cooperation. A late acknowledgement or a late
retirement after revocation never restores input; retained pixels stay inert.

### B.5 Budget conservation

- `ContentResourceBegin` reserves **both** staging bytes and resident byte
  credit, so a completing transfer can never fail at End for budget. Exactly one
  terminal status is emitted either way, and a rejected Begin acquires nothing.
- Staging, resident and retiring are **disjoint** classes, each within its own
  cap, and their sum within an explicit ceiling:

  ```
  max_connection_bytes = max_staging_bytes + max_resident_bytes + max_retiring_bytes
                       = 8 MiB + 16 MiB + 16 MiB = 40 MiB
  ```

  Bytes move class exactly once, on acceptance and on retirement. A reserved
  credit is an accounting entry, not allocated memory: resident credit taken at
  Begin is charged at admission, while the actual bytes still sit in staging.
- If acceptance copies rather than moves, the **peak overlap is charged** until
  the temporary is freed. "Never charged to both" describes bookkeeping, and does
  not excuse real duplicate storage.
- **Renderer and upload copies are charged to the resident class.** A named
  native renderer image-store budget does exist —
  `DEFAULT_NATIVE_RENDERER_IMAGE_BYTE_BUDGET`, 512 MiB, at
  `gbm_platform/scanout/context.rs:141`, owned by
  `NativeGbmRenderedScanoutContext`, checked in
  `context/renderer_images.rs:228-265` and returning `RendererImageStoreFull` on
  exhaustion. Its scope is native image store and bridges, which does **not**
  establish an aggregate bound over every content CPU copy, upload temporary and
  composition target. r5 therefore charges content-owned renderer and upload
  copies to the connection's **resident** class at peak overlap, keeping this
  ledger closed and checkable on its own. A separately specified aggregate
  renderer budget may later replace that charge.
- Before live work is admitted, the session reserves **global retirement credit**
  equal to `max_connection_bytes`, because a disconnect can turn a connection's
  entire live footprint into dead-epoch bytes. Against
  `max_session_retiring_bytes` of 64 MiB that leaves 24 MiB of headroom, and
  since exactly one content grant is live at a time, a reconnect is admitted only
  once the prior epoch has drained to at or below that headroom. A stalled
  renderer therefore produces admission backpressure rather than an unbounded
  dead-epoch class.
- **Retirement never discards referenced storage to stay under a cap.** If the
  retiring class is full, a Retire or epoch close does not free bytes the
  renderer still references; the connection refuses new admissions until
  references drain. Exhaustion is backpressure, never a silent free.
- Control-queue capacity for an obligation's outcome is reserved in the 64-record
  control queue **before** that obligation is accepted. Response priority alone
  does not establish `NoAcceptedObligationLost`.

## Alternatives

**File-descriptor passing, shared memory, or DMA-BUF for pixel transfer.**
Rejected for r5. `content-shell.md` requires measured need plus a separate
specification covering ownership, synchronization, format, damage, lifetime,
fallback and renderer failure. They also cost more than they look: the shell
transport has no ancillary-data path today, so this would be a transport rewrite
rather than a message addition. They remain named candidates.

**One complete resource per surface, instead of a placement list.** Considered
seriously and rejected. It is simpler to validate, but it forces a full-surface
re-upload for any change — 320 KiB whenever a clock ticks, against 15 KiB for the
tile. The two costs that argued against tiles, z-order and partial coverage, are
each answered by one rule: the placement list is ordered, and every allocation is
rebuilt from transparent black. With those, tiles carry no hidden state and
remain a complete replacement rather than a delta.

**Deltas against a base generation.** Rejected outright: a delta must name and
validate its base, and an ordered complete list achieves the same bandwidth
saving without one.

**Allocating content kinds at 123.** Rejected: only five kinds are free before
`control_v1`'s block at 128–134, and content needs about twenty. Splitting the
content vocabulary around another family's block is worse than a fresh aligned
block at 160.

**Following the `0xFF00` extension-record shape.** Rejected: that mechanism
exists because `wm_v1`'s counted-record path is frozen. `shell_v1` is not frozen
and allocates ordinary dense gated messages.

**Extending `ShellV1DescriptorSnapshot` instead of adding records.** Rejected as
wire-incompatible: `ipc/shell_v1.rs` requires `reserved == 0` followed by
`cursor.finish()`, so old readers reject trailing bytes, and `TabsEntry` embeds
that payload — an edit would break the tabs codec and Narthex's independent Nim
decoder.

**Resampling a scale-mismatched resource rather than rejecting it.** Rejected: it
produces blurry text and pairs new pixel geometry with input targets computed at
the old scale.

**Bounding full-screen content by image size.** Rejected as unsound: several
tiles each under the per-resource cap can cover a whole output. Coverage is a
matter of allocation role and extent, so that is where it is enforced.

**The interim fallback — publishing the active-output fact where the existing X11
panel can read it.** Not taken. It would show focus on an empty output sooner,
but it entrenches a fixture that holds no shell authority and is scheduled for
removal. Recorded as a deliberate choice rather than an oversight, and still
available if this design and its implementation stall.

## Consequences

### Amendments to content-shell.md

1. **Driving client.** The acceptance section requires "both the Quickshell
   adapter and an independent C client." **Amend to ironbar plus an independent C
   client.** The purpose survives — no toolkit becomes a Sophia dependency, and
   the contract is implementable with neither Qt nor Sophia libraries. The
   Quickshell obligation is dropped because that fixture is being retired, and
   proving a contract against something scheduled for deletion is dead weight.
   Recorded as an explicit amendment: choosing a fork does not silently rewrite
   an admitted requirement.

2. **Transfer mechanism.** The proposal leaves cached content as "the first
   transport experiment." This record fixes it as bounded chunked byte frames for
   r5 and keeps shared memory, fd passing and DMA-BUF as named candidates, each
   still owing the full specification named above.

### What this does not close

- **Indicator and active-output delivery is a separate vocabulary.** `wm_v1` r3
  already carries `ProjectionIndicator` and `ProjectionOutputStatus` into Engine;
  republishing them to a shell is a sibling extension. This record allocates no
  kind for it. Content drawing authorizes no policy action — a shell-local
  `action_id` must not be conflated with a policy action integer, and relaying one
  preserves its issuer and epoch checks.
- **CP-15 prerequisites.** `t022` (family audit) and `t023` (whole-family
  conformance runner) remain open. `check_shell_protocol.sh` is the shell entry
  point and does not become either.
- **Effects, keyboard interactivity, and layer ordering.** No effect vocabulary is
  admitted and `effect_mask` is 0. r5 carries discrete actions only, so there is
  no keyboard focus to hand a content surface. Ordering is implied by role — a
  popout orders above its parent panel — and a general layer vocabulary awaits a
  driving client that needs one.
- **A custom launcher.** Unchanged from the proposal: content drawing alone
  authorizes no launch.
- **An aggregate Engine renderer byte budget.** The native image-store budget
  cited in §B.5 covers its own scope, not every content CPU copy, upload
  temporary and composition target. Until an aggregate bound is specified, §B.5
  charges those to the connection's resident class.
- **Measurement.** `transfer_timeout_ms` and the tile bandwidth argument are both
  prototype selections awaiting the static-panel, changed-region and
  continuous-content measurements the proposal already requires.
- **Physical acceptance.** Headless and X11-panel results do not substitute for
  native physical presentation and input evidence on the real desktop.

### Implementation consequences

The plan's implementation phase bundles content with indicator publication; per
the scoping above, those are two extensions that may land together but must be
specified apart.

The first workflow that must work end to end: one panel on a selected output, a
bounded work-area reservation, a button, an anchored popout, and Engine-mediated
outside dismissal — with the focused output visibly distinguishable **including
when it is empty**, which is the case that motivated this work.

## Acceptance and connections

Status is **proposed**. `ShellContentLifecycle.tla` now exists and passes under
TLC with two negative controls; §9 records exactly which invariants that covers
and which it does not. `ShellContentBundleComposition.tla` checks the content
and work-area seam, with its latched-readiness negative control. This is the
composition check described in step 3 below, not a claim to have composed every
action of all three sibling models.

**Operator acceptance of this record authorizes modelling, not implementation.**
`content-shell.md` requires the lifecycle and authority invariants to be *modeled
and checked* before implementation, and accepting an architecture does not
discharge that. The gate is a sequence, and each step blocks the next:

1. The wire appendix, budgets and state transitions are final — this record.
2. `ShellContentLifecycle.tla` is written, with its mapping to
   `ShellDescriptorLifecycle`, `StableBackingLease` and
   `ShellWorkAreaCoordination` named explicitly, along with its renderer-progress
   and scheduling-fairness assumptions. **Done**, with the coverage limits in §9.
3. Model checking runs and its traces are retained under the evidence policy.
   **Done for the safety invariants named in §9, including the cross-model
   composition.** The liveness half of release is not, and remains an explicit
   renderer-progress assumption.
4. **Only then** is implementation authorized.

Step 4's blocking condition is now the liveness assumption and whichever of §9's
uncovered properties the implementation is expected to rest on -- not the
composition, which is checked.

- [Content shell proposal](../../content-shell.md) — the normative document this
  design admits, and the source of the two amendments above.
- [Indicator descriptor contract](../../sophia-indicator-descriptor.md) — the
  already-implemented `wm_v1` revision 3 indicator wire that the sibling fact
  vocabulary will deliver to a shell.
- [Quickshell X11 panel](../../quickshell-x11-panel.md) — the fixture this
  replaces, retired once the content shell renders.
- `t022` and `t023` in [todo](../../../todo.md) remain open CP-15 prerequisites
  that this record does not close.
