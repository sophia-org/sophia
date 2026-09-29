# Window translation contract

Sophia Engine owns presentation motion. A WM chooses final layout, camera
policy, focus and membership; it submits one atomic projection. Engine moves
retained pixels and surface chrome between accepted placements on the GPU.
There are no per-frame WM requests or client resize transactions for translation.

## Negotiation and authority

`sophia_wm_v1` revision 3 optionally negotiates `translation_groups` (bit 12).
The complete projection candidate contains counted sections of
`ProjectionTranslationGroup` (`0xFF03`, 32 bytes) and
`ProjectionTranslationMember` (`0xFF04`, 24 bytes) rows. The
[WM file schema](../protocol/sophia-wm-files-v1.kdl) defines their offsets;
the [WM file contract](sophia-wm-files.md) defines submission and custody.

A group carries an output, nonzero opaque group ID, signed final x/y translation,
member count and zero reserved word. Members repeat output/group and name opaque
generational surfaces. At most 16 nonempty groups and 1,024 total members are
accepted. Each member must occur once and have a placement on that group's
affected output. Fullscreen and minimized members are rejected. A malformed or
unauthorized group rejects the entire proposal before commit.

The group ID is scoped to the WM connection epoch and output. It conveys no
workspace, column, application identity, pixel access or new input authority.
The Engine does not infer spatial policy from it. WMs omit this extension when
the capability is unavailable; their final placements still work immediately.
Affected-output replacement withdraws omitted hints; unaffected outputs retain
their committed hints. Shell clients need no new endpoint or motion authority.

## Presentation

Engine maintains a shared group translation and a local position per member
(`final placement - group translation`). Both use a critically damped spring
with stiffness 800 and unit mass. Retargeting starts at the current interpolated
position with zero initial velocity, matching the inspected Niri movement path.
Identical targets leave the clock alone. New members start at their local target
under the current camera; existing members can move independently during insertion
or removal. Only position animates; client sizes and committed generations do
not change. Member size/group/output/epoch changes discard the old local motion.

Native output service schedules retained composition at each output's refresh
interval, respects pending frames and Present ownership, and queues a final
settled frame. Direct scanout is suppressed while translation is active.
Page-flip publication supplies input with the geometry of the presented frame,
including its surface chrome. Fixed shell and indicator chrome stays fixed.
Removed members lose their motion state; VT suspension and topology rebinding
settle motion and discard deadlines. Fresh WM epochs cannot inherit old motion.

Changed translation targets also invalidate retained composition when no spring
is active. In particular, a fresh WM epoch must repaint stationary placements
without waiting for client damage or another focus action. The ordinary retained
publication barrier still protects frames awaiting Present retirement. A target
whose size differs from the committed client size keeps the committed geometry
until matching pixels are available; invalidation does not authorize a resize.

Motion is enabled by default on the native GPU composition path. Set
`SOPHIA_ENABLE_WINDOW_TRANSITIONS=0` in the session launch environment to use
immediate placements. This is an Engine setting. It does not alter WM policy.
The debug events `translation_targets` and `translation_settled` record changes
and final-frame scheduling without logging every animation frame.

## Verification and physical acceptance

Offline regressions cover camera retargeting, identical requests, member motion,
output/epoch/size isolation, motion off, pixel identity and presented geometry,
bounded wire decoding and negotiated extension assembly. Hagia independently
checks the shared records and runs camera/navigation/reconciliation regressions.
The backend reload regression retains one client frame across a size change and
restoration, checks repaint admission with motion on and off, and verifies that
the vacated output region is damaged without leaking an offscreen column onto
its neighboring output. It exercises frame planning, not a physical scanout.

Physical acceptance remains pending: open three Kitty windows; move in both
directions, reverse during a transition, insert after the middle window, close
the new window, then close an earlier column. Repeat vertically and on both
outputs. Check clicks against moving content, outer-edge stopping, adjacent
monitor handoff, VT return, and normal logout. Record the installed Sophia and
Hagia identities. This is a focused usage check, not a restart of the 36-row
comparison campaign.
