---
id: kgo1ugnz
date: 2026-10-02
kind: plan
status: proposed
tags: [plan, milestone]
---
# Held capture and blind WM capabilities for niri parity

## Scope and exit

niltempus asked for Hagia's recent-windows switcher to behave as niri's does.
Hagia tracks the work as h009-h015 (Hagia plan n6dx1wbt). Five of those items
need Sophia mechanisms. Each mechanism is generic and additive, behind its own
WM capability bit at interface revision 3, and none names a client or a key.

The WM stays blind (`docs/sophia-wm-api.md`): titles, classes and PIDs never
reach it. Window identity reaches the WM only as opaque equality tokens, and
text only as labels that Sophia draws. Each task follows the same order as
the t277 chording work:

1. contract, with its generated rows;
2. C SDK release and Rust doc import;
3. Sophia vendor pins;
4. Sophia implementation;
5. the matching Hagia repin and behaviour.

Exit for each task: its scripted, Session and mutant evidence, and then the
operator's live check of the matching Hagia behaviour. Live acceptance is
not claimed by this plan.

## Task details

<a id="t279"></a>
**t279: held capture (bit 22), admitted.** Peer: hagia/h009.
- A presentation whose covered outputs are all Overlay may carry a keyboard
  output and bindings when `held_capture` is selected. The bit requires
  `surface_instances` and `presentation_actions`.
- The capture never takes a modifier key, so the focused client keeps both
  edges and chords end as before. Other presses are matched exactly or
  consumed; chord-followed presses stay with the shortcut authority.
- Only a non-modifier key that an application already holds delays the
  capture. While shielding, the modifier rule follows the presented pixels'
  scope until every head retires a change: per-head
  `PresentedKeyboardScope`, Modal over Held over none, with an unknown head
  counting as Modal.
- Contract ac04e1a7; C SDK 0.7.0 (4608010f, tag v0.7.0); Rust import
  ed364d40; Sophia pins f0e40cc0 (C) and f67e6f11 (Rust); implementation
  55830dda.
- Frozen evidence: development-evidence/niri-parity-01 (PACKAGE-02, root
  reviews 01/02, sign order 03, sign/). Post-vendor gate on the exact
  implementation tree (sign/05-sophia-post-vendor-gate): fmt, layout,
  generator, both SDK checks and strict clippy pass; the workspace with all
  features and --no-fail-fast has 6238 passed and 0 failed.
- Live acceptance passed on 2026-10-07. On the installed release
  `niltempus-44f76a7d05b2b8264a63` (Sophia `825d9146`, Hagia `155daab6`), on
  two monitors, niltempus held Alt+Tab and confirmed every held key: Escape,
  Return and space, Left and Right, Home and End, Tab and Shift+Tab, and the
  scope keys a, w, o and s. xev in the focused window saw only Alt's press and
  release, and the strip behaved the same on both monitors. The session's
  recovery log stayed empty with no fatal record. The record is
  `attended-closures-01/RESULTS.json` in the development evidence. t279 is
  closed.

<a id="t280"></a>
**t280: surface attention (bit 23), candidate.** Peer: hagia/h015.
- The X authority parses WM_HINTS UrgencyHint and
  `_NET_WM_STATE_DEMANDS_ATTENTION` into one per-surface snapshot state bit.
  This is behaviour, not identity.
- Adds an Attention presentation region role, coloured by Engine.

<a id="t281"></a>
**t281: opaque surface groups (bit 24), candidate.** Peer: hagia/h012.
- One extension record per surface carries a per-connection-epoch keyed token
  of the reduced WM_CLASS class.
- Equal classes give equal tokens. The token is never a name and is not
  stable across epochs.

<a id="t282"></a>
**t282: compositor-drawn surface labels (bit 25), candidate.** Peer:
hagia/h013.
- A SurfaceLabel region references an instance target. Sophia fills it from
  the broker descriptor, under the user's disclosure policy (class-only by
  default).
- Sophia draws the label with the existing CompositorText path, and the WM
  never receives the text.

<a id="t283"></a>
**t283: policy deadlines (bit 26), candidate.** Peer: hagia/h014.
- One outstanding `{token, delay 1..60000 ms}` request per connection. It
  produces a Deadline cause when due, is replaced by a newer request, and is
  dropped on an epoch reset.
- It is merged into the owner wait on the owner clock. It is bounded and
  explicit, not polling.

t280-t283 are planning candidates outside the current release.

## Connections

- Hagia plan n6dx1wbt (niri parity for the recent-windows switcher) and
  h009-h015.
- t277 generic chording: the chord lifecycle, ChordAction and the protected
  routing that held capture builds on.
- Evidence: development-evidence/niri-parity-01 (packages, root reviews,
  sign order).
