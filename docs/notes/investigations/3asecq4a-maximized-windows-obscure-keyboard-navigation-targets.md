---
id: 3asecq4a
date: 2026-09-11
kind: investigation
status: resolved
tags: [policy, validation]
---
# Maximized windows obscure keyboard navigation targets

## Report and candidate

During installed acceptance on 2026-09-11, the user reported that Super+navigation
stops working in Super+F mode. The personal profile binds Super+F to
`toggle-maximized`; Super+Shift+F is the separate fullscreen toggle. Super+Left
and Right select columns, while Up and Down select windows within a column.

The running session is
`00000001789143018118-8259844a-396f-469e-928d-0bec2bd54d00`, with installed
Sophia `0c069d2f3a232224dfdde3c20ba82e1cd78ee0fe` and Hagia
`36bbb9453f625f692369c7fa3c231c5471f0f252`. The process executable hashes are
`7ebd31ed8beacb73fe9e1de126d72665069f717071e0d36e4efb04f4386469b3`
and `92c24358b00158b46bb1df5cc32d1751ae5763c709a0a89b097b8be895ace52e`,
respectively. The profile hash remains
`6ab0a40dece42d69f00349eee0242fa90a0ff095d68d221dbc2f28c428355c4c`,
with pointer focus enabled. Hagia `9349e57` is staged on disk but has not been
reloaded; the old process still holds the replaced executable.

The checkpoint inspected after the report has four tiled windows on the large
left output, none maximized or fullscreen, and an empty right output. It is
not a capture of the reported failure. A held reproduction from the first
column was requested to distinguish a hidden focus target from an output-edge
handoff. No physical cause is inferred from the post-report checkpoint alone.

## Source finding

Hagia's column navigation includes maximized tiled windows and can select the
next ordinary column. Projection separately expands every maximized window to
the work area and places it above every ordinary window, regardless of which
window now has focus. A normal navigation target can therefore receive focus
underneath the maximized window. This is a policy projection defect; it does
not require changing Sophia's input routing or window metadata boundary.

The original stacking repair remains necessary: enlarging an earlier tile
without raising it lets a later neighbor cover it. The first repair attempted to make
the selected window visible while retaining maximized geometry and state,
preserving parent/dialog families, and retaining fullscreen ordering. Ordinary
tile order must still return when maximization is toggled off.

## Initial stacking-only repair (superseded)

The initial offline regression on Hagia `9349e57` reproduced six ordinary
target ordering failures across first/middle/last maximized columns, plus a
focused dialog family below an unrelated maximized window. The artifact
`.artifacts/t004-maximized-navigation/regression-before-fix.txt` retains the
failure output. Its fullscreen exception was a test fixture missing the
fullscreen capability, not a production defect; that fixture was corrected
before the final gate.

The repair retains the expansion layers, then finds the root of the focused
window's visible parent chain with the existing family-depth bound. When a
maximized placement is present, that family moves above background maximized
families and below unrelated fullscreen roots. The family traversal still
places dialogs above their parents. The operation changes only the projected
order; canonical state, geometry, and ordinary ordering without maximization
remain unchanged.

The signed, pushed Hagia candidate is
`43cfcae0ac7481e7032962ff45be0cb45fe0d7ef`. Full `nimble verify` passed
with inherited `SOPHIA_*` and `HAGIA_*` variables removed and only the paired
Sophia checkout path set. The retained `hagia-verify.log` in the artifact
directory contains 249 passing Hagia cases, the paired Sophia tests, protocol
corpora, formatting/layout checks, eight Alloy assertions, Z3, and four TLA+
checks. The existing expanded-window session test still passes.

Six new cases cover first/middle/last maximized columns, explicit directional
focus and reversal, an ordinary parent's focused dialog, both creation orders
of two maximized parents, fullscreen families alongside a maximized window,
and normal order with maximization cleared or absent. The fixture capability
correction and stricter named-neighbor assertions were reviewed before the
final gate. No new Sophia code or wire behavior is involved.

The release build and current desktop/extracted-policy validation passed. The
binary SHA256 is
`58df6b1d160efb44435a10b731ce6c7efba191a7c0c526f5812a74d5cd3d220d`.
It was atomically staged at `~/.local/state/sophia/bin/hagia`; `deployment.json`,
`hagia-release.log`, the new binary, and its predecessor are retained in
`.artifacts/t004-maximized-navigation/`. This replaces the previously staged
`9349e57` binary and includes that commit's empty-output navigation fix. The
session was not reloaded, and the personal profile remains unchanged.

Deterministic reproduction, regression checks, and physical acceptance are
separate evidence. The installed Super+F navigation report is not yet resolved
by a physical retest. The owning acceptance task is
[t004](../plans/queue-02-cp-14-3-development-session-readiness-and-milestone-14-c.md#t004).
After loading the corrected restart binding with Ctrl+Alt+R and invoking
Ctrl+Alt+Shift+R, verify the live executable identity and test
Super+Home, Super+F, Super+Right, then Super+Left with the pointer still. The
neighbor must become visible and the maximized window must return when focus
does. Dialog and separate fullscreen acceptance remain part of t004's gate.

## Activation correction after the reported failed retest

The user reported that the neighbor remained invisible and clarified that the
camera did not follow keyboard focus after Super+F. Inspection still found
Hagia PID 30410 holding the old executable hash `92c24358...ace52e`, while the
configured binary path held the new `58df6b1d...3d220d` build. The retained
`failed-live-retest.checkpoint` shows window 1 maximized and focused after the
navigation/reversal sequence; it does not capture the intermediate target.
This retest therefore does not establish a failure of `43cfcae`.

The agent's activation instruction was wrong. Ctrl+Alt+R invokes
`session:reload-profile`, and an unchanged profile did not replace the running
process. The existing Ctrl+Alt+F5 binding named `session:restart-wm`, which
explicitly replaces Hagia using the staged executable. The user was told to
use that binding and navigate right once. This second instruction also proved
incorrect: the reserved virtual-terminal chord preempts the binding, as below.
No second production change is justified by the old-binary retest. Verify the
replacement process hash before attributing further results to the repair.

## Reserved restart chord switched away from the desktop

The user reported a session crash after Ctrl+Alt+F5. The owner and Hagia were
both still alive as PIDs 30405 and 30410, and the journal continued recording
resource samples. Records 306218–306227 instead show a queued virtual-terminal
switch, a drained renderer handoff, then seat suspension. No owner fatal or
session exit is recorded. The desktop remains on `/dev/tty7`, login session 14;
the user was on tty2 at inspection. Ctrl+Alt+F7 was given as the return path.

Sophia handles Ctrl+Alt+F1–F12 before configurable shortcuts in physical input
routing. The personal `Ctrl+Alt+f5` restart binding was therefore unreachable.
The agent should have checked reserved input handling before recommending it.
Evidence is retained under `.artifacts/t004-maximized-navigation/vt-switch/`.
This is not evidence of a restart crash or a failure in Hagia `43cfcae`; neither
restart attempt had loaded that binary.

The active profile and its chezmoi source now bind `session:restart-wm` to
`Ctrl+Alt+Shift+r` and explain the VT reservation. Only that binding and its
comment changed in each file. Both desktop-profile validation and Hagia's
extracted-policy validation pass. The active profile SHA256 is
`359f19803a75745cf40d589102514f25de0cd6ba9a5240d2b0394742927f6d2f`.
The chezmoi change is signed and pushed as `1c93c79`.

The corrected shortcut must first be loaded with Ctrl+Alt+R, then invoked with
Ctrl+Alt+Shift+R. The binding change belongs to session shortcut configuration;
its policy fragment is byte-identical, so profile reload alone need not replace
Hagia. Recovery to the existing desktop and a verified new Hagia process remain
required before retrying maximized navigation. No process was restarted by the
agent during this investigation.

## Verified activation

After the user completed the corrected return/reload/restart sequence, direct
inspection confirmed Hagia PID 650 running executable SHA256
`58df6b1d160efb44435a10b731ce6c7efba191a7c0c526f5812a74d5cd3d220d`,
the `43cfcae` candidate. Sophia owner PID 30405 remains alive and tty7 is active.
Journal record 306910 reports epoch 2, one restart, and preserved layout. The
health record remains running with zero discarded records and storage errors;
there is no owner fatal record. Evidence is retained under
`.artifacts/t004-maximized-navigation/activated/`. The component journal's own
digest is retained separately; the candidate identity above comes from hashing
`/proc/650/exe` directly.

The checkpoint preserves all four left-output windows, maximized window 1
focused, and the empty right output. Activation is now established; visible
navigation is still awaiting the user's next check. Pressing Super+Right once
and holding that state will allow inspection of the intermediate focus/camera
result before reversing direction.

The preceding [pointer focus investigation](nsu4a0n2-optional-pointer-focus-follows-presented-targets-through-committed-policy.md)
records the independent empty-output arrow trap and installed drag repair.
The [original stacking incident](../sources/2026-09/legacy-active-0637-2026-09-06--maximized-stacking-and-gtk-startup-in-the-replacement-session.md)
retains the evidence behind the initial expansion layering rule.

## Triad edge presentation replaces the stacking-only repair

With `43cfcae` verified live, the user reported that the browser appeared to
float over the maximized pane. Raising the neighbor fixed occlusion but did
not provide the requested scrolling behavior. The held checkpoint does not
establish intermediate keyboard focus; the reported appearance motivated the
corrected presentation contract below.

The user clarified the contract against the actual Triad personal profile:
Super+F is `maximize-window-to-edges`, Super+M is `maximize-column`, and
Super+Shift+F is fullscreen. Triad baseline
`fb8fb27ec294e0fe2361375de0b2fa8c08be0ca9` derives effective maximization from
focused-family presentation in `src/systems/presentation_policy.nim` and
`src/systems/layout_projection.nim`. Window and column transitions in
`src/systems/window_state.nim` and `src/entities/column_ops.nim` keep edge intent
separate from full column width. The personal Hagia bindings already express
these distinct actions and require no remapping.

Hagia now derives edge expansion for the focused tiled family in scrolling
layouts. Navigating away restores the former pane's normal strip geometry and
camera translation membership; navigating back restores its retained edge
preference. Dialog focus preserves its parent's expansion. Full column width
suppresses edge presentation without replacing the saved proportional width;
F from that mode selects edge presentation. Fullscreen remains separate.

Sophia snapshots echo committed presentation bits, so simply clearing the
inactive pane's wire maximize bit would erase its preference on the next
snapshot. The adapter tracks the last emitted maximize bit per live surface,
promotes it only with a committed candidate, and preserves private intent on a
matching echo. A changed external bit still updates intent. Checkpoint version
15 retains this reconciliation state. Legacy checkpoints infer it from observed
bits and normalize the old combined column/window flags to preserve their
previous edge presentation.

The independent regression failed against the old implementation on background
geometry, maximize bits, camera membership, and F from M. The repaired eight-case
suite covers both axes with a negative output origin, ordinary redraw echoes,
external unmaximize, rejection, checkpoint restoration and migration, and F/M
transitions with eight-pixel gaps. Dialog-family tests additionally require the
focused parent's expanded geometry and the unrelated pane's ordinary geometry.
Evidence is retained in the local edge-presentation artifact directory.

The signed and pushed replacement is Hagia `b4842f3d265b2aa9effbccfc272e86371b74eb1b`. Final
`nimble verify` passed with 258 Nim cases, the real Sophia socket and launch
checks, protocol corpora, formatting/data-layout checks, eight Alloy assertions,
Z3, and four TLA+ checks. `verify.log` is the final complete gate, including the
independent floating-overlay regression. Release compilation and validation of
the active desktop profile and extracted Hagia policy passed. The new release
also successfully migrated a copy of the live checkpoint, preserving windows
and edge intent while clearing contradictory full-column state. The original
checkpoint was not rewritten.

Release SHA256 is
`ca6aef093b1dfab3e71c531a6521f959594dc938fd3d62bf01d88d2f2d1b0188`. The release
is atomically staged at the configured executable path. Its predecessor, build
logs, deployment record, and checkpoint evidence remain local. The running
executable still matches the previous candidate; staging has not activated the
repair. The user can invoke the already loaded
`Ctrl+Alt+Shift+r` restart binding. Verify the replacement executable hash before
attributing a subsequent visual result to this candidate.

The next physical check is Super+F on a tiled pane, Super+Right to its neighbor,
then Super+Left to return, with the pointer held still. The neighbor must appear
through normal scrolling and the F pane must regain edge expansion on return.
F/M geometry, dialogs, and the separate fullscreen binding remain acceptance
requirements under t004. This implementation and its deterministic evidence do
not close t004 or the separate t078 pointer-focus task.

## F-to-M transition exposes the moving neighbor

The user subsequently reported that changing F to M briefly reveals the right
neighbor over the focused pane before settling. Executable identity confirmed
the report is against `b4842f3`, and the held checkpoint confirms full-column
mode with suspended edge intent. Detailed session captures remain local.

Hagia immediately restores ordinary strip stacking when edge presentation is
suspended by M. Sophia's `TranslationTimeline` commits the resized focused pane
at its new extent while unchanged-size neighbors retain animated positions.
The right neighbor therefore temporarily intersects the enlarged column and,
with ordinary strip order, paints above it. This finding combines the user's
visual report, deterministic projection geometry/order, and the Engine's
translation code; it is not a recorded frame-by-frame capture.

The repair gives the focused full-width column's pane the same elevation as
edge expansion. It reuses focused-family resolution, including parented dialogs
and an independent floating overlay. Navigation to another tiled family
releases that elevation. Full-column geometry, gaps, ordinary wire presentation,
camera membership, fullscreen ordering, and checkpoint schema are unchanged.

The new regression establishes that the neighbor's old rectangle intersects
the M pane, its target lies outside, and its unchanged size permits translation.
It then requires the focused pane to remain above that neighbor through M,
redraw, and navigation reversal, and verifies restoration of ordinary order.
Additional cases cover direct M on first/middle/last columns in both axes,
dialog owners, independent floating focus, and navigation away. Initial exact
gap assertions omitted the existing inner reveal margin; those fixture
assertions were corrected to require inset geometry. The observed ordering
failures remain captured separately in the local pre-fix log.

Signed and pushed Hagia candidate `c6dad96c1547f44a34bc1325602078980d0bed6c` passes the complete
`nimble verify` gate: 261 Nim cases, paired Sophia protocol/socket checks, eight
Alloy assertions, Z3, and four TLA+ checks. The preexisting column-width test now
identifies its pane by stable ID rather than assuming an index in paint order.
The final log contains no failed test cases. Release compilation, active-profile
and extracted-policy validation, and offline restoration of a checkpoint copy
also pass.

Release SHA256 is `d1aa17ca895f859b927af6e045cb409cafce1b0d65103eed37a446f33a8702d4`. The candidate is atomically staged
at the configured executable path, with its predecessor and validation artifacts
retained locally under the F-to-M transition evidence directory. The agent has
not restarted the live session. Activate with the already loaded
Ctrl+Alt+Shift+R binding, then verify F-to-M has no neighbor painted over the
focused pane during settling. Check navigation away and back as well. Physical
acceptance remains required under t004; the deterministic checks alone do not
close it.

## Confirmed edge-gap flash

The user reproduced the flash with `c6dad96` verified running, then clarified
that the neighbor appears only in the right-edge gap, not over the focused
pane's content. This supersedes the earlier interpretation as content occlusion;
the column-elevation candidate did not address the gap. No additional stacking
change is justified by this clarified report.

A standalone offline probe exports actual Hagia F/M projections, passes them
through Sophia's `TranslationTimeline`, builds the production chrome display
list, and constructs output scene snapshots at seven fixed times. The old
projection exposes a neighbor at x=1590 from 16ms through 250ms in a 1600-wide
fixture; the focused content remains on top. At rest x=1590 is clear. This is a
synthetic scene replay, not a native GPU capture of the user's desktop. Detailed
local captures, probe source, projection data, and before/after logs remain in
the gap-transition evidence directory.

Triad's `src/systems/layout_projection.nim` replaces tiled instructions with
only the focused expanded pane before adding floating instructions. Hagia had
instead retained background tiled placements beneath F. Those placements kept
animation membership alive, allowing their old positions to appear through M's
new gap. Hagia now omits ordinary background tiled placements while effective
edge presentation is active in either scroller. Canonical windows, columns,
widths, and camera intent remain. Parented dialogs follow visibility; independent
floating overlays, other outputs, and the existing fullscreen rule are retained.
Leaving F restores strip placements at current targets without hidden motion.

The updated replay keeps x=1590 clear at all seven samples, while preserving the
focused pane's coverage and the ordinary final layout. A permanent Engine test
checks hidden-member removal and reappearance through translation, chrome, and
scene construction. Hagia regressions require edge-only placements with no
translation group, unchanged canonical window count, F/M and navigation
restoration, dialog families, both axes, and output isolation. The existing
session expansion test now distinguishes edge-only visibility from fullscreen
layering while requiring both neighbors to return when expansion is disabled.

The signed and pushed Hagia candidate is `b77722b5de03e025491e53d307a04cbf60dc0aca`. Full
`nimble verify` passes with 262 Nim cases, paired Sophia socket/protocol checks,
eight Alloy assertions, Z3, and four TLA+ checks. Sophia's `cargo fmt --check`,
`git diff --check`, offline metadata check, and complete offline `cargo test -q`
also pass. The Engine translation suite now contains five passing cases.
The independent scene replay passes all seven sampled gap/content assertions.

The separate source-layout audit exits 1 on existing findings in 35 Sophia
files. Every flagged file is byte-identical to HEAD; this patch changes no
Sophia production source. The audit output and baseline comparison are retained
locally. These existing layout findings are not presented as a passing gate.

Release compilation, current profile/extracted-policy checks, and offline
checkpoint restoration pass. Release SHA256 is
`f413024b57fc787cd51c8494c154dff989e4776d6389d36196bd513b7df65baa`. It is staged atomically at the configured executable
path, with a backup and deployment record retained locally. No live restart was
performed by the agent. Load with Ctrl+Alt+Shift+R, release the keys, and check
Super+F followed by Super+M for a clean gap from the first frame. Navigation
away and back remains part of t004's physical acceptance; it is still open.


## Niri spacing for ordinary scrolling

The user separately reported a persistent sliver at the edge of ordinary
scrolling and selected niri's model after comparing the local references.
Niri baseline `9e72e4917ca31baf4010496bf7f4aaf78d34d236` separates one uniform
`gaps` value from optional `struts`: `compute_working_area` reserves struts,
and `compute_new_view_offset` supplies along-axis padding. Its default config
explicitly describes side struts as a way to reveal neighboring columns.
Hagia's previous outer gap reduced the viewport before the camera supplied
another inner gap. With eight-pixel outer/inner gaps, two half-width columns
on a 2560-pixel output put the next column at x=2552, exposing eight pixels.

Hagia now accepts `gaps 8`, with zero struts by default. The corresponding
positions are x=8 and x=1284, widths 1268, and the next column starts at x=2560.
Top/bottom spacing remains eight pixels inside the panel work area. Explicit
side struts reserve preview space; vertical scrolling transposes that rule.
Native and tree layouts also honor struts. Gap actions preserve struts, edge
maximization uses the original work area, and fullscreen uses physical bounds.
Existing outer/inner profiles retain their geometry and cannot mix with the
new form. Checkpoint version 16 carries the gap model and struts; older
checkpoints retain legacy settings until the profile chooses uniform gaps.

Regression coverage includes exact edge positions, navigation, asymmetric
struts, vertical scrolling, gap actions, M/F/fullscreen bounds, native/tree
layouts, malformed profiles, Triad migration, and checkpoint compatibility.
A committed two-output reload case checks that custom column choices and
focus survive legacy-to-uniform migration and a subsequent 8→9→8 gap change.
Its initial exact-camera round-trip assertion was too strict: niri leaves a
column still when the reduced padding already fits. The corrected case
requires exact restored pane sizes and the retained nine-pixel camera inset,
plus stable repeated projections. No production camera change was made to
satisfy the initial assertion.

Signed and pushed Hagia candidate `acb94e02aea6e26c643398f7ec2b160d224b24dc` passes the full
`nimble verify` gate: 272 Nim cases, paired Sophia integration and protocol
checks, eight Alloy assertions, Z3, and four TLA+ checks. Release compilation,
both prepared policy fragments, and offline restoration of the checkpoint copy
also pass. The default-profile test was updated to expect the new explicit
uniform model; bare models retain their legacy representation for API callers.
The release SHA256 is `6a6e28da90c344abd08aed84772b45e4791a6c8902849003794ee02cd028afb3`.

The release is atomically staged at the configured executable path. The active
personal profile and chezmoi source now use `gaps 8`, with omitted, zero-valued
struts. Only the gap block changed in each file. No live restart was performed.
Ctrl+Alt+R loads the changed profile and replaces the policy client; verify the
live executable before attributing a physical retest to this candidate. All
detailed evidence and rollback copies remain local under
`.artifacts/t004-niri-gaps/`. Physical acceptance still belongs to
[t004](../plans/queue-02-cp-14-3-development-session-readiness-and-milestone-14-c.md#t004)
and [t011](../plans/queue-04-2-establish-the-live-session.md#t011).

## Operator retest after the spacing update

The user reported that the issue "appears to be fixed" after the reload and
edge/F-to-M retest instructions for `acb94e0`. The session checkpoint now uses
schema 16, uniform gaps of eight pixels, and zero struts, confirming activation
of the new spacing settings. This records acceptance of the reported visual
artifact in observed use. It does not individually establish the remaining
t004 dialog/fullscreen and navigation cases or t011's insertion/close,
vertical-scrolling, and both-output coverage; those broader tasks remain open.

## Grouped desktop acceptance preparation

The user approved proceeding with desktop acceptance first, retaining Brave for
daily use and the named Firefox checks, with logout/login and a disposable
recovery cycle at the end. The live Hagia executable matches `acb94e0`'s release
digest above; Sophia remains `0c069d2f`. Baseline recorder health reports no
discarded records or storage errors. The working Hagia binary, profile,
checkpoint, and component identities are retained privately with checksums in
`.artifacts/todo-acceptance-20260911/baseline/`.

A temporary profile adds Super+Ctrl+F7 for numbered Kitty windows, F8 with the
same modifiers for vertical scrolling, and F9 for a synthetic GTK parent/dialog
fixture. The dialog fixture supports sibling and nested windows without using
the signing agent. Compilation and both owning profile validators pass. The
profile is staged for user-triggered reload; chezmoi remains unchanged. Remove
these exact temporary blocks and restore the original layout after acceptance.
The first requested scenario is three labeled terminals followed by two column
steps backward and forward. Preparation alone does not close t007 or t011.

The operator performed the first scenario and reported that it "looks buggy".
Profile generation 4 activated, and the recorder captured three application
starts and three committed surface admissions. The subsequent checkpoint has
seven windows, all on the left output, with the original uniform gap settings.
The event rings and checkpoint are retained privately under
`.artifacts/todo-acceptance-20260911/first-scrolling-report/`; recorder health
still reports zero discarded records and storage errors. This confirms execution
of the fixture launches, not correct rendering or the requested focus sequence.
The checkpoint was captured after the operator returned to the conversation,
so it cannot establish the intermediate navigation state. Clarification of the
visible failure is pending; the broader acceptance cases remain unaccepted.

The operator clarified that no acceptance labels were visible and that a large
gap between Kitty windows looked like two invisible windows. Read-only process
inspection found all three fixture Python children alive, waiting on their
terminal input, and X11 inspection found all three expected acceptance titles.
The test surfaces are 71303182, 73400334, and 75497486. Their visual candidates
were recorded during admission, but none has a completed Present retirement in
the preserved launch/navigation interval (sequences 815430–817677); older Kitty
surfaces do. This narrows the investigation to admission/presentation rather
than a missing launch command, but does not yet establish the root cause.
The operator was asked to navigate back to the gap. A bounded, read-only
checkpoint watcher retains intermediate focus and camera states locally so
returning to the conversation does not erase that evidence.

The operator confirmed that focus stops inside the blank space. The checkpoint
watcher captured focus visiting logical windows 6, 7, and 8, with viewport
offsets 5096, 6372, and 7648 on forward navigation. These are the three fixture
surfaces. Their first presentations still do not retire while older windows
continue to present. The repeated gap evidence is preserved locally under
`focus-stops-in-gap/` beside the checkpoint sequence.

Code inspection found a matching failure path in the native Present driver.
It skips a candidate absent from the lowered physical frame, including a new
column whose settled geometry is visible but whose animated position has not
entered the output. For a first frame, admission still waits for that exact
candidate's retirement and holds later drawing in quarantine. A deterministic
head-lowering case reproduces the visibility mismatch at animation start and
confirms capture once the camera advances. This establishes the failure path;
the existing daily records do not identify the precise rejection branch taken
by the live candidates.

The repair retains a first candidate outside the runnable Present queue until
its animated geometry intersects its assigned output. It preserves exact
candidate identity, allows another surface to present, and uses existing
removal/shutdown cancellation. Already displayed surfaces retain offscreen
Skip semantics. The focused head-lowering and scheduler suites pass 27 tests,
including delayed visibility, unrelated-window progress, stale-generation
rejection, and draining the parked candidate. Workspace tests and the native
backend's full-feature suite pass, as do formatting, offline metadata, task-ID
uniqueness, note links, and whitespace checks. The initial sandbox run could
not bind the test Unix sockets; the rerun with that access passed. Running the
whole backend suite with only the two focused-test features exposed existing
feature-gating assumptions, so broader coverage used all backend features.
The source-layout audit still reports existing violations in unchanged files.
Installed acceptance remains pending; t004, t007, and t011 remain open.

## Fresh-session retest of c9ff05d8

The operator installed the candidate and reported that Super+Enter did not
show Kitty. The live Sophia executable matches the packaged SHA256
`aa536d7e5e1ecc25531da4d3239ae93bd5aff21cdcd44f2c27fe8acd11180e0e`;
Hagia still matches `acb94e0`, independently of concurrent source work in that
repository. The launcher started thirteen Kitty processes. The first received
an admission acknowledgement and a centered 1266×1390 client allocation, but
no visual candidate was recorded before two four-second admission attempts
expired. X11 inspection subsequently found it unmapped. The checkpoint still
contains no managed windows, unlike the earlier seven-window scrolling case.
The session later switched away and its seat became suspended.

This failure occurs before the first-buffer evidence seen in the earlier case;
it does not establish which startup operation stopped Kitty drawing. The
session records and checkpoint are retained privately under
`.artifacts/todo-acceptance-20260911/new-session-kitty-missing/`. The numbered
fixture now writes rendering diagnostics to a private file for the next launch;
it does not record keyboard input. Physical acceptance remains unresolved.

## Accepted, 2026-09-20

The acceptance this note waited for is given: several days of continuous
daily-driver development use of Sophia, Hagia and Lom on installed release
`0.1.0-d461492da746` with both monitors, which is the normal use every row
here asked to be judged in. t004, t007 and t011 are closed on it.

The last evidence recorded above is a failed fresh-session retest from
2026-09-11 -- thirteen Kitty processes launched and the first never drew --
and that is left standing rather than deleted. It is nine days old, it names a
startup failure rather than a stacking one, and sustained use since has not
reproduced it. If it returns it is its own report, not this note's.

What remains of this note's subject belongs elsewhere: t078, optional
focus-follows-mouse, is an implementation row and its own investigation is
`nsu4a0n2`.
