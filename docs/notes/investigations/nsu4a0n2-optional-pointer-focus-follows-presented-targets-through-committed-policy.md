---
id: nsu4a0n2
date: 2026-09-11
kind: investigation
status: accepted
tags: [policy, input, session]
---
# Optional pointer focus follows presented targets through committed policy

## Scope and acceptance

User-selected on 2026-09-11: follow niri's disabled-by-default convention and
provide a fully tested Hagia `policy { focus-follows-mouse #true }` setting.
Omission and explicit false retain click and keyboard activation. When enabled,
physical pointer motion may focus an eligible presented window or activate a
monitor with no window beneath the pointer. No input or rendering authority
moves into Hagia.

The implementation must validate configuration and independent wire encoding,
retain default behavior, resolve targets from presented root geometry, suppress
hover focus under captures and grabs, preserve input ordering, and change public
focus only after Engine commit. Configuration replacement and checkpoint recovery
must use the current setting; rejected or timed-out candidates retain prior focus.
Deterministic tests cover these boundaries and launch/picker ordering. Installed
physical acceptance remains a separate gate.

## Evidence

Sophia `58ed7f7a` with Hagia `24f3203` is installed and running (owner PID 4545).
The user confirmed app launches and picker-launched Ghostty on DP-1. Moving the
pointer to empty DP-2 and pressing Super+Enter retained DP-1, consistent with
current click/keyboard activation. This observation does not reject the prior
launch-focus fix. DP-1 is the large left monitor; DP-2 is the smaller right.

## Design and validation

Implemented with Hagia `d5eb25cd3ba606192a470849e27236120747198a` and the
paired Sophia change based on `58ed7f7a`. The policy setting belongs to Hagia;
Sophia reduces presented pointer targets into bounded opaque policy causes.
Negotiated capability bit 13 gates cause kind 4 at the actual socket writer,
and disabled profiles do not request the capability. Raw input, coordinates,
device identity and application metadata stay outside Hagia's policy interface.

A bounded ordered input queue holds later shortcuts behind a hover settlement;
only adjacent hover observations coalesce. Replacement clears queued work.
Eligibility comes from the pointer output's retired projection, with no primary
fallback for an unpresented output. Captures, grabs and popups retain their
existing input ownership. Rejected observations may be retried by fresh motion.
Hagia checkpoint version 13 carries the setting; older checkpoints migrate off,
and a replacement profile overrides the restored setting.

Retained evidence is in `.artifacts/t078-pointer-focus/`:

- `check.log`: complete Sophia `cargo xtask check`, exit 0; 3,053 Rust tests,
  29 intentional ignores, archive checks and host pixel proofs passed.
- Two additional queue/input-order tests passed in the final eight-test focused
  pointer suite after that full run. The strengthened launcher capture test also
  passed with a nonzero presentation epoch, so capture itself is tested.
- `hagia-verify-final.log`: final `nimble verify`, exit 0, including 207 Hagia
  cases, formatting/data-layout checks, Alloy/Z3/TLA+ checks, real pregraphics
  admission, and both paired pointer-focus tests. The gate verifies test names
  before execution so a filter matching zero tests cannot pass silently.
- `hagia-paired-final.log`: both compiled-Hagia socket cases passed. They cover
  enabled/disabled negotiation, real empty-output and window-target requests,
  Engine stage/commit, timeout discard and retry, and an old server refusing the
  enabled setting before configuration with an explicit diagnostic.
- `clippy-final.log`: final affected-crate all-feature/all-target check, exit 0.

The protocol generator had a pre-existing shell revision-3 assertion while the
schema already described revision 4. Updating that check and its expected nine
revision-4 messages was necessary to regenerate the new WM capability. Schema,
Rust and C constants are synchronized.

Claude implemented and tested Hagia through the user-authorized Herdr helper and
reviewed Sophia. Review confirmed launcher interception precedes hover emission,
configuration is mandatory in this live policy owner, and complete projections
remain bounded. No unresolved blocking review finding remains.

## Remaining physical gate

At implementation handoff the running desktop used Sophia `58ed7f7a` and Hagia
`24f3203`; installation and personal-profile activation were still pending.
The physical gate requires default-off click/keyboard behavior and enabled
hover between windows and onto empty DP-2; immediate terminal/browser/picker
shortcuts must use the committed active output. Confirm captures and window
movement remain usable, then reload with the setting off and confirm it stops
following the pointer. Keep t078 open until these installed checks are accepted.
This is separate from the remaining reload-geometry and expanded-window checks.

## 2026-09-11 installed acceptance in progress

The user installed Sophia `3085149116ef418d0aefe563a1b1d9b69d9fe25f`
with Hagia `36bbb9453f625f692369c7fa3c231c5471f0f252` and entered session
`00000001789138761895-b8f5b4e5-9487-4ec5-9919-6783dae91c45`.
Live owner PID 16982, Hagia PID 16987, and Narthex PID 17026 match the
packaged binaries. Hagia SHA256 is
`92c24358b00158b46bb1df5cc32d1751ae5763c709a0a89b097b8be895ace52e`;
Narthex SHA256 is
`1a22f1212667de6849d82a63acf5967f1a61afc485d60bd93d7d8034302074f1`.
Baseline evidence is retained in `.artifacts/installed-acceptance-30851491/`.

With the setting omitted, the user clicked a left-monitor terminal, moved the
pointer to the empty right monitor without clicking, and pressed Super+Enter.
Kitty remained on the left, accepting this default-off case. The second
click/window test could not proceed because the right workspace was empty.
The checkpoint confirms that both outputs already have three independent views:
left views 1–3, right views 4–6, with local workspace slots 1–3 on each.
The personal profile lacked monitor-focus bindings; background clicks without
a surface currently do not activate an output.

The next test profile enables `focus-follows-mouse #true` and binds
Super+Ctrl+Alt+Left/Right to `focus-output-prev/next`. Both Sophia's desktop
envelope check and Hagia's extracted-policy check pass. The existing
Super+Alt+Left/Right column-movement bindings are preserved. Enabled behavior,
captures, and disabling through reload still require user observation.

The user reloaded the enabled profile and confirmed that moving onto empty
DP-2 followed by Super+Enter opens Kitty on DP-2, and that focus follows the
mouse. The committed checkpoint has `focusFollowsMouse=true` and window 7
assigned to logical output 2. Identity records 100880–100881 show WM epoch 2,
profile generation 2 activation, and desktop/launch generation 2 publication.
The enabled personal-profile SHA256 is
`6ab0a40dece42d69f00349eee0242fa90a0ff095d68d221dbc2f28c428355c4c`.
The `hover-enabled-*` snapshots and `hover-enable-reload-events.log` retain
this evidence beside the baseline. Picker capture, workspace independence,
reload geometry, drag behavior, and disabling remain to be checked.

The user subsequently accepted picker capture across outputs: the picker opened
on the right, retained typing after the pointer moved left, and launched btop
on the right. Super+2 then Super+1 on the right changed only that monitor's
workspace and restored Kitty. The user also confirmed that reload preserved
window geometry without glitches or duplicate startup windows. This supplies
the separate t001 reload acceptance; browser placement/input, floating-window
drag/resize, and disabling through reload remain for t078.

## Super-drag crash during installed acceptance

The next physical check failed. The user pressed Super+T on Kitty, then
Super+left-drag, and the session exited. The exact post-toggle floating state
was not captured; the interaction began on surface 56623118 on DP-1. This
rejects the drag portion of t078 and prevents closing it. The prior clean t001
reload observation remains valid for its separate workflow.

The complete available failed-session journal is retained in
`.artifacts/t078-drag-crash/`. Health reports zero discarded records and storage
errors, with final sequence 128881. Records 128850–128855 show the pointer
gesture and policy response; transaction 54 reconciles six DP-1 placements,
then record 128862 reports an owner-loop fatal error. Record 128871 identifies
`phase=window_management failure_code=unclassified`. Cleanup drained native
work, stopped presentation, restored terminal state, and returned to the
display manager with exit status 1. The transient checkpoint was removed by
cleanup; the earlier retained snapshots are not a claim about post-toggle state.

The code regression reproduces a concrete failure at this boundary:
`public WM projection has no reconciled content placement`. A pointer request
names one affected output, and real Hagia answers only for that output, as the
protocol requires. Sophia reconciles fresh content only for those placements,
but `StagedPolicyProjection.projections()` includes the untouched outputs too.
Materialization incorrectly demanded fresh content for their surfaces and
failed when the other monitor was populated. Claude independently confirmed
the request/response scope. The journal's omitted raw error prevents recovering
the exact string from the physical session; its placement sequence matches the
reproduced path. This is separate from t077's older control-phase incident.

The repair passes the complete reconciliation record to materialization so it
can distinguish updated outputs from retained outputs. Updated outputs still
require freshly reconciled content. Untouched visible surfaces retain their
committed content geometry, raster, crop, transform, and translation; they do
not cross chrome clearance again or replay prior size requests. Missing
committed content still fails closed. New approved diagnostic codes distinguish
`wm_missing_reconciled_content` and `wm_missing_retained_content` without exposing
arbitrary error text.

The failing-then-passing reducer/materialization regression covers both outputs,
move and resize, and begin/update/end/cancel phases. It checks retained layers,
focus, commit, and explicit failure for genuinely missing content. A real-Hagia
socket regression and the full contributor gate are required before packaging.
Installed Super+drag acceptance and the enabled-to-disabled reload check remain
open; no physical fix is claimed from offline tests.

The final `cargo xtask check` passed with inherited `SOPHIA_*` and `HAGIA_*`
variables removed; `.artifacts/t078-drag-crash/check.log` retains the full gate,
including strict Clippy, archive verifiers, buffer-age equivalence, and GLX/EGL
pixel checks. `paired-hagia.log` records the separately enabled real-Hagia
regression: one test executed against the exact installed binary above, with
eleven settled socket cycles covering both populated outputs, move/resize
begin/end, cross-output activation, and full SceneChanged replacement. Every
cycle materializes and installs the reconciled result. Untouched content is
identical and receives no extra size request. All 19 diagnostic tests pass,
including approved failure-code retention and payload redaction.

The Sophia candidate is the signed commit containing this repair and record;
Hagia remains `36bbb9453f625f692369c7fa3c231c5471f0f252`. No Hagia source
change or new wire behavior was needed. Final review confirmed minimized
placements remain omitted, untouched translations survive, and relative stack
order is preserved. An admission-race concern was withdrawn after checking that
new unplaced surfaces have no committed output and cannot reach the retained
branch; genuinely missing committed layers continue to fail closed.

## Repaired installed drag retest

The user installed signed Sophia `0c069d2f3a232224dfdde3c20ba82e1cd78ee0fe`
and entered session
`00000001789143018118-8259844a-396f-469e-928d-0bec2bd54d00`. Live owner
PID 30405 has SHA256
`7ebd31ed8beacb73fe9e1de126d72665069f717071e0d36e4efb04f4386469b3`;
Hagia PID 30410 and Narthex PID 30451 match the unchanged packaged hashes
above. The enabled personal profile still has SHA256
`6ab0a40dece42d69f00349eee0242fa90a0ff095d68d221dbc2f28c428355c4c`.

The user confirmed moving and resizing the floating Kitty, then pressing
Super+T again to return it to the scrolling layout. The checkpoint retains
window 2's manual rectangle `(898,293,1089,687)` and `floating=false`.
At inspection both windows belonged to output 1 and output 2 was empty, so
this accepts the basic gesture/retile cycle but does not yet revalidate the
original two-populated-output trigger. That check and the browser placement/
input check were requested next. Disabling pointer focus through reload also
remains pending.

Evidence is retained under `.artifacts/t078-drag-crash/installed-retest/`:
identity, manifest, health, lifecycle, the post-gesture checkpoint, and pointer
events. At capture the journal was running with zero discarded records or
storage errors and no owner-loop fatal record.

## Empty-monitor arrow navigation

In the repaired session the user subsequently reported that Super+arrows lost
camera/focus control and clicking a window recovered it. The pointer location
and exact direction sequence were not remembered. The post-click snapshot
shows output 1 active with four tiled windows and output 2 empty; it cannot
establish which output was active during the failure. The checkpoint and
input/focus journal slice are retained as `after-navigation-report.checkpoint`
and `navigation-report-events.log` in the installed-retest directory.

Source inspection found an independent trap in Hagia `36bbb94`:
`focusColumnRelative` hands off to the adjacent monitor when navigation steps
past the last column, but returns immediately when the active output has zero
visible columns. Stepping into an empty monitor therefore cannot be reversed
with the opposite arrow. This is a concrete candidate for the report, not a
claim that the user remembered or reproduced that exact sequence. Explicit
`focus-output-prev/next` bindings remain a way back. A focused reproduction and
repair of empty-output directional handoff are being validated separately from
the accepted basic move/resize/retile cycle.

Hagia's signed repair is `9349e57ebd56068bb47822b4da11f6548ea2beda`.
The reproduction failed before the change in both directions and across an
empty middle monitor. One helper now handles both empty strips and populated
strip edges, reads the destination through the state query, and changes the
active output through the entity operation. Existing destination focus is
preserved. Six new regressions cover populated/empty round trips, a middle
empty monitor, two empty monitors, non-default workspace/focus preservation,
and populated/empty single-monitor edges.

The full `nimble verify` passed after review, including eleven navigation
tests, the policy model and independent protocol suites, paired Sophia tests,
formatting/layout checks, and Alloy/Z3/TLA+ checks. Evidence is retained in
`.artifacts/t078-empty-output/hagia-verify.log`. The release build and current
extracted-policy validation also passed. Its executable SHA256 is
`7b9358d61f6ad9da6e5255c9e3ba978d7ba4410f80c3982d488c1d80fd429e88`.

The replacement was atomically staged at the user's configured
`~/.local/state/sophia/bin/hagia` path, with the previous binary retained in
the artifact directory. Sophia remains installed `0c069d2f`; no process was
reloaded. Activation of the replacement and a keyboard round trip into the empty
right monitor and back remain required. This repairs a demonstrated defect,
without claiming the user's unremembered navigation sequence had this cause.

Before that reload occurred, the [maximized navigation repair](3asecq4a-maximized-windows-obscure-keyboard-navigation-targets.md)
produced signed Hagia `43cfcae0ac7481e7032962ff45be0cb45fe0d7ef`, which
includes the empty-output fix. Its verified release replaces `9349e57` at the
configured binary path. One user-triggered restart can activate both repairs;
neither physical navigation result is inferred from staging the binary.
The initial Ctrl+Alt+R instruction was incorrect for an unchanged profile: the
subsequent process inspection still found the old executable. The linked
investigation records that activation correction and the subsequent discovery
that Ctrl+Alt+F5 was intercepted for virtual-terminal switching. The replacement
restart binding is Ctrl+Alt+Shift+R, after Ctrl+Alt+R loads the edited profile.

## Installed acceptance completed — 2026-10-07

On the installed release `niltempus-44f76a7d05b2b8264a63` (Sophia `825d9146`,
Hagia `155daab6`, focus-follows-mouse on in its profile), on two monitors,
niltempus confirmed the three checks this note still owed. A new browser
window landed where expected and took typing. Super-dragging and resizing a
floating window with windows on both monitors, the original crash trigger,
left the session up. Keyboard focus moved into the right monitor and back. The
session's recovery log stayed empty with no fatal record. The record is
`attended-closures-01/RESULTS.json` in the development evidence.

Disabling through reload was not repeated. This release renders its profile
into a root-owned directory and reads nothing from the user's state, so the
setting cannot be turned off there. niltempus accepted the default-off
behaviour recorded on 2026-09-11 as covering it (decision of 2026-10-06).
t078 is closed.

## Related repair

The preceding [launch and pointer repair](v4geoq2j-policy-reload-compares-independent-configuration-generations.md)
records t001's independently accepted reload result. Expanded-window task t004
still needs its own physical acceptance.
