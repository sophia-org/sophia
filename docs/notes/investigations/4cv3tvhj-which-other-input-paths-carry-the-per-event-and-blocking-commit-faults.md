---
id: 4cv3tvhj
date: 2026-09-19
kind: investigation
status: investigating
tags: [investigation, input, rendering, kms]
---
# Which other input paths carry the per-event and blocking-commit faults

## Question

Two faults cost a client frames under pointer motion: **per-event routing** to
the X frontend ([[c4x3drli-pointer-motion-reaches-the-x-frontend-one-event-at-a-time]])
and a **blocking cursor-only KMS commit**
([[0lamaqyi-a-blocking-cursor-only-commit-spends-the-vblank-the-next-frame-needed]]).
Both were repaired for the pointer. Does either shape exist anywhere else in
the input surface?

## Evidence

### The input surface is four kinds

`sophia_protocol::InputEventKind` (`packets/input.rs:25-39`) is the whole of
it: `PointerMotion`, `PointerButton { button, pressed }`,
`PointerAxis { horizontal_v120, vertical_v120 }`, `Key { keycode, pressed }`.
Nothing else reaches a client.

### Keyboard: not exposed, and must not be

Keys route one packet per event (`live_session/input.rs`, the `keys_routed`
sites), which is correct and cannot change: a keystroke is not superseded by
the next one the way a pointer position is, so latest-wins would drop input.
Rate is human, and synthetic repeat is bounded by the configured interval --
25 ms by default (`sophia-config/src/types.rs:229-230`), about 40 a second
against a 120 Hz frame. There is no pressure here to relieve.

### Scroll: the same shape, an order of magnitude smaller, and not fixable the same way

`PointerAxis` is routed per event and is **deliberately excluded** from the
coalescer: `coalescible_motion_key` (`sophia-engine/src/input/routed.rs`)
matches `PointerMotion` only, so an axis packet flushes as state-changing
input. That is the same per-event delivery the pointer had.

Two things bound the concern. Device rate is the report rate of a wheel or
touchpad -- on the order of 100-125 a second, against the 1000 a second a
gaming mouse produces -- so it sits near parity with a 120 Hz frame rather
than eight times over it. And the repair cannot be copied: each axis packet
carries a **delta**, so latest-wins would lose scroll distance. Collapsing
them means *summing* within a frame, which is a different operation with its
own semantics -- discrete notches versus continuous motion, and `v120`
fractions that clients accumulate themselves.

Unmeasured. No run here has driven a high-rate scroll device the way the
synthetic shake drives motion, so whether this costs anything is unknown
rather than answered. Tracked as t121.

### Touch and tablet: not routed at all

The libinput poller counts touch devices (`policy.touch_devices`,
`input/libinput/native.rs:182`) and nothing routes a touch event, because the
event surface above has no variant for one. This is not a performance question
but a missing capability: a touchscreen is inert under Sophia today. Tracked
as t122, on a Lenovo X13 that has one.

Worth noting for whoever implements it: touch motion has the pointer's shape,
not the keyboard's -- a contact position is superseded by the next one, at
device rate. It should arrive with per-contact latest-wins coalescing on the
frame boundary from the start, rather than repeating this repair later.

### The blocking commit has no sibling at pointer rate

Every `.blocking()` atomic commit in the backend:

| site | path |
| --- | --- |
| `native_primary_plane/request.rs:404` | the cursor-only commit, now gated |
| `native_primary_plane/multi_head_submit.rs:90,129` | topology change (modeset) |
| `native_scanout/prepare.rs:439` | applies a policy; blocking only for `blocking_modeset` |
| `hardware_validation/mirror_probe.rs:354` | startup probe |

Modesets and probes are rare and expected to block. The cursor-only commit was
the only one reachable at the rate a hand moves a mouse, which is why it was
the only one that cost frames.

## Finding and resolution

**Answered.** The keyboard is not exposed. The blocking-commit fault has no
sibling: every other blocking commit is a modeset or a probe. Scroll carries
the per-event shape at roughly a tenth the rate and cannot take the same
repair, because axis packets are deltas that would have to be summed rather
than superseded -- unmeasured, and worth measuring before changing (t121).
Touch is not routed at all, which is a capability gap rather than a
performance one (t122), and should be built with frame-boundary coalescing
rather than acquiring it later.

## Validation and remaining work

- [ ] Measure scroll under a high-rate device the way the shake measures
      motion, before deciding whether summing axis deltas per frame is worth
      its semantic cost (t121).
- [ ] Route touch, with per-contact latest-wins coalescing on the frame
      boundary (t122).

## Connections

- [Pointer motion reaches the X frontend one event at a time](c4x3drli-pointer-motion-reaches-the-x-frontend-one-event-at-a-time.md) —
  the per-event fault, repaired for motion.
- [A blocking cursor-only commit spends the vblank the next frame needed](0lamaqyi-a-blocking-cursor-only-commit-spends-the-vblank-the-next-frame-needed.md) —
  the blocking-commit fault, which this audit finds has no sibling.
