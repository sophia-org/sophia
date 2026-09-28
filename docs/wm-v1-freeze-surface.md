# `sophia_wm_v1` Wire-Change Surface

**Role:** decision record for wire-layout risk.

Historical scope: this survey describes the retired WM IPC endpoint, including
source paths removed by t269. Current WM roles use the
[WM file contract](sophia-wm-files.md). The survey's old transport citations are
not current implementation or conformance claims.

This file enumerates, for every retained row in Hagia's port ledger, whether
closing that row can require a wire change — and if so, which kind. It was
written to bound the risk of freezing the protocol. `sophia_wm_v1` is no longer
frozen: it is a work in progress whose revisions advance, with
`ClientHello`/`ServerWelcome` negotiation as the compatibility contract (see
`docs/sophia-wm-api.md`).

The optional `pointer_focus` cause now has an explicit outbound gate in
`PolicyWmSessionTransport::send_projection_request`: cause kind 4 is refused
before writing unless capability bit 13 was selected. This is the gated enum
addition anticipated by the direction rule below; existing cause encodings and
message layouts remain unchanged. See [the wire contract](sophia-policy-ipc.md#optional-presented-pointer-focus).

The optional `launch_origin` capability (bit 14) adds paired uncounted extension
chunks `0xff05` and `0xff06`, with unchanged ordinary record counts and layouts.
Both directions are gated; see [child-launch origin](sophia-policy-ipc.md#optional-child-launch-origin).

The survey outlived the freeze, because the facts in it are about wire cost
rather than permission. Knowing which rows are additive, which need a new
message kind, and which need a new revision is what makes it possible to
advance a revision deliberately instead of discovering the cost afterwards.
Read a "wire impact" column as an estimate of what a change costs, not as a
verdict on whether it is allowed. The canonical ledger is
`docs/triad-port-ledger.md` in the Hagia repository, at Triad baseline
`fb8fb27ec294e0fe2361375de0b2fa8c08be0ca9`. See
`docs/triad-port-ledger-pointer.md`.

---

## The Direction Rule

Every "what does this addition cost?" question reduces to one check, so
establish it first:

- **Client to server, additive forever.** The server simply accepts more than it
  used to, and an older client never sends the new value. Inbound capability
  gating already exists (`crates/sophia-runtime/src/policy_ipc.rs:289`, `:461`,
  `:496`).
- **Server to client, needs outbound gating.** A client speaking an older
  revision rejects what it cannot decode, so the producer must consult the
  negotiated capability set. It now does: `encode_wm_v1_policy_snapshot` takes the selected
  capabilities and omits governed record kinds along with their declared counts
  (`crates/sophia-protocol/src/ipc/wm_v1_records.rs:609`), and the production
  caller passes what negotiation actually selected
  (`crates/sophia-session/src/live_session/policy_transport_worker.rs:258`).

So the sentence "a client that never negotiates the capability never receives
it" holds in both directions. That is what lets a server-to-client addition
land without stranding older clients. The gate covers governed record kinds
only; enum values inside already-sent records sit at fixed offsets and no gate
reaches them, which is why widening an enum vocabulary is the change that costs
a revision rather than a capability bit.

Note which direction the pointer work sits in: `ProjectionRequest` is
`direction="session-to-policy"` — server to client — and carries
`interaction_kind`, whose client-side reject arm is
`crates/sophia-protocol/src/ipc/wm_v1_records.rs:290`.

## What Costs What

Four expansion moves, cheapest first. Read each against the direction rule above.

### 1. A new message kind — cheap, but not free for a frozen client

Message kinds 32 through 52 are assigned; 53 and above are free. A new message
does not touch any existing layout, so it is the correct home for genuinely new
behavior — the watched-reload visibility protocol is the known candidate.

The caveat is directional: a frozen client that *receives* an unknown message kind
fails at the envelope (`crates/sophia-protocol/src/ipc/frame.rs:79`
`UnknownMessageKind`), so new server-to-client messages still need outbound
capability gating. New client-to-server messages are additive without ceremony.

### 2. Reserved-field consumption — cheap, but asymmetric and finite

Thirteen reserved fields remain and are validated as zero on decode, so they are
genuinely held for future use rather than ignored padding. Note that the
generated Rust structs **omit** reserved fields while the wire still carries and
checks them — `WmV1ProjectionOutputRecord` has four fields but
`PROJECTION_OUTPUT_RECORD_SIZE` is 24 bytes, and
`crates/sophia-protocol/src/ipc/wm_v1.rs:419` pushes the zero that
`:448` reads back and rejects if non-zero. Do not conclude from a struct
definition that there is no reserved space.

Reserved space is distributed unevenly:

| Record | Kind | Size | Reserved |
| --- | --- | --- | --- |
| `SnapshotOutput` | snapshot 1 | 56 | **none** |
| `SnapshotSurface` | snapshot 2 | 80 | **none** |
| `SnapshotAction` | snapshot 3 | 140 | **none** |
| `SnapshotSessionOperation` | snapshot 4 | 12 | **none** |
| `ProjectionOutput` | projection 1 | 24 | `u32` |
| `ProjectionPlacement` | projection 2 | 60 | **none** |
| `ProjectionIndicator` | projection 3 | 64 | **none** |
| `ProjectionOutputStatus` | projection 4 | 48 | `u32` |

Eleven of the twenty-one messages carry one reserved `u16`. `ProjectionRequest`'s
former `reserved_cause` slot is now `interaction_axis`; the other `reserved` field
remains zero-checked. `capabilities` is a `u64` with bits 0 through 10 assigned,
leaving 53.

**Six of the eight record kinds have no reserved space at all.** Any new
per-output, per-surface, per-placement, or per-indicator fact is therefore either
a bitfield/enum extension or a layout change.

### 3. An existing enum or bitfield gains a value — pre-freeze only, and the binding constraint

Unknown discriminants are rejected, not ignored, so a frozen client cannot tolerate
a post-freeze value in anything the server sends it. **This is the constraint that
actually binds**, not record kinds: enum values live at fixed offsets inside
fixed-width records, so the uncounted extension chunk of section 4 cannot carry
them. Client-to-server widening stays additive forever; server-to-client widening
is irreversible until outbound gating exists.

The extensible fields are `SnapshotSurface.kind`,
`SnapshotSurface.capability_bits`, `request_state_bits`, `current_state_bits`,
`ProjectionPlacement.transform`, `presentation_bits`,
`ProjectionIndicator.state_bits`, `ProjectionOutputStatus.focus_bits`,
`SnapshotSessionOperation.target_bits`, and `ProjectionRequest.cause_kind` /
`interaction_phase` / `interaction_kind`.

Three of these are near-empty today and are the standing one-way doors:

- `PolicyInteractionKind` now spends its planned values on `Move = 1`,
  `Resize = 2`, `Drag = 3`, and `Scroll = 4`. This vocabulary is fixed before
  the freeze; live drag and scroll production still needs implementation.
- `PolicyTransform` has only `Identity = 1` (`:185-188`).
- `PolicyInteractionPhase` has all four of `Begin`/`Update`/`End`/`Cancel`
  (`:125-132`), but only `End` is ever constructed, so the other three are
  wire-reachable and implementation-absent. No wire change needed to use them.

### 4. A new record kind — depends entirely on whether it is *counted*

Unknown record kinds are rejected outright today:
`crates/sophia-protocol/src/ipc/wm_v1_records.rs:762` for snapshots and `:997`
for projections both end in
`other => return Err(invalid("…_record_kind", u32::from(other)))`.

**A counted record kind is the expensive one, and it is pre-freeze only.** It needs
a declared count in `*Begin`, and both `*Begin` messages are fixed-layout with a
strict `cursor.finish()?` (`crates/sophia-protocol/src/ipc/wm_v1.rs:886-896`).
`WmV1SnapshotBegin` carries `chunk_count, output_count, surface_count,
action_count, session_operation_count`; `WmV1ProjectionBegin` carries
`chunk_count, output_count, placement_count, indicator_count, status_count`.
Adding a sixth count changes an existing message layout, which per
`docs/sophia-indicator-descriptor.md` requires a new interface family. The
indicator descriptor is the worked precedent: it landed pre-freeze precisely so
`indicator_count` and `status_count` could be added to `ProjectionBegin`, and that
change moved the `projection_begin` payload from `0x20` to `0x24` bytes.

**An uncounted extension chunk is not.** It is structurally representable today
without touching either `*Begin`, because nothing in the transfer protocol requires
a chunk to map to a declared count:

- The chunk is **self-delimiting**. `crates/sophia-protocol/src/ipc/wm_v1.rs:952`
  derives `data` length from `payload.len().saturating_sub(16)`, not from
  `item_count × record_width`, so a receiver that does not know a record's width
  can still consume the frame correctly.
- Assembly (`wm_v1_records.rs:726-771`) validates
  `begin.chunk_count == end.chunk_count == chunks.len()`, per-chunk connection
  epoch, and dense monotone ordinals, then makes exactly four `require_count`
  calls — one per *known* kind. **Nothing ties the sum of `item_count` across
  chunks to the sum of the declared counts, and nothing requires every chunk to
  contribute to a declared count.**
- `chunk_count` is already dynamic, not structural: `push_snapshot_chunk` and
  `push_projection_chunk` early-return on an empty record slice, so live transfers
  carry between one and four chunks depending on content.

Three constraints bind any extension chunk, and they are permanent:

1. **It must carry at least one item.** Both independent clients reject
   `item_count == 0` (`bindings/c/tests/sophia_wm_v1_client.c:125`, and Hagia's
   `src/sophia/policy_client.nim:354`). It can never be a zero-length probe.
2. **It must append last**, because ordinals must stay dense.
3. **It must be capability-gated outbound**, or a frozen client receives it and
   fails. See the direction rule above.

Two shapes are genuinely unavailable and no amount of care recovers them: new
message kinds die at the envelope for a frozen receiver (`frame.rs:79`), and a
trailing extension region on `*Begin`/`*End` dies at `cursor.finish()`
(`wm_v1.rs:888`, `:1208`) and again at `frame.rs:98` `TrailingBytes`. The uncounted
extension chunk is the only mechanically available escape hatch.

**What this changes about the freeze.** The freeze does not foreclose adding a
WM-side fact later; it makes doing so a deliberate, reviewable relaxation of the
unknown-kind reject arms rather than a forced new interface family. That is why
revision 3 can be frozen confidently. It does **not** rescue enum values: those sit
at fixed offsets inside fixed-width records that a frozen client parses, and no
opaque side channel reaches them. Section 3 remains pre-freeze-only work.

---

## Enumeration

Twenty-eight retained rows across four authority tables. Wire-impact classes:

- **None** — closes inside an authority with no `sophia_wm_v1` change.
- **Enum** — needs a value in an existing enum or bitfield. Pre-freeze only.
- **Record** — needs a new record kind, hence a `*Begin` layout change.
  Pre-freeze only, and the expensive class.
- **Message** — needs a new message kind. Survives the freeze.
- **Separate** — belongs to `sophia_shell_v1`, a broker, or a portal family, not
  to `sophia_wm_v1`.

### Spatial Policy — Hagia

| # | Row | State | Wire impact |
| --- | --- | --- | --- |
| 1 | Stable logical windows, outputs, tags, views, columns, reconciliation | Complete | None |
| 2 | Scrolling columns and fixed-point geometry | Complete | None — retained proportions, centering, movement, gaps, and constraints reduce to `ProjectionPlacement` geometry already on the wire |
| 3 | Tags, workspaces, names, dynamic creation/pruning, occupancy navigation, output affinity | Complete | **Decision 1** — the retained profile uses indicator labels and configures no custom names |
| 4 | Focus, movement, exchange, grouping, histories, cross-output behavior | Complete | None — every checked-in target resolves through opaque `SnapshotAction` tokens |
| 5 | Floating, fullscreen, maximize, minimize, restore, client-visible state | Complete | None for the retained proven states; excluded snapping and rule defaults do not widen `presentation_bits` |
| 6 | Dialogs, transients, popups, scratchpads | Complete | None — `transient_index`/`transient_generation` already carry reduced parent facts |
| 7 | Additional native layouts, frames, tabs, BSP/split trees, grid, layout switching | Complete | **Separate** — the retained five-layout profile is complete; excluded additional layout chrome stays outside WM policy. Layout switching is an opaque action: None |
| 8 | Declarative policy configuration | Complete | None — `PolicyConfiguration` and the profile messages exist |
| 9 | Janet commands and layouts | Excluded | None — sandboxing, determinism, and memory bounds are Hagia-internal; the output is an ordinary projection |
| 10 | Placement, sticky behavior, swallowing, size policy, window rules | Complete | **Decision 2** — trusted one-shot launch placement is capability-gated and committed once; metadata rules remain excluded |
| 11 | Completed and continuous pointer policy interactions | Complete | **Enum**, and see Decision 3; the retained profile uses move and resize only |
| 12 | Checkpoint, crash, reconnect, last-layout preservation | Partial | None — corpus, host, and driver work; Engine already supports the epoch advance |

### Visible Desktop — Hagia Shell

The retained Tier-0 status and generic switcher rows are Complete; overview,
layout chrome, and general overlays are Excluded from this freeze. All five are
**Separate** by construction: they need a least-authority shell endpoint and
target-resolved input, not WM messages. The shell reservation to work-area to WM
projection chain rides `SnapshotOutput`'s existing
`work_x/work_y/work_width/work_height`, so coordinating it costs no
`sophia_wm_v1` change.

One caveat that is *not* a wire risk but is an implementation-coupling risk: the
shell texture-transport question (64 KiB frame cap against a bar frame several
times that size) would touch the shared transport in `sophia-runtime`. The
envelope is role-neutral and each role negotiates its own family, so a shell-role
descriptor extension stays additive. Resolve it during the experimental port
anyway, because it is the only shell question with a back-edge into shared code.

### Session And Dedicated Sophia Authorities

| # | Row | State | Wire impact |
| --- | --- | --- | --- |
| 1 | Physical key, pointer, axis, gesture, switch, shortcut matching | Complete | None — the retained key and move/resize pointer bindings resolve entirely in Engine; axis, gesture, and switch bindings are excluded |
| 2 | Input-device and XKB configuration | Complete | None — the retained startup candidate is session-owned and never crosses policy IPC |
| 3 | Output mode, scale, position, transform, VRR, enablement, power, reservations | Partial | None, **conditional on Decision 4** |
| 4 | Launch, startup environment, configured processes, shell supervision | Complete | None — retained registered launches use opaque session-operation tokens |
| 5 | Lock, logout, session exit, idle inhibition, shortcut inhibition | Excluded | None — clean logout remains retained elsewhere; the security authority is post-freeze |
| 6 | Cursor theme, visibility, inactivity, and find feedback | Excluded from WM policy; theme and size configurable | None on the wire — the Engine still owns the cursor. Theme and size are stated in the desktop profile's `input { cursor { … } }` and applied by the session, which needs no WM vocabulary. Shake-to-find is built: the Engine detects the gesture and the session repaints the cursor buffer. Visibility and inactivity remain unbuilt |
| 7 | Configuration discovery, validation, activation, reload, rollback | Complete | **Message** only for excluded watched reload; retained startup activation and rollback need no new message |

### Brokers And Portals

| # | Row | State | Wire impact |
| --- | --- | --- | --- |
| 1 | Application classification and launch placement | Complete | **Decision 2** — trusted registered-launch provenance emits one opaque class in extension kind `0xFF00` |
| 2 | Window lists and shell-facing descriptors | Complete | **Separate** — the proven generic switcher endpoint stays out of WM policy |
| 3 | Screenshots and capture sessions | Excluded | **Separate** — portal |
| 4 | Clipboard, drag-and-drop, files, notifications | Complete | **Separate** — bounded text clipboard is retained; broader transfers are excluded |

### Result

Twenty-four of twenty-eight rows need no `sophia_wm_v1` change. The four wire
decisions below are settled. The only retained addition is the already-chosen
capability-gated extension-chunk shape for trusted one-shot launch placement;
it does not widen an existing record or enum.

That addition is now implemented. Capability bit 10 gates snapshot extension
kind `0xFF00`; its fixed 16-byte records carry only surface index, surface
generation, and opaque nonzero `u64` classification. The ordinary chunk count
remains unchanged, the extension appends last with a dense ordinal, and the
default-capability stream is byte-identical.

---

## The Four Decisions

### Decision 1 — Where configured workspace and view names live

Hagia implements dynamic workspace creation and pruning, occupancy navigation,
and scratchpads. `setWorkspaceName` exists in its policy state with length
validation but no bound action, and the ledger row still lists configured names
as open.

`ProjectionIndicator` already carries a 32-byte UTF-8 label plus `state_bits` and
an action token, and `ProjectionOutputStatus` carries a 32-byte layout name. A
workspace name is the same shape of fact, and routing it through the indicator
descriptor is blind-safe by construction: policy authors the label, so no broker
sanitization is required.

**Recommendation:** project names as indicator labels; add no field and no record.
This costs nothing and closes the naming half of the row. The only cost is the
32-byte label ceiling, which is already permanent for indicators.

**Settled as recommended.** A workspace name reaches the desktop as a
`ProjectionIndicator` label, and no wire change is required. Two consequences are
worth stating because they are easy to rediscover as bugs:

- **32 bytes is a hard ceiling, and it is UTF-8.** A name is truncated on a
  character boundary, never mid-sequence, and truncation is silent to policy —
  policy authored the label, so there is nobody to report it to. A configuration
  that names workspaces longer than the ceiling is a configuration error caught at
  migration, not a runtime negotiation.
- **A name is not an identity.** Indicators already carry an action token for
  activation; nothing may resolve a workspace *by* its label, or the label stops
  being presentation and becomes a namespace. Hagia's `ViewId` values stay private
  and stay the identity.

This closes only the naming half of its ledger row. Dynamic creation and pruning,
occupancy navigation, and scratchpads are implemented; complete command parity is
what keeps the row partial.

### Decision 2 — How broker-issued classifications reach Hagia

This is the one row that can genuinely force a `*Begin` layout change, and it
governs two ledger rows at once: Spatial 10 and Broker 1.

Hagia must consume opaque broker classifications and never receive title, app ID,
PID, path, or regex input. The question is the shape of what crosses:

- If a classification is **a small closed set of policy classes** — "dialog-like",
  "prefers-floating", "wants-workspace-N" — it fits `SnapshotSurface.kind` or
  spare bits in `capability_bits`. That is an **Enum** change: cheap, pre-freeze,
  no layout impact.
- If a classification is **an expiring grant with its own identity** — a token
  Hagia echoes back, with a generation or expiry — it does not fit.
  `SnapshotSurface` has no reserved space, so it needs either a new snapshot
  record kind (a `*Begin` layout change) or a widened surface record (also a
  layout change).

**This decision must be made before the freeze even though the broker does not
exist yet**, because the freeze forecloses the second option. Deciding the
*shape* now does not require building the broker.

**Settled — and neither option above is what was chosen.** Both were framed before
outbound capability gating existed, when clause 2 of the forward-compatibility rule
was unsound and every server-to-client addition was now-or-never. Gating has since
landed, and reading Triad's actual rule surface showed the first option does not fit
anyway.

#### What the rules actually contain

`WindowRule` at Triad baseline `fb8fb27e`
(`src/types/runtime_values.nim:211-283`) carries roughly thirty-five outcomes. They
do not reduce to a small set of classes:

- **Matching input** — `appIdMatch`, `titleMatch`, `matches`, `excludes`. Never
  crosses; excluding it is the broker's entire purpose.
- **Already expressible** — `minWidth`/`minHeight`/`maxWidth`/`maxHeight` and
  `respectSizeHints` are `SurfaceConstraints.min_size`/`max_size` on the wire today.
  `openFullscreen` and `openMaximized` are `request_state_bits`. `parentedRole` is
  the reduced parent/role fact whose ledger row is already complete.
- **Not WM facts** — `border`, `focusRing`, and `clipToGeometry` are Engine chrome.
  `keyboardShortcutsInhibit`, `idleInhibitMode`, `presentationMode`, `openOverlay`,
  and `openUnmanagedGlobal` belong to session and security authorities. Routing any
  of these through the WM interface would widen it into the general state socket
  the ledger explicitly forbids.
- **Genuinely WM-bound booleans** — `openFloating`, `openFocused`,
  `openMaximizedToEdges`, `openOnAllWorkspaces`, `centerFloating`, `tiledState`,
  `allowSwallow`, `terminal`, `dialogViewportJump`. Around nine, and the list is a
  floor rather than a ceiling.
- **Genuinely WM-bound parameters** — `defaultWorkspace`/`defaultWorkspaces`,
  `openOnOutput`, `defaultColumnWidth`, `scrollerProportion`,
  `scrollerSingleProportion`, `defaultWindowWidth`/`Height`, `openNamedScratchpad`,
  `defaultFloatingPosition`, `maximizePolicy`, `forcedLayout`.

The parameters are what break the first option. `capability_bits` has eleven free
bits, which the booleans alone would nearly exhaust, and a bitfield cannot carry a
workspace number, a column proportion, or a scratchpad name at all. "A small closed
set of policy classes" was an accurate description of a classification vocabulary
and an inaccurate description of these rules.

#### Where classifications live

**In a capability-gated extension chunk, not in `SnapshotSurface`.** A chunk of
`(surface, classification)` records takes a kind from the reserved `0xFF00`–`0xFFFF`
range, is uncounted, and reaches only a client that negotiated the governing
capability. Chunk data is self-delimiting, so it carries parameters as easily as
flags.

This is strictly better than either original option:

- Against the closed set in spare bits: it does not spend the eleven remaining
  `capability_bits`, which stay available for facts that genuinely describe what a
  surface *is* rather than how one host wants it placed. It also imposes no ceiling,
  so a rule family discovered after the freeze is an added chunk record rather than
  an impossibility.
- Against the expiring grant: it costs no `*Begin` layout change, because the chunk
  is uncounted. If per-surface grants with generations later prove necessary, they
  fit the same chunk. The capability that gates it is the revocation mechanism.

`SnapshotSurface` therefore gains nothing for this row, and **nothing needs
reserving in it before the freeze**. That is the substantive change: the pre-freeze
obligation this decision was believed to carry has been removed rather than
satisfied.

#### Pre-freeze obligations resolved

The generator rejects every ordinary record kind in `0xFF00`–`0xFFFF`, and the
trusted registered-launch path now supplies the retained classification without a
general metadata broker. Capability gating, uncounted transfer assembly,
retry/reconnect retention, one-shot commit consumption, and Hagia placement all
have executable regressions. The classification vocabulary remains outside the
frozen record layouts.

### Decision 3 — The continuous-pointer payload

`PolicyInteractionKind` needs `Drag` and `Scroll` values, and
`PolicyInteractionPhase`'s existing `Begin`/`Update`/`Cancel` need
implementations. Both are pre-freeze **Enum** work.

The payload question was better than feared. `ProjectionRequest` already carries
`interaction_x`, `interaction_y`, `interaction_width`, `interaction_height` as
four `i32`s, plus the `u16` that was formerly `reserved_cause`. A scroll axis plus
delta fits those existing fields. The slot is now named `interaction_axis`; the
fixed layout did not change.

The semantic `PolicyRequestCause::Interaction` carries the decoded axis explicitly;
that is an in-process type addition, not a wire-layout addition. No parallel cause
message exists.

Two coupling notes that constrain *when*, not *what*:

- `Cancel` is a lease-revocation contract. Security transitions advance epochs
  and revoke leases and captures, so specify `Cancel` alongside the lock and
  security-authority epoch barrier or it will be specified twice.
- Per-motion `Update` requests are currently dropped: the queue deduplicates by
  pointer-gesture source and returns `Duplicate` for a second request on the same
  surface and mode. Continuous updates need a coalescing rule — replaceable
  latest-value, matching the reduced-continuous-value discipline already ratified
  for target-resolved input — not a queue-capacity increase.

**Settled and implemented at the wire boundary, with live behavior later.** The
wire half landed before the freeze because it is enum widening, which no gate
reaches; values that are not yet emitted do not change live behavior.

Fixed now:

- `PolicyInteractionKind` has `Drag = 3` and `Scroll = 4` beside `Move = 1` and
  `Resize = 2`. Four values is the whole vocabulary — a gesture Sophia later wants
  that is none of these needs a new interface family, not a fifth value.
- The continuous payload rides `interaction_x`/`y`/`width`/`height` with the axis
  discriminant in `interaction_axis`. Scroll uses `interaction_x`/`y` as the delta
  pair and leaves the size fields zero; drag uses all four exactly as move and
  resize do. Horizontal and vertical are 1 and 2; zero is reserved for non-scroll
  geometry interactions.
- `PolicyInteractionPhase` needs nothing: `Begin`, `Update`, `End`, and `Cancel`
  are already wire-reachable and only `End` is constructed.

Deferred, and why that is safe: the coalescing rule and `Cancel`'s revocation
semantics are behavior, not layout. Specifying `Cancel` early would either
duplicate the lock and security-authority epoch barrier or guess at it, and a
guessed revocation contract is worse than a late one. Both remain gated on that
barrier, and neither can force a wire change once the four values above exist.

### Decision 4 — The logical-space contract for outputs

`SnapshotOutput` carries `output, generation, focus, bounds, work_area` and no
scale, transform, mode, or enabled flag. This looks like a missing pre-freeze
field addition and is not:

- Hagia's `Scale` is a fixed-point column-width ratio, not display scaling.
- Policy operates purely in the logical space it is handed via `bounds` and
  `work_area`.
- Rotation can be absorbed by Engine presenting pre-rotated logical bounds.
- Disablement is expressed by omission from the complete snapshot, bounded by the
  permanent 16-output maximum.

**Recommendation:** write this down as a contract rather than leaving it implicit.
The output-authority tranche is the largest remaining chunk of work and the most
likely place to reach for a wider `SnapshotOutput`. Note also that the row demands
reservations and a separate power authority beyond test/apply/rollback, and that
reservations couple to the shell work-area coordinator — so the temptation will be
real. `SnapshotOutput` has no reserved space; widening it is a layout change.

**Settled.** The contract is now normative in `docs/sophia-policy-ipc.md` under
Stable Spatial Semantics: scale, transform, mode, and connector identity never
cross; enablement is expressed by omission; mirroring projects as one output. It
was ratified before the output-authority tranche rather than after, which is the
whole point — the guardrail is worth nothing once the code that needed it exists.

---

## Two Resolved Product Decisions

Both were open when this enumeration was first written. Both are now settled.

1. **The forward-compatibility rule is the three-clause form**, recorded normatively
   in `docs/sophia-policy-ipc.md` under Versioning: the frozen revision is final for
   record layouts and enum vocabularies; new WM-side facts arrive as
   capability-gated extension chunks in reserved kinds `0xFF00`–`0xFFFF`; new
   authorities take new interface families. Receivers keep rejecting unknown kinds,
   because gating guarantees they are never sent one they did not negotiate.

   Its prerequisite was outbound capability gating, which now exists — see the
   direction rule at the top of this file. Clause 2 is therefore sound: an extension
   chunk in the reserved range reaches only a client that negotiated it. What
   remains now-or-never is enum widening, not record addition.

2. **Native output mirroring will be implemented before the freeze.** The
   alternative — rejecting the port obligation while retaining mirroring as a named
   future capability — was considered and not taken.

   The shape is fixed by this document's own boundary: **one logical output backed
   by N connectors**, which is invisible to policy because `SnapshotOutput` carries
   no connector identity, mode, scale, or enabled flag. The rejected shape is two
   logical outputs sharing surfaces, which is inexpressible — it violates
   one-output-per-surface and raises `DuplicateSurface`. So mirroring carries **zero
   `sophia_wm_v1` wire risk** and does not compete for the pre-freeze window; it is
   Engine and output-authority work.

   Two constraints are load-bearing. It requires narrowly amending the ratified
   invariant that presentation is output-scoped with no globally simultaneous
   multi-output retirement instant (`docs/engine-architecture.md`), to joint
   retirement *within* a mirror group and independent retirement *between* groups.
   Each physical head has its own native target, so unequal modes are admitted
   through the Engine-owned fit, cover, or exact transform without widening the
   policy record. The old same-mode-only assumption was retired when distinct
   per-head buffers and explicit mirror projection landed.

   Because joint multi-head retirement changes multi-output and buffer-lifetime
   semantics, it triggers the standing requirement to extend the bounded
   visual-retirement model first. That pulls Milestone 14's first roadmap item
   forward into Milestone 13.
