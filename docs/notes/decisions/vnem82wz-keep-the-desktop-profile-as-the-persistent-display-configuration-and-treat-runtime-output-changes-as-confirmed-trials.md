---
id: vnem82wz
date: 2026-09-30
kind: adr
status: proposed
tags: [adr, output, profile]
---
# Keep the desktop profile as the persistent display configuration and treat runtime output changes as confirmed trials

## Context

The output role now has a file export, a generic SDK client and a reference
consumer that proved it end to end (t253). That raised two questions from
niltempus: whether display settings should change at runtime without editing
the profile, and whether the reference CLI is infrastructure.

Today, Session applies the profile's output layout as its own transaction at
startup and on reload. A reload re-admits output only when the profile's output
values change, so an identical or output-unrelated reload leaves a committed
runtime topology in place. Profile output configuration inherits the current
state for unspecified values, so a partial profile has no independent
restoration target. Runtime state is never written back. Output revision 1
publishes each head's supported transforms and VRR capability, not the settings
in effect, so a client cannot infer an untouched head's current values.

Three desktops were surveyed read-only. niri applies IPC output changes as
transient state and drops them only when the config's output section changes.
Hyprland clears monitor rules during reload after syntax validation succeeds;
an invalid configuration returns before that reset. Gershwin's Display pane applies changes live
through xrandr, asks "Keep this resolution? Auto-revert in 15 seconds" for
resolution changes, and persists only through an explicit Save that writes a
managed block to the X server's configuration, effective on the next restart.
These inspected paths distinguish runtime changes from explicit persistence.
Gershwin's confirmation illustrates the usability problem: a bad mode can leave
the screen unusable. Its client-owned timer is not evidence of recovery after
client death; Sophia's proposed owner timer addresses that separately.

Survey provenance: local source checkouts under `~/src`, read on 2026-09-30;
no desktop was run. The inspected files match their recorded commits:

- niri `5f4469b6a992492cf7221b269e9379f42e737649`, `src/niri.rs:1642`
  (preserve transient output settings when disk output configuration is unchanged).
  The checkout has an unrelated deletion of `docs/uv.lock`.
- Hyprland `7d3a817ffbf1d2df9ef6cf44d0795874a46ace3d`,
  `src/config/lua/ConfigManager.cpp:706` (validation refusal) and `:715`
  (monitor-rule reset).
- gershwin-components `c1d9095217bcae020887a0c93a685304ba863423`,
  `Display/DisplayController.m:674` (mode apply), `:741` (15-second countdown)
  and `:1111` (explicit configuration save).

An earlier scope extension admitted supervised output invocation through 9P
administration (`37c8536f1`). It is deferred, not reverted.

## Decision

1. The desktop profile is the single persistent source of display
   configuration: the desired state, not proof that it reached hardware.
   Session's startup and reload transactions are hardware activation owners.
   They never write the profile.
2. Runtime output-role changes are session-scoped trials: validate-only, or an
   apply through the existing prepare, apply, first-presentation and rollback
   owner. Session never writes a trial back to the profile.
3. An applied trial needs confirmation. A later output contract revision adds
   an explicit Confirm candidate and a nonterminal AwaitingConfirmation event.
   Session's native owner holds one monotonic, bounded deadline that starts at
   the defined usable trial presentation point, not at submission or queued
   admission; earlier preparation and reconciliation keep their own bounds.
   Confirm names the admitted output epoch, the original transaction and the
   tentative topology identity. Only the assigned client may confirm; a stale,
   late, mismatched or replayed confirmation is rejected without any other
   effect. A valid confirmation leads to the one terminal Committed outcome.
   Expiry, an explicit revert, peer death or reassignment enters the existing
   rollback path, which reports RolledBack only after restoration is observed
   and Failed if restoration fails. An acknowledgement is not a confirmation
   and cannot extend the deadline. No second apply is admitted while a trial
   or rollback debt exists. Profile-owned startup and reload transactions need
   no client confirmation. Revision 1 is unchanged; a mandatory safe-trial
   grant refuses clients that do not support confirmation rather than silently
   downgrading them.
4. Keeping a trial means an authorized host tool writes it into the user's
   profile through the existing profile validation and reload owners. The
   tool stages and validates a complete layout, confirms the trial, publishes
   the profile edit atomically, then requests reload. These steps are not one
   transaction: if confirmation succeeds but saving fails, the tool reports
   session-kept but not persisted. If saving succeeds and reload fails, it
   reports profile-saved but reload-not-applied; the next startup may use that
   saved configuration. A
   successful commit is never silently rolled back, Committed never implies
   persistence, and an unconfirmed layout is never written to the profile,
   since a timeout could then revert the hardware while leaving the failed
   layout ready for the next start. The protected output-role child receives
   neither general profile write access nor host-admin access.
5. A persisted layout is complete and restorable: every enabled head with an
   explicit mode, transform, VRR policy and geometry; the disabled or omitted
   heads; the primary output; mirror grouping and mapping; scale where
   supported; and a stable monitor identity. Because revision 1 cannot supply
   current transform or VRR values, completeness comes from explicit user or
   profile intent, never from inference about untouched heads. Omitted-field
   behavior is defined before any reversion promise is made.
6. Reversion targets the previous active topology, not the profile. The
   current reset behavior is retained: a confirmed but unsaved change persists
   across a reload that does not change the profile's output values. That is
   stated prominently wherever trials are documented.
7. Docking layouts keyed by the connected monitor set are a separate task and
   decision, or a client of the output role; they are not required to close
   t253 or t272. Interactive invocation stays with t254.

Naming: the frozen `sophia-output` CLI remains the reference peer that proved
the role. The product-facing tool is a subcommand of the unified command CLI
that t254 specifies, on the `niri msg` and `swaymsg` pattern. Separate sockets
alone do not admit a host process to the supervised output role, and a
subcommand name waives neither checked supervisor identity nor namespace
isolation; a unified CLI may orchestrate separately admitted helpers, and t254
must specify the grant and launch path. The reference peer is not retired
until its replacement meets the applicable proof requirements.

## Alternatives

- Silent persistence, whether by clients or by Session writing runtime changes
  back: rejected. It creates a second source of truth that drifts from the
  profile, and none of the surveyed desktops does it.
- Reset trials on any reload, as Hyprland does: deferred. It needs explicit
  tracking of profile-derived baseline versus trial override so that an
  unchanged reload can request restoration. A successor decision may adopt it
  if lingering confirmed changes prove troublesome.
- A client-side revert timer: rejected. A blank screen or a crashed client
  cannot revert itself.
- A revert timer after the existing Committed outcome: rejected. Commit
  releases rollback resources immediately before policy settlement, so a
  later timer cannot promise restoration. Confirmation needs a new owner
  phase before that point, with the tentative topology visible.
- A privileged profile-write operation offered to the output-role child:
  rejected. It widens a protected role into profile authority.
- Declaring docking layouts in the profile now: deferred to its own task.

## Consequences

- The output files contract needs a revision 2 for the Confirm candidate, the
  AwaitingConfirmation event and the safe-trial grant, with matching SDK and
  reference-client changes. Revision 1 clients keep working and cannot
  receive a trial grant.
- Session needs a confirmation phase before rollback resources are released,
  and the provisional desktop must service input and rendering for the
  confirmation UI while those resources stay retained.
- The profile writer needs its own specification: a caller-selected writable
  source, include ownership, refusal of symlink or ownership ambiguity,
  preservation of unrelated settings, comments and includes, an optimistic
  digest check against concurrent edits, a same-directory temporary file with
  preserved metadata, fsync, rename and directory fsync, and a distinction
  between saved and reload-applied. Atomic rename alone does not prevent lost
  updates. No privileged or general profile-write RPC is required.
- Omitted-field behavior for profile output configuration is defined before
  reversion is promised.
- None of this is part of t253's revision-1 native acceptance or of t272's
  IPC removal. The reference CLI stays frozen at its signed candidate.

## Acceptance and connections

Proposed on 2026-09-30 by niltempus, with the contract qualifications above
supplied by the director's review. Not accepted; no revision-2 behavior exists.

- [t253](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t253--migrate-the-separate-output-role)
  owns the revision-1 output exits this decision does not change.
- [t254](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t254--migrate-administrative-commands)
  owns the unified command CLI and the interactive grant and launch path.
- [Native output records](vkkjmufd-use-native-records-for-the-separate-output-file-role.md)
  is the revision-1 record design this decision builds on.
- [9P as the public interface](1uoozfl8-adopt-9p2000-l-as-the-target-public-interface-while-preserving-authority-boundaries.md)
  owns the authority boundaries that the naming rules preserve.
- [Output file records](../../sophia-output-files.md) and the
  [output authority contract](../../sophia-output-v1.md) carry the current
  revision-1 contract, unchanged here.
- [9P control bus](../../sophia-9p-control-bus.md) owns the administrative
  direction that the subcommand follows.
