---
id: h0vxis10
date: 2026-09-06
kind: investigation
status: investigating
tags: [investigation, x11, rendering]
---
# Brave GPU watchdog repeats during live use

## Question

Why is Brave Origin slow in the current live session?

## Evidence

The user reported sluggish Brave Origin and later an intermittent fade confined
to the window border under CPU activity. Application content did not fade.
The user clarified that the border flashes orange to black and back.
The inspected session is
`00000001788748566248-d7be692d-f3cf-4f0d-9e70-9e6dfb3e7359`, running installed
`ef1ba0e7`. Its Sophia binary SHA-256 is
`a53655c27d7b6e7819dccabcbffc7a6ec5f31a4da314eed431c15e8267cfa64f`.
Brave is the user's `brave-origin-1.94.121`, launched with `--ozone-platform=x11`.

Read-only inspection found three GPU-process dumps in the existing
`~/.config/BraveSoftware/Brave-Origin/Crash Reports/completed` directory:

| Local time, 2026-09-06 | Dump | Process |
| --- | --- | --- |
| 22:58:25 EDT | `a6f7f1be-83e0-4c5b-a6d5-aae519d1b25b.dmp` | 12877 |
| 22:58:55 EDT | `de58f976-55af-4a02-9b92-ea836dcac72a.dmp` | 14642 |
| 22:59:25 EDT | `91494f3f-9c46-4f58-b8c0-eaabaf2f7909.dmp` | 16249 |

All three identify `--type=gpu-process`, crashing file
`gpu/ipc/service/gpu_watchdog_thread.cc`, GPU thread `.main`, and a hung main
thread. Their annotations say neither GPU initialization nor power-resume
handling was the trigger. The replacement GPU process, PID 18429, initially
used about one CPU core and had `--use-gl=disabled` when inspected later.
Only selected diagnostic fields were inspected; no browsing history, URLs,
credentials, or memory payloads were exported from the dumps.

Both Brave's main process and GPU process sent stdout/stderr to `/dev/null`.
No separate debug log was found. The session launcher inherits those streams,
so the missing browser error lines cannot be recovered from Sophia's structured
recorder. The current recorder remained running with zero discarded records
and storage errors. No retained protocol-error or timeout event matched this
launch; `timeout_msec` on resize configuration is not itself a timeout.

## Finding and limits

A repeating GPU watchdog hang is established. The disabled-GL replacement is
consistent with graphics fallback after the failures. Neither observation
identifies the GPU operation that blocked or proves that QueryPointer caused
it. No browser or session process was restarted, signalled, or reconfigured.

The border report is separate evidence. Engine's frame/focus-ring construction
uses opaque borders and focused/unfocused colours; the inspected path has no
CPU-load fade indicator. Recent retained chrome-set generations alternate, but
the recorder omits the frame and focus counts from those records. A generation
change alone cannot distinguish focus changes, changed geometry, or missing
chrome during composition. The border cause remains unconfirmed.

## Replacement-session recurrence

Installed `86ab21e4`, session
`00000001788751946481-31db6852-d07f-4f08-8ed9-87f63a561f59`, produced dump
`70caf92a-1dba-4215-9b18-57045b6dab1b.dmp` at 23:41:18 EDT on September 6.
Selective inspection identifies a GPU process and the same watchdog source
file. The user reports that all Brave interaction stops after a few clicks,
then recovers after switching windows.

At 23:46:55 EDT, browser PID 29334 and replacement GPU PID 28919 were both
sleeping in `poll_schedule_timeout.constprop.0`; the GPU watchdog thread was
waiting on a futex. No newer dump existed. These are waiting-state samples,
not stack traces or proof of a particular blocked X request.

Source inspection found that deferred `PresentNotifyMSC` requests advance only
through Present completions. A future timing request can therefore lack progress
when there are no further completions; reading the clock and queuing a request
also use separate locks. This is a liveness hypothesis, not an attribution of
the browser freeze. An invisible temporary probe window subscribed only to its
own Present events and asked for current MSC followed by MSC + 1. Two probes
returned in approximately 1 ms and 14 ms, including the one immediately after
the latest freeze report. The second advanced from 19566741 to 19566742.
The probe did not reproduce a stopped global presentation clock. It never
mapped a window, took focus, injected input, or changed another application's
subscription. Its observed script path is `/tmp/sophia-present-clock-probe.py`.

The [Present specification](https://sources.debian.org/src/xorgproto/2025.1-1/presentproto.txt)
defines timing notifications independently of submitting new pixmaps. Any
repair must use Engine/backend timing and retain frontend event ownership,
without fabricated frame completions or a WM timing policy.

[Interaction diagnostics](ce2b55uy-blank-thunar-menus-and-frozen-brave-need-separate-pixel-and-delivery-evidence.md)
now preserve delivery and grab counters that the old recorder discarded. This
is an evidence repair; neither the graphics hang nor the freeze is fixed.

The next session, installed from `e8573cf1`, retained routed clicks followed by
repeated explicit-grab rejections. The [click-lease investigation](744uylx4-explicit-pointer-grabs-must-replace-their-own-click-lease.md)
reproduces that ownership defect and tracks its repair separately. No new GPU
dump accompanied the sampled recurrence. Neither this correlation nor the
grab regression attributes the earlier watchdog dumps to pointer ownership.

## Next diagnostic step

On a deliberate browser relaunch, retain browser stderr in a private bounded
log and correlate the next watchdog failure with frontend request/presentation
progress. Do not disable the watchdog or advertise unsupported GPU capabilities
to hide the failure. For the border, correlate focus/chrome counts and exact
presentations before changing frame colours, animation policy, or damage.

## Border recurrence on 2026-09-07

On installed `a44d1e16`, the user reports border flicker while typing on
Monkeytype in Brave, while moving the mouse in Brave, and occasionally in
Kitty. This recurrence is distinct from the newly measured
[GBM import failure](uqnx2t2b-brave-gpu-restarts-after-va-buffers-fail-gbm-import.md).

The live recorder now retains chrome counts. A recent sample contains 1082
chrome-set records; examples at uptime 310368506 through 310372479 alternate
between zero and one focused frame while retaining three frames and three
eligible surfaces. Source inspection found that `run_cpu_repaint` overwrites
`self.focused_surface` with `raised_surface`, although ordinary production
accepts focus separately from that optional stacking override. The next
regression checks whether repaint preparation incorrectly clears chrome focus
when no stacking override is requested. These records establish chrome-state
changes, not the duration or appearance of every physical flicker.

After relaunching Brave with its media device aligned to renderD128, the user
reported that flicker stopped. This happened on the unchanged installed build.
It limits attribution of the visible symptom: the repaint focus regression is
independent source evidence, not proof that every observed flicker had that
cause. The cadence caller raises a client-positioned popup when one exists,
otherwise the focused surface; it does not pass a generic layout override.
A frameless raised popup is the concrete case in which assigning stacking to
chrome focus can remove the focused frame.

The candidate now passes stacking and focus separately through ordinary and
forced repaints. Startup passes no focus; topology retains the current chrome
focus; cadence passes the seat's focus independently of the raised popup.
The regression calls real display-list preparation and verifies that a popup
or another framed window can be raised without taking focus, that raising still
changes stack order, and that explicit absent focus clears the frame. Mutating
focus back to the raise, bypassing chrome normalization, or dropping the raise
makes the regression fail. All 30 backend library tests passed.

The combined `cargo xtask check` passed outside the restricted tool sandbox,
including archive verification and buffer-age equivalence. The first restricted
run failed when an unrelated CLI socket fixture received EPERM. The installed
session remains the original build; physical acceptance of this source repair
has not occurred.

The repair and its installed acceptance belong to the existing t066 redraw
work; task status remains in [todo.md](../../../todo.md).

## September 18: bounded stderr and three mapped windows

Session `00000001789758977436-af9db2f8-2d13-4251-bf37-4bae3d66b43c`
runs installed `7a56c3e6`. Its launch recorder contains one browser launch,
transaction 19, followed by three additional surface admissions. The operator
reports two browser windows and a blank column the width of a normal window.
The retained records do not identify which surface was blank or distinguish
a helper window from a legitimate browser window. Changing gaps or suppressing
windows would not be a demonstrated repair.

The private bounded stderr log records GPU exits at 19:17:30, 19:18:00 and
19:18:30 UTC. Selective annotations from the three corresponding existing
Crashpad dumps identify `gpu_watchdog_thread.cc:691`, a hung GPU main thread,
and neither initialization nor resume as the trigger. Raw stack-address
inspection finds `xcb_wait_for_special_event` and the same libgallium return
chain in each dump. Matching disassembly and Mesa's DRI3 back-buffer wait loop
make a withheld Present completion/idle notification a concrete lead. This is
not a symbolized unwind or identification of an exact stranded transaction.
No dump memory payloads, page contents or URLs are included in normal evidence.

The frontend decoder also omits CreateWindow's InputOnly/InputOutput class.
That is a separate conformance gap; this capture does not establish that one
of these three surfaces was an InputOnly window. Requested class is therefore
recorded before translation in the diagnostic candidate, without using it to
change admission policy.

The diagnostic candidate correlates lifecycle transactions with process-local
opaque resource tokens and records Present acceptance, readiness, actual queue
admission/refusal, write start and successful write/flush separately. Successful
socket write is not proof of peer consumption. The existing bounded recorder
filters application strings and raw XIDs; overload remains counted, so a missing
record alone cannot establish a missing protocol effect.

A headless control drives the actual frontend registry and socket writer with
three windows, a separate subscriber, alternating buffers and both feedback
orders. It completes 1,000 cycles per window in each byte order while retaining
one earlier presentation, then releases that exact presentation. This passes;
completions are supplied, so it does not exercise native rendering or reproduce
the browser hang. Evidence is private under
`.artifacts/brave-multiple-windows-7a56c3e6/`.

Remaining work is exact backend-to-wire correlation and a failing ownership
control before changing presentation behavior. The isolated SelectionNotify
sequence repair belongs to the integration owner and is not bundled into this
diagnostic slice. Brave usability and physical acceptance remain open.

## Related work

- [t003](../plans/queue-02-cp-14-3-development-session-readiness-and-milestone-14-c.md#t003)
  still requires usable Brave typing; Ghostty's accepted startup remains valid.
- [QueryPointer repair](knjco01f-pointer-queries-must-share-admitted-namespace-state.md)
  addresses a separately reproduced X input-query defect.
- [Runtime crash](fltuldiq-runtime-session-crash-retains-no-specific-cause.md)
  remains a separate incident; these dumps do not establish the same cause.
