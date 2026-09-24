---
id: queue-04
date: 2026-09-06
kind: plan
tags: [plan, milestone]
---
# 2. Establish the live session

This plan retains the scope, constraints, and task details from the roadmap
cutover. Task status and order live only in [todo.md](../../../todo.md)
and the [monthly completion history](../../../done.md). Follow the
[work-tracking contract](../../work-tracking.md).
Historical candidate identities in the details require revalidation before use.

[Parent scope](queue-02-cp-14-3-development-session-readiness-and-milestone-14-c.md).



Previously completed evidence: [Allow local session installation from signed local Hagia commits without requiring a push or origin/master equality.](../sources/2026-09/todo-cutover-completed.md#legacy-done-009).


Previously completed evidence: [Add the opt-in Quickshell X11 panel and cargo xtask panel launcher with explicit GPU/software selection and retained binary identity.](../sources/2026-09/todo-cutover-completed.md#legacy-done-010).


Previously completed evidence: [Honor the desktop startup list in the ordinary Hagia launcher, retaining explicit CLI overrides and a terminal fallback.](../sources/2026-09/todo-cutover-completed.md#legacy-done-011).


## t007

Revalidate the third-terminal crash repair: `4eb1136a` physically showed
both panels, kept terminal bounds correct and admitted new terminals, then
failed when a repaint had no staged Present image. The lowered-frame guard
now skips invisible candidates while preserving the old-area repaint. Accept
terminal insertion and scrolling out of view/back on the rebuilt session;
[diagnosis and retained evidence](../sources/2026-09/legacy-active-0006-2026-09-06-third-terminal-crash-after-successful-panel-presentation.md).


## t008

Confirm shell-owned panel startup: the automatic Tier-0 bar, fixed top
reservation and hit targets are removed. Quickshell is the selected panel;
Narthex's switcher and explicit tab descriptors remain enabled. Check one bar
per output and full work-area restoration when the panel stops.


## t009

Accept this panel in normal GPU use on both outputs: pointer hit targets,
popout anchoring, focus, stop/relaunch and coexistence with Narthex. Native
shell content and DMS remain deferred; this is an X11 compatibility client.


Previously completed evidence: [Confirm the panel popout visibility repair in normal GPU use.](../sources/2026-09/todo-cutover-completed.md#legacy-done-012).


## t010

Diagnose the forced-deadline control drain race captured in
`/tmp/sophia-panel-probe-v5` (12 dispatched, 11 delivered, one pending).
Normal-exit probe success does not waive this separate shutdown finding.

The [control-quiescence investigation](../investigations/2ss985xn-quiescence-must-consume-acknowledgements-for-final-controls.md)
records the reproduced missing acknowledgement boundary, Session repair, and
isolated deadline-probe validation.



## t011

Accept the scrolling repair in normal use: three Kitty windows, reversal,
insertion/close, vertical scrolling and both outputs. The implementation adds
committed Hagia camera anchors and Engine GPU translation; see the
[contract and focused check](../../window-transitions.md). Broader physical
acceptance remains pending; new-window camera following is confirmed below.
The user's selected uniform gap model must retain eight-pixel spacing with no
forced edge sliver when two half-width columns align with the viewport. Check
normal scrolling on both outputs and vertical scrolling; arbitrary widths may
still leave a partially visible neighbor. Implementation and validation are in
the [spacing investigation](../investigations/3asecq4a-maximized-windows-obscure-keyboard-navigation-targets.md#niri-spacing-for-ordinary-scrolling).


Previously completed evidence: [Confirm that opening new windows in the scrolling layout moves the camera to them.](../sources/2026-09/todo-cutover-completed.md#legacy-done-013).


## t012

Revalidate installed startup after the output-ownership repair. Release
`0.1.0-86b5fe1d20bc` exited on the first Kitty Present after losing its output
assignment. The operator entered installed fix `84c109c6`; logs confirm two
outputs settled, continuing GPU Presents, and no startup fatal error.
Visible both-output behavior and normal logout still need acceptance. See the
[diagnosis](../sources/2026-09/legacy-active-0012-2026-09-05-installed-startup-loses-the-first-windows-output-ownership.md).


Previously completed evidence: [Delegate WM policy-setting validation to Hagia.](../sources/2026-09/todo-cutover-completed.md#legacy-done-014).


## t013

Reuse the existing launcher and installed session path; identify the exact
Sophia/Hagia/Narthex binaries and profiles, retain a known working fallback,
and document start, normal logout, emergency escape, and rollback.
Release `0.1.0-417e97d2e25b` is packaged with Hagia `38ea8da` and checked
executable/profile digests. The personal profile's two legacy application IDs
were corrected with operator approval; paired parser checks and complete
session-argument preflight pass. That release was installed and started,
which surfaced two session-ending defects now fixed in `e18beede`: a pointer
preference a device could not hold failed the whole seat, and the requested
output topology was validated and then never applied. Release
`0.1.0-e18beede1831` packages those with Hagia `5662d43`, whose profile
validation now refuses a trigger Sophia cannot bind to a keycode — the second
defect that ended a login, and one `hagia config check` had been calling
valid. Installation of that release awaits local sudo authentication.

The September 7 [guard update](../investigations/6q35cl9y-normal-login-arms-recovery-without-a-keyboard-rehearsal.md)
removes manual arming from ordinary installed logins while retaining independent
recovery and the manual proof handshake. Accept login without the handshake,
normal logout, and a separate emergency recovery after installing the candidate;
these observations do not replace the other t013 identity and fallback exits.

The [September 24 acceptance record](../investigations/6q35cl9y-normal-login-arms-recovery-without-a-keyboard-rehearsal.md#operator-acceptance-and-retained-installation-2026-09-24)
records Mason's repeated successful login/logout/recovery, verified current and
previous package identities, the retained accepted `d461492d` fallback, and
the live user-profile/Hagia override. This supersedes the stale installation
and physical-acceptance remainder above; no new live session was required.


## t014

Pass a short physical acceptance check: start normally, launch terminal
and Firefox, type and change focus, resize, use both outputs, return from a
VT, and log out cleanly. Include a basic tab-layout interaction.


Exit: the operator can enter and leave a normal development session without a
comparison controller or benchmark workload. This stage establishes pilot use,
not milestone completion; fuller workflow acceptance follows in stage 4.
