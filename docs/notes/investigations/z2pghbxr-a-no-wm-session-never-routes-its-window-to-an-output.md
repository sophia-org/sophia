---
id: z2pghbxr
date: 2026-09-19
kind: investigation
status: investigating
tags: [investigation, rendering, session, placement]
---
# A no-WM session never routes its window to an output

## Question

The glxgears benchmark's session -- a standalone, no-window-manager profile --
fails to start: the single 500x500 window never reaches an output, so startup
times out and the client renders under 1 FPS. It last worked on 1 September.
What stopped placing the window, and why only without a window manager?

## Evidence

Reference host, `just glxgears-shake` (equivalently `benchmark_sophia_glxgears_tty3.sh`),
session `00000001789837427705-3029f01f...`, running this tree's binary
(`cf68ea92`). The preflight passes -- the earlier capability regression is
fixed -- and the session itself is what fails now.

```
native desktop output candidate admitted status="prepared_not_applied" cause="would_block"
GL_RENDERER = AMD Radeon RX 7900 GRE (radeonsi, navi31 ...)   # the client started
first-visibility budget expired; ... surface=2097154 reason=NoApplicableOutput   # x3, 2s apart
5 frames in 6.1 seconds = 0.826 FPS                            # the client, starved
sophia_live_session_startup schema=3 status=failed stage=not_committed elapsed_msec=8000 focus=false
Error: "startup application was not visibly presented within 8000 milliseconds: stage=not_committed"
```

The window maps (`sophia_x_window_lifecycle ... role=PolicyManaged mapped=true
width=500 height=500`), Present submissions are accepted, but every first
candidate is parked `NoApplicableOutput` and expires.

### The mechanism, from the source

`present.rs:88-103` computes the outputs that owe a surface's retirement,
retains only those it `live_surface_routes_to_output`, and parks the first
candidate `NoApplicableOutput` when the set is empty
(`production_visual_runtime/present.rs`). Routing
(`compositor_graphics.rs:184-187`) is:

```rust
match surface_outputs.get(&surface) {
    Some(owner) => *owner == output,          // a policy/WM output assignment
    None => geometry_routed.contains(&surface), // the geometry fallback
}
```

- `surface_outputs` is filled only from `layer.output`
  (`production_visual_runtime.rs:1166-1168`), and a layout layer is created with
  `output: None` -- "the proposal sets the owner" (`wm/layout.rs:511`). The
  proposal is the WM's. **No WM, no owner.**
- `geometry_routed` comes from the session
  (`owner_loop/authority_production.rs:96`):
  ```rust
  let geometry_routed_surfaces = presentation_layout.iter()
      .filter(|layer| layout.is_client_positioned(layer.surface))
      .map(|layer| layer.surface).collect::<Vec<_>>();
  ```
  `is_client_positioned` is true only for `ClientPositioned`
  (`wm/layout.rs:671-674`). The glxgears window is `PolicyManaged`, so it is
  **not** geometry-routed either.

A `PolicyManaged` surface in a session with no window manager therefore
matches neither arm: no output owner and not geometry-routed. It routes to no
output, so no output owes its retirement, so it is parked `NoApplicableOutput`
until the 8 s budget expires. The session had `engine_owns_initial_placement`
(Direct policy-map mode, `from_external_wm(false)`), so the engine centred its
geometry -- but centring sets geometry, not an output owner, and routing is by
owner-or-geometry-set, not by geometry.

### The regression

`4eb1136a` (2026-09-05, "Preserve live panel routing and first-frame
presentation ownership") introduced `geometry_routed_surfaces` and the field
doc states the assumption directly: *"Visible frontend-positioned surfaces,
explicitly authorized by the session. All other surfaces require a policy
output assignment."* That holds when a policy client exists. A no-WM session
is exactly the case where nothing can make that assignment, and the engine is
meant to own placement instead -- which the commit did not account for. The
only recorded `profile=direct` outcome after that date is this failure; the
last success was 1 September, before it. The path has not run in the interval,
the same reason the DRI3 capability preflight rotted unnoticed
([[12tnf6wc-an-offscreen-client-is-never-throttled-and-its-evidence-evicts-everything-else]]
records a sibling gap: this whole family of standalone-only paths has no gate).

## Finding and resolution

**Established.** A `PolicyManaged` surface with no window manager reaches no
output because routing needs either a policy-assigned owner or membership in
the client-positioned geometry set, and a no-WM session produces neither.

The repair, to establish before implementing: in a session that owns initial
placement (Direct policy-map mode, i.e. no external WM), a `PolicyManaged`
surface must be geometry-routed, because there is no policy to assign it an
output and the engine is already the one placing it. The narrow change is at
`authority_production.rs:96`: route by client-positioned **or** engine-owned
placement. The layout already carries the discriminant it needs
(`bypass_policy_admission`, true exactly for Direct); it would gain an
`is_geometry_routed(surface)` returning `is_client_positioned(surface) ||
engine_owns_initial_placement`. The alternative -- assigning the surface an
output owner at engine placement time -- is larger and touches the WM
proposal path; routing is where the omission is.

This is the compositor's placement path, and a fault here is a window on the
wrong head or a session that will not start, not a failing test. The no-WM
standalone path has no automated coverage at all, which is why it broke
silently; the fix must add it.

## Validation and remaining work

- [ ] Route a `PolicyManaged` surface to geometry in a no-WM session and prove
      the standalone glxgears session reaches `bounded_complete` again, with
      `stage=not_committed` gone.
- [ ] Add a test that a Direct-placement session commits and presents a single
      `PolicyManaged` surface -- the coverage whose absence hid this.
- [ ] Gate the standalone glxgears benchmark, or its session preflight, so a
      no-WM regression fails a run rather than a hand at a TTY.
- [ ] Re-run the shake harness, which this blocks, and finally read the
      surviving halving under a scripted shake.

## Connections

- [Pointer motion reaches the X frontend one event at a time](c4x3drli-pointer-motion-reaches-the-x-frontend-one-event-at-a-time.md) --
  the halving the shake harness was built to measure, which this blocks.
- [An offscreen client is never throttled and its evidence evicts everything else](12tnf6wc-an-offscreen-client-is-never-throttled-and-its-evidence-evicts-everything-else.md) --
  a sibling standalone-only gap, and the same "no gate ran it" cause.
