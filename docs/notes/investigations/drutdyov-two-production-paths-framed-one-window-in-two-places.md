---
id: drutdyov
date: 2026-09-19
kind: investigation
status: investigating
tags: [investigation, rendering, session]
---
# Two production paths framed one window in two places

## Question

Why did every repaint flicker the window frames in a live two-output session,
starting in the middle of ordinary use and continuing until the browser window
was closed?

## Evidence

Live Hagia session `00000001789856355797-f3862799-2247-4ffb-a97c-4455029970f5`
on installed release `0.1.0-84d906f148d6`, display `:77`, outputs DP-1
2560x1440 and DP-2 1920x1080. Three framed surfaces: kitty 2097166 on output 1,
Brave 12582915 and a second kitty 56623118 on output 2. Records were copied
before rotation to
`~/.local/state/sophia/session-investigations/20260919-chrome-double-composition-flicker/`
(events 19:05-19:43, window tree, process snapshot). The session's own event log
rotates about every 25 minutes; the startup segment was already gone.

The operator reported flickering in the gaps around the output-1 kitty while
Brave animated a video page at 40-90 presents per second.

### The signature

`sophia_live_compositor_chrome_set` is emitted only when the chrome display list
changed, and its `generation` is an FNV hash over each frame border's committed
geometry, width, colour and role. From 19:34:58.912 it alternated between
exactly two generations on consecutive owner-loop turns, one after each scanout
and one on the turn that followed, with every count identical:

```
generation=17733829688024964069 eligible_surfaces=3 frames=3 focused_frames=1 unfocused_frames=2 focus_rings=0 primitives=12 clearance=1
generation=8323122400825524277  eligible_surfaces=3 frames=3 focused_frames=1 unfocused_frames=2 focus_rings=0 primitives=12 clearance=1
```

Before that moment the record appeared about once a minute. After it, about
twice per scanout: 1057 records in the 19:35 minute, 2074 in 19:38. Identical
counts with a differing hash means one frame's geometry differed, not the set.

### What preceded it

| time | record |
| --- | --- |
| 19:06:24 | `resize_epoch schema=3 status=visual_committed transaction=133176 surface=12582915 width=2558 height=1414` |
| 19:15:38 | `status=visual_armed epoch=80 surface=12582915 width=1266 height=1398`, never committed |
| 19:34:58.868 | `status=visual_armed epoch=118` for 12582915 and 56623118, both 1266x694; `status=queue_committed epoch=118 staged_presents=1` |
| 19:34:58.912 | the alternation begins, on the very next repaint |
| 19:35:06.367 | `sophia_live_wm status=layout_timeout transaction=120 preserved_layout=true rollback_transaction=9223372036854775808 rollback_configures=1`, then `queue_aborted epoch=120 rejected_presents=1` |

Brave's last *visually committed* size was therefore the full output-1 rectangle
from 19:06. Two later epochs moved and resized it onto output 2 and neither
reached `visual_committed`. The 19:35:06 layout timeout is four seconds later
than the onset: a consequence of the same stall, not the trigger.

The X tree at 19:43 still disagreed with the layout, showing the second kitty at
1266x1398 where epoch 118 had asked for 1266x694.

### Live control

At 19:52 the operator closed the Brave window with Super+q and the flicker
stopped at once, naming surface 12582915 as the frame drawn in two places. The
chrome-set record carries only a hash, so the log alone could not have named it.
That gap is closed below.

## Finding and resolution

The compositor had three observation sites, each handing the chrome hash a
different view of surface state, and one owner-loop turn runs exactly one of
them (`owner_loop/authority_production.rs`, the branch on
`has_dma_buf_present_submissions`):

- the Present turn hashed `prepared.candidate()`, the committed baseline with
  the presenting surface at its presentation-layout geometry
  (`production_visual_runtime/present.rs`);
- the CPU production turn hashed `report.committed_surfaces`, the Engine's
  post-intake set (`production_visual_runtime.rs`);
- the cadence repaint hashed `production.committed_surfaces()`.

For a surface presenting continuously whose committed geometry is stale, the
Present turn frames it where the layout put it and the next CPU turn frames it
where the Engine still holds it. The Engine commit is deferred to page-flip
retirement and discarded outright when the baseline moved
(`engine/layout/authority_transaction.rs`, "discarded prepared surface commit
because its baseline became stale"), so the two views never converge on their
own. The retained-repaint path already had the correct notion, overlaying
`present_scheduler.in_flight_candidate()` on the committed set, and nothing else
used it.

A second, independent mismatch on the same seam: the CPU adapter composed its
software frame with `surface_chrome_display_list`, framing every surface in the
presentation order, while the head frames and the observation used
`chrome_surfaces`. Client-positioned popups the session never authorised a frame
for were framed in that path.

Correction, in `sophia-backend-live`:

- `displayed_surface_view()` names the one view, the in-flight candidate else
  the committed set, and every chrome site derives from it. The Present turn
  keeps its candidate, which becomes that view at `mark_rendering`.
- The Present turn's observation moved to the two points where frames actually
  reach a head, so a Present deferred for first visibility or rejected observes
  nothing.
- `LiveProductionCpuCycleAdapter` takes `chrome_surfaces` and uses
  `surface_chrome_display_list_for_surfaces`.

Diagnostics, so a recurrence names itself:

- `sophia_live_compositor_chrome_frame schema=1 generation= source= in_flight=
  surface= x= y= width= height= focused=` accompanies each chrome-set change,
  one record per framed surface, for the first sixteen changes and then on
  powers of two. `source` is `present`, `production` or `repaint`. The field is
  deliberately not called `path`: the record reducer drops that key
  unconditionally, as a filesystem path is payload.
- `sophia_live_session_present schema=5 status=discarded transaction= surface=
  outcome= baseline_generation= current_generation=` promotes the previously
  warn-only discarded retirement, which is the state that keeps the screen and
  the committed set apart.

## Validation and remaining work

Deterministic, on `fix/chrome-flicker-displayed-view`: `cargo fmt --check`,
`git diff --check`, `cargo metadata --no-deps --offline`, and 2032 tests across
`sophia-engine`, `sophia-backend-live` and `sophia-session` with
`libdrm-events,gbm-probe,native-session`.

Two new regression tests in
`crates/sophia-backend-live/tests/support/live_presentation_regressions.rs`.
`chrome_follows_the_displayed_view_while_a_present_is_in_flight` models this
incident, a surface whose committed geometry is stale while a Present at its new
geometry owns the scanout, and asserts that no observation alternates across
four turns of both kinds, with a negative control proving the committed set
would have hashed differently. `the_software_frame_frames_only_the_authorised_surfaces`
pins the chrome-set threading against composed pixels.

Not covered deterministically: the native Present turn and the cadence repaint
have no headless constructor for `LiveProductionNativeScanout`. Those remain for
`tools/run_sophia_input_latency_qemu.sh` and physical acceptance.

That QEMU gate could not confirm them, because it is already red on master. On
this branch and on a pristine `d461492d` checkout it fails identically: the
input proof types and clicks, `sophia_live_session_quiescence status=started
reason=tick_limit timeout_msec=2000` begins the shutdown, and xterm then dies
with `fatal IO error 11` and exit status 84, which the session reports as
`client_fatal source=primary` and `sophia_qemu_guest status=failed
reason=session_exit`. The failure signatures match line for line, and the last
green run of this gate was 2026-08-28, so it is a stale gate rather than a
regression from this change. Evidence:
`~/.local/state/sophia/qemu-input-latency/<commit>/`. Queued as t126; until it
is green again the native path here has no automated cover.

A verification hazard found while running these: a terminal launched by the
session inherits `SOPHIA_RUN_REAL_ATOMIC_SCANOUT_SMOKE=1` and `SOPHIA_HAGIA_BIN`,
so an ordinary `cargo test -p sophia-backend-live` from such a shell runs the
real-GPU atomic scanout smoke and the Hagia profile admission case. Clear the
`SOPHIA_*` variables when running crate tests from a session terminal.
Separately, `cargo test --offline -q` with default features does not build
`sophia-session` at `d461492d`: `diagnostics/shell_component.rs` references
`crate::live_session`, which is gated behind `native-session`. Confirmed on a
pristine master checkout, so it predates this change.

Out of scope here, and the reason the two views could diverge at all: nothing
reconciles the WM layout, the frontend geometry and the Engine's committed
geometry after a layout timeout. `wm/layout.rs` `expire_pending` never rewrites
`self.layers`; an armed visual epoch has no deadline of its own, so epochs 80
and 118 stayed armed indefinitely; and `observe_authority_batch` corrects
`layer.geometry` from the authority only for `ClientPositioned` surfaces, never
for a policy-managed window. Queued in `todo.md` as t127.

## Connections

The four-second budget in
[a kitty window waits out a four-second layout budget](cnbxdj48-a-kitty-window-waits-out-a-four-second-layout-budget-before-it-opens.md)
is the same `expire_pending` path whose rollback follows this incident's onset.
[Window switches reveal delayed visual updates](vwo9wmie-window-switches-reveal-delayed-visual-updates.md)
records a related staleness symptom in the same client.
[Brave GPU watchdog repeats during live use](h0vxis10-brave-gpu-watchdog-repeats-during-live-use.md)
covers the browser-side GPU restarts seen later in this same session, which are
a separate fault: on relaunch at 20:23 Brave's window stayed black while its GPU
process was killed by its own watchdog every thirty seconds, and at 20:24:52 the
owner loop ended the session with `runtime_fatal ... failure_code=unclassified`.
Neither is explained here.

## t126: the gate was failing on its own shutdown — 2026-09-20

Reproduced on master before changing anything, having first built an
initramfs for `6.18.50_1`: the newest was `sophia-6.18.46_1.img` from
2026-08-30 while the running kernel has been `6.18.50_1` since 2026-09-08,
which is part of why the gate had gone unrun. The signature matched the
record line for line.

**It is not a client crash.** The ordering is the finding:

```text
quiescence schema=3 status=started reason=tick_limit timeout_msec=2000
xterm: fatal IO error 11 (Resource temporarily unavailable) ... on X server ":181"
client_fatal status=detected source=primary exit_status=84 action=bounded_cleanup
client_fatal status=cleaned ... cleanup_errors=0
session_failure status=failed phase=lifecycle failure_code=unclassified
```

The session begins shutting down on its own tick limit. That closes the X
server its terminals are connected to, so xterm loses the connection and exits
84 -- the shutdown working. The session then reports that consequence as an
unclassified lifecycle failure, with `cleanup_errors=0` on the very next line.

`owner_loop/lifecycle.rs` tested only `!status.success() && !config.normal_session`
and never consulted quiescence, although `session_quiescence` is in scope in
that file and another branch already guards with `is_none()`. The secondary
detector had the same gap one branch away.

Repaired by `terminal_exit_is_session_failure`, a pure predicate beside
`successful_primary_exit_ends_session`: an exit is this session's failure only
when the session was not already quiescing. The exit is still recorded either
way; what the predicate decides is whether it ends the session as a failure.

**After the repair the session completes.** `client_fatal` and
`session_failure` are gone, quiescence reaches `status=complete
reason=tick_limit elapsed_msec=50` with every pending count zero, and
`sophia_qemu_guest` reports `status=complete ticks=300`. Composition is
exercised and recorded: `runtime_committed=32`, `native_submissions=37`,
`native_retirements=35`, `native_nonzero_exports=18`, `cpu_nonzero_frames=16`,
`input_text_match=true`.

## What still holds t126 open

The gate now fails later and elsewhere, in
`tools/verify_live_session_persistent_evidence.sh`, whose `positive_keys` list
requires `runtime_surfaces` and `cpu_layers` to be non-zero. Both are zero
here, and **the list appears to mix gauges with counters**:

- `runtime_committed` accumulates through `record_runtime_commits`, which is
  `saturating_add`. A counter.
- `runtime_surfaces` is assigned `runtime.committed_surfaces().len()` at each
  sample. A gauge, whose correct value at the end of a session whose client has
  exited is zero -- which is now the ordinary path, where before the repair the
  run never reached this verifier at all.

`cpu_layers` is not yet traced to its source, and there is a reason not to
guess: the known-good reference line in `tools/check_hagia_native_matchers.sh`
carries `cpu_layers=0` *and* `runtime_surfaces=3`, so that list does not match
a passing native session either. Deciding what those two keys are for is the
remaining work, and weakening the assertion to make the gate green without
knowing would be the wrong repair -- the gate exists to cover native scanout
composition, and the composition evidence above is exactly what must keep
being required.
