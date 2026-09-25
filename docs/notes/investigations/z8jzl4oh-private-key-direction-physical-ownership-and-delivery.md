---
id: z8jzl4oh
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, x11, input, keyboard, private-native, t220]
---
# Private key direction: physical ownership and event delivery

t220's private native key path selects both directions with the press's
masks. This note is the design and red checkpoint admitted on 2026-09-25.

- It traces the delivery and admission architecture.
- It pins X11's rule to the specification and a reference-server revision.
- It records Sophia's recipient contract.
- It proposes a bounded model and contract for review.

It changes no production code, public API, ledger semantics or wire.

Review status. The first review accepted `0b2c4e61` as red and design
evidence, not as a production candidate. Option (c) below is not approved as
the compatibility solution: it freezes release-direction selection at the
press, and it leaves the C1–C3 conflicts and physical state without a
recipient unresolved. No design here is approved. No red control counts as
acceptance, and no existing invariant is changed.

## What the controls do and do not observe

The controls (`crates/sophia-x-authority/tests/support/private_native_key_direction.rs`,
mounted in the private keyboard test module next to the existing private
support mounts) call the native source's `press_key`, `join_key` and
`release_key` under the real common permits.

They observe:
- the ledger outcome;
- XKB physical state and modifiers;
- the QueryKeymap bits;
- activation retirement;
- the hold's native status and residual;
- the **emission the source constructs**: the built key event and the
  encoded frame of the press or release emission.

They do not observe:
- the socket writer;
- a flush;
- recipient-obligation settlement;
- lease settlement.

`Residual::ExternalLease` in the lease control is the correct outcome, but it
means the lease is still owed, not settled.

## What the private path does today

Accepted master `9ee301e7`:

- **Press.** `press_key` finds the applied focus client, or the active
  activation's owner. `prepare_keyboard_press` picks the recipient client
  (the focus, or a grab owner). `commit_key` then resolves one plan with
  `resolve_key_plan` (the window and the core/XI forms), using the press's
  bits: KeyPressMask, `XI_KeyPress` and KeyPress do-not-propagate. The plan
  is resolved before anything is installed. On `NotSelected`, `commit_key`
  returns before any of these:
  - storing the `KeyHold`;
  - binding the recovery ticket;
  - `permit.press`;
  - `map_evdev_key`;
  - `observe_pressed_key`.
  The executor (`private_execution_key.rs`) turns that into
  `StaleExecution` and drops the custody.
- **Join.** `join_key` inherits the retained recipient. It reads no focus,
  selection or grab and leaves XKB alone.
- **Release with a hold.** `release_key` calls `permit.release`. On
  `DeliverTo`, `finish_key_release` releases XKB, clears QueryKeymap,
  updates modifiers, retires the activation and computes residuals. It then
  builds the release emission from `hold.plan`, keeping the press's window
  and forms. A `StateOnly` route builds nothing but marks
  `RecipientTerminationRequired`. The recipient obligation then still needs
  exact termination evidence. It is **not** a precedent for a release that
  owes nothing.
- **Release without a hold.** The ledger answers `NotHeld`. The recorded
  decision owes no event because no aggregate terminal exists. That settles
  no `DeliverTo`.

The ordinary (non-private) path moves XKB and QueryKeymap on every admitted
transition before any selection is read. It routes the release to the
current focus or grab owner and selects the direction at write time.

## Admission boundary

A physical key reaches the X frontend only when the Session admits it
(`sophia-session/src/live_session/input/routing/key.rs`). The following never
become an X route:
- keys consumed by shortcuts;
- keys consumed by launcher and switcher capture;
- protected and reserved actions;
- keys with no Engine focus (`keys_suppressed_no_focus`);
- keys with stale focus.

A key is admitted as `Deliver` to the Engine-focused committed surface. Held
keys are flushed as releases, or cleared `StateOnly`, when focus changes
(`live_session/client_keys.rs`). In the frontend, a publication focus of
`None` is Engine `ClearFocus` (`PrivateAppliedFocusChange::apply` in
`private_applied_state.rs`).

The cases, kept separate:

- **(A) A physical edge Sophia never admitted to X** (Engine, WM, shell,
  protected focus, or no focus). The X frontend must not observe it: no XKB,
  QueryKeymap or device-state update. That is the information boundary, not
  an X device-state defect, and nothing here changes it.
- **(B) An admitted edge with applied focus and a resolved recipient client,
  but no selector for its direction.** This is the XKB/QueryKeymap coupling
  defect the red controls show. It is scoped to admitted edges only.
- **(C) An admitted edge while the X focus publication is pending or
  unpublished.** It is refused before effect by the published-focus rule
  (plan 7xqjn8rp gate B: "Pending focus refuses; applied focus permits"). It
  is not treated as a device-state defect here.
- **(D) An admitted edge while the client's core focus is None or
  PointerRoot.** X discards the event and still updates logical key state
  (`requests:SetInputFocus`: "If None is specified as the focus, all
  keyboard events are discarded"; `requests:QueryKeymap` reports the logical
  state). How the private publication represents core None or PointerRoot
  was **not traced** in this checkpoint, so whether (D) exists as a distinct
  admitted state is open.

## Gaps (admitted edges, case B)

- **R1.** The release is built for the press's recipient even when that
  client did not select KeyRelease. It never reaches a KeyRelease selector
  that is not the press's window.
- **R2.** KeyRelease do-not-propagate (bit 1) is never consulted. KeyPress
  do-not-propagate also stops the release.
- **R3, corrected.** Core keyboard grabs report both transitions whatever the
  client selected, so a grab's mask is not a per-direction filter:
  - `GrabKeyboard` and `GrabKey` carry no event mask.
  - Sophia builds both with `event_mask: 3` (`dispatch/core/grabs.rs`) and
    refuses XI2 keyboard device grabs (`dispatch/extensions/xi.rs`,
    `device_id != 2`).
  - What remains of R3 is owner_events: the owner's normal delivery must be
    tried per direction before the grab window.
  The first checkpoint's press-only grab-mask controls modelled a state no
  request can create. They are replaced by guards.
- **R4.** A KeyRelease-only selector (core bit 1 or `XI_KeyRelease`) never
  gets its release: the press refuses, so no hold exists.
- **Physical coupling.** For an admitted edge, a key nobody selected never
  enters XKB or QueryKeymap. An unselected Shift leaves the next selected key
  without its modifier. Any task proposal goes to the director; this note
  adds no ledger row.
- **Observation, outside this slice.** The XKB StateNotify is a frame of the
  key's own emission, so an unselected key also withholds it from XKB state
  selectors.

## X11's rule, pinned

**Primary specification.** *X Window System Protocol*, X11R7.7
(`https://xorg.freedesktop.org/archive/X11R7.7/doc/xproto/x11protocol.html`,
fetched 2026-09-25, sha256 prefix `ef66b4aef805ddbf`):

- `#events:input:source`: "The event window is found by starting with the
  source window and looking up the hierarchy for the first window on which
  any client has selected interest in the event (provided no intervening
  window prohibits event generation by including the event type in its
  do-not-propagate-mask). The actual window used for reporting can be
  modified by active grabs and, in the case of keyboard events, can be
  modified by the focus window."
- `#events:KeyPress` (shared KeyPress/KeyRelease entry): "KeyPress and
  KeyRelease are generated for all keys, even those mapped to modifier
  bits."
- `#requests:GrabKeyboard`: "If owner-events is False, all generated key
  events are reported with respect to grab-window. If owner-events is True
  and if a generated key event would normally be reported to this client, it
  is reported normally. Otherwise, the event is reported with respect to the
  grab-window. Both KeyPress and KeyRelease events are always reported,
  independent of any event selection made by the client."
- `#requests:GrabKey`: "The active grab is terminated automatically when the
  logical state of the keyboard has the specified key released."
- `#requests:SetInputFocus`: "If a generated keyboard event would normally
  be reported to this window or one of its inferiors, the event is reported
  normally. Otherwise, the event is reported with respect to the focus
  window." Also: "If None is specified as the focus, all keyboard events are
  discarded."

**Reference server.** XLibre xserver `9ba1d707b8770409a5e061d7120aef3f76be7723`
(`xlibre-xserver-25.2.0-490-g9ba1d707b`, the local checkout
`~/src/xserver`). The claims below are for this revision only.

| Function and location | Behaviour |
| --- | --- |
| `Xext/xinput/exevents.c` `UpdateDeviceState` (871; press 942–947, release 953–957) | Sets the key down or up (`KEY_PROCESSED`) before any grab or delivery. A release of a key that is not down is `DONT_PROCESS` (discarded). |
| `Xext/xinput/exevents.c` `ProcessDeviceEvent` (1837; 1906–1951) | A press checks passive grabs. A release of the activating key under a passive keyboard grab sets `deactivateDeviceGrab`. Delivery happens at event time: `DeliverGrabbedEvent` if grabbed, else `DeliverFocusedEvent`. |
| `dix/events.c` `EventIsDeliverable` (2765) | The core, XI and XI2 selection checks and the core do-not-propagate check each use the filter of this event's own type, so do-not-propagate is per direction. |
| `dix/events.c` `DeliverDeviceEvents` (2884) | Walks from the window up, XI2 then XI then core. It stops at the first delivery, at `stopAt` (the focus), or at do-not-propagate. |
| `dix/events.c` `DeliverFocusedEvent` (4197) | If the sprite window is the focus or inside it, walks with `stopAt = focus`. If nothing was delivered, it **delivers to the focus window itself** ("just deliver it to the focus window"). |
| `dix/events.c` `DeliverGrabbedEvent` (4354) | With owner-events, tries the owner's normal delivery (from the sprite window, stopping at focus) first. Otherwise, or failing that, uses `DeliverOneGrabbedEvent` with the grab's mask. |
| `dix/events.c` `ProcGrabKeyboard` (mask at 5286), `ProcGrabKey` (mask at 5678) | `mask.core = KeyPressMask \| KeyReleaseMask`. |

**Correction of the first checkpoint.** The earlier observation that the
reference server does not retry the focus after a do-not-propagate stop was
wrong. `DeliverFocusedEvent` falls back to the focus window whenever the walk
delivered nothing. Sophia's shared `x_key_delivery_target` retry matches it.
The per-direction DNP controls nest a leaf under a selecting window, so they
exercise the stop without the retry.

## Sophia's contract (unchanged)

- **Ledger:**
  - `DeliverTo` is owed to the first press's recipient.
  - `HoldIncarnation` carries that recipient and its generation.
  - The barrier refuses a new press for the same recipient and generation
    while an old record's native or recipient obligation is open.
  - Native reconciliation and recipient transport settle separately.
- **Plan 7xqjn8rp:** the last release targets the recorded recipient; focus
  changes cannot turn cleanup into a new action; no late reselection or
  restamp; `Flushed` settles transport only; proven termination settles the
  recipient; missing, rejected, failed or timed-out routes retain debt.
- **InputAuthorityArbitration:** routes use the presented choice; security
  transitions revoke delivery without acknowledgement.
- **InputDeliveryRecovery:** NoFalseFlush, ExactlyOneTerminal,
  NoAcceptedObligationLost and BarrierSound.

## Candidate designs considered

- **(a) Bind the prepared client, resolve the window late.** This breaks
  "no late reselection" and "release keeps its target and forms", and still
  diverges from X across clients.
- **(b) A recipient-less hold, resolved at release.** This breaks the
  first-recipient rule, the incarnation, the barrier, bind-before-effect and
  "cleanup cannot become a new action". It would also hand a key's release
  timing to a client that never saw the press. Not proposed.
- **(c) Two direction plans frozen at the press.** Not approved. It keeps
  the invariants but freezes release selection at the press, leaves C1–C3,
  and gives no answer for physical state without a recipient.

## Bounded model and contract proposal (for review)

The proposal separates three things the private path currently fuses into one
`KeyHold`.

### 1. Native physical recognition

- **Scope.** Only an **admitted** edge (case B, and D if it exists).
  Unadmitted physical activity (A) stays invisible to the X frontend.
- **What it records.** The frontend-seat key edge: the ledger aggregate,
  XKB and QueryKeymap. It records them whether or not any event is
  authorized.
- **Obligation.** It carries a native obligation only, settled by native
  reconciliation under the guard. It carries no recipient transport
  obligation, so no transport receipt, flush or termination is recorded for
  it.
- **Open question for the ledger.** The ledger mints a recipient identity
  on every press, so this layer needs either a ledger record kind without a
  recipient or a proof that the existing record may carry one without owing
  it. That is a ledger-semantics decision for the director.

### 2. Authorization of an ordinary edge

- **Timing.** Each edge, press and release alike, is authorized at its
  **own** guarded execution, using the X rule above: the current applied
  focus, pointer, grab and owner_events, with the direction's selection and
  do-not-propagate.
- **Result.** Zero or one ordinary delivery obligation. When there is one,
  it is bound before effect to an exact connection and generation, with its
  own recovery ticket, as today.
- **An edge that authorizes nothing needs a distinct recovery terminal.**
  Call it `Unselected`: neither `Flushed`, termination nor failure. It is
  recorded under the same guard, with source-authenticated evidence of the
  decision: the selection revision, the focus publication and the grab
  state that were read. `NotHeld`, `SurvivorRemains` and the pointer's
  `reaches == false` are **not** that evidence. The latter is a bind that
  answered `Ended`/`Unknown` for a recipient, not proof that a live
  recipient had no selector.
- **Retirement.** Attempts and the barrier retire exactly as for a
  delivered edge.

### 3. Retained recipient cleanup

- **When it exists.** Only when an ordinary press event was actually
  constructed for a recipient. The obligation then stays with that exact
  recipient and generation (the existing `DeliverTo` invariant) and is never
  rerouted.
- **Settlement.** It settles by one of:
  - the release edge's own ordinary delivery, when that delivery's
    authorization names the same recipient and generation (one write
    settles both);
  - proven termination of the recipient;
  - a director-approved `NothingOwed` outcome. That requires
    source-authenticated exact evidence that the retained recipient (same
    generation) had no selector for the release at the release's own
    execution. It requires its own recovery terminal semantics and the
    retirement of the barrier and attempts. A false `owes_event` bit is
    not enough.
- **When the release goes elsewhere.** If the release edge authorizes a
  different recipient (focus or grab moved, C2/C3), or nobody while the
  retained recipient still selects KeyRelease, the cleanup obligation
  remains. Its disposition is the one explicit divergence from X that this
  proposal leaves.

No blanket reroute happens at release time: layer 2 creates new obligations
at the current authorization, and layer 3 never moves.

### Model before production

This is a new bounded TLA+ module (proposed name
`KeyEdgeAuthorization`), or an extension of InputDeliveryRecovery, with:

- **Bounds:** two clients, three windows, one key, two sources and two
  connection generations.
- **Actions:** admission and withholding of edges; changes to focus,
  pointer, selection and grab; press and release authorization; writing,
  flushing and termination; disconnect and replacement; security
  revocation.
- **Invariants:**
  - `NoUnadmittedObservation`;
  - `PhysicalStateMatchesAdmittedEdges`;
  - `CleanupRecipientImmutable`;
  - `NoFalseFlush`;
  - `NoFalseTermination`;
  - `NothingOwedRequiresAuthenticatedDecision`;
  - `ExactlyOneTerminal` per ticket;
  - `BarrierSound`;
  - `NoOrphanNativeDebt`.
- **Liveness:** every native record reconciles, and every ticket reaches a
  terminal.
- **Mutations that must fail:** settling cleanup through `owes_event`
  alone; rerouting cleanup to the current focus; observing an unadmitted
  edge; accepting a bind's `Ended` as `Unselected`.

### Decisions for the director

1. Whether layer 1 may exist as a ledger record without a recipient, and its
   capacity accounting.
2. The `Unselected` terminal, and the evidence required for it.
3. The disposition of retained cleanup when the release is authorized
   elsewhere. The choices are: a cleanup release to the old recipient (a
   Sophia divergence from X), or `NothingOwed`. `NothingOwed` applies only
   with authenticated evidence that the retained recipient deselected.
4. Cross-client release delivery after a focus or grab move (X delivers to
   the new target). This is a key-timing disclosure the retained model
   currently prevents.
5. Case (D): trace the core None/PointerRoot publication first.

## Conflicts with X, under today's code and under the proposal

| Conflict | Today | Proposal |
| --- | --- | --- |
| C1: selection change between press and release | Release uses the press plan | Layer 2 follows X; cleanup disposition is decision 3 |
| C2: pointer or focus change | Press plan and recipient | Layer 2 follows X; cross-client is decision 4; cleanup is decision 3 |
| C3: grab taken or ended | Press plan | Layer 2 follows X; cleanup is decision 3 |
| C4: recipient departure or replacement | Emission kept for the original; replacement refused while owed | Unchanged |

Existing guards that encode today's coupling:
- `native_key_unpublished_focus_and_missing_selection_refuse_before_xkb`
  asserts that an unselected press leaves XKB released.
- `native_key_normal_routing_stops_at_focus_and_honors_pointer_descendants`
  asserts, in cases 1, 2 and 4, `NotSelected` with the key released.
- `native_key_emission_survives_selection_changes_and_release_uses_current_geometry`
  and `native_key_xi_selection_uses_exact_coordinates_and_does_not_emit_core`
  deselect after the press and still expect the release (C1).

Any change to these needs the director's approval of the corresponding
decision; none is changed here.

## Red controls

| Control | Covers | On `9ee301e7` |
| --- | --- | --- |
| `t220_key_press_only_selection_settles_the_release_without_an_event` | R1 | red: release built |
| `t220_key_release_only_selection_owns_the_press_and_receives_the_release` | R4, physical ownership | red: press `NotSelected` |
| `t220_key_unselected_modifier_still_reaches_xkb_and_query_keymap` | coupling, QueryKeymap, final release | red: press `NotSelected` |
| `t220_key_unselected_join_survivor_and_final_release` | join, survivor, final release | red: press `NotSelected` |
| `t220_key_press_and_release_propagate_to_their_own_selectors` | R1 propagation | red: release at the child |
| `t220_key_release_do_not_propagate_stops_only_the_release` | R2 | red: release built |
| `t220_key_press_do_not_propagate_leaves_the_release_selected` | R2 | red: press `NotSelected` |
| `t220_key_xi_release_only_selection_receives_only_the_xi_release` | R4, XI2 | red: press `NotSelected` |
| `t220_key_owner_events_grab_resolves_each_direction_separately` | R3 owner_events | red: release at the grab window (root) |
| `t220_key_route_lease_remains_a_residual_when_no_release_is_built` | lease residual | red: release built (`ExternalLease` retained first) |
| `t220_key_core_grab_reports_the_release_without_any_selection` | GrabKeyboard, both directions | green (guard) |
| `t220_key_passive_grab_reports_the_release_and_retires_without_selection` | GrabKey, retirement | green (guard) |
| `t220_key_departed_grab_owner_keeps_the_release_with_its_recipient` | departure, no reroute | green (guard) |
| `t220_key_recipient_replacement_is_refused_while_the_release_is_owed` | generation replacement | green (guard) |
| `conflict_t220_key_release_deselected_after_the_press_is_not_written` | C1 (X) | red: release built |
| `conflict_t220_key_release_selected_after_the_press_is_written` | C1 (X) | green: inherited plan |
| `conflict_t220_key_release_follows_the_pointer_at_release_time` | C2 (X) | red: release at the child |

Not covered: the departure of an **ungrabbed** focus recipient (it needs the
recipient-termination fixture), and case (D).

## Red evidence

- **Branch:** `input/t220-key-direction` (parent `9ee301e7`), with only tests
  and this note changed.
- **Isolation:** device-hidden bwrap, nice 19, jobs 2 and an on-disk target.
- **Command:** `cargo test -p sophia-x-authority --lib -- t220_key`.
- **Current run** (run 5): 5 passed and 12 failed, each at its intended
  assertion.
  - **Logs:** `.artifacts/t220-key-direction/red-run-5.log`, with each
    reason in `red-reasons.txt`.
  - **Release side (7):** the protocol assertion is reached only after the
    physical assertions pass (`DeliverTo`, XKB released, QueryKeymap clear
    and `NativeReconciled`, or `ExternalLease` retained).
  - **Press side (5):** refused as `NotSelected` before any physical effect.
- **Earlier runs:**
  - Runs 1–4 back the first checkpoint `0b2c4e61`. They fixed fixture errors
    only.
  - Run 4's three press-only grab-mask reds are withdrawn as unreachable
    (R3, corrected).
