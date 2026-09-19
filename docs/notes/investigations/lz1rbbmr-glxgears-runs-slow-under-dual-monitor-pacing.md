---
id: lz1rbbmr
date: 2026-09-19
kind: investigation
status: investigating
tags: [investigation, rendering, x11]
---
# glxgears runs slow under dual-monitor pacing

## Question

Why does `glxgears` run slowly (exactly 30 FPS) when launched from the `kitty` terminal in a live session under a dual-monitor setup (DP-1 at 120Hz, DP-2 at 60Hz)?

## Evidence

### Environment and Hardware
- **Session ID:** `00000001789821426620-6a16a92d-bc45-475a-80e7-108b00b0d4ca` (status: `running`, recording: `running`)
- **Primary GPU:** AMD Radeon RX 7900 GRE (`radeonsi`, `navi31`, ACO, Mesa 26.1.8)
- **Active Outputs (`xrandr`):**
  - `SOPHIA-1` (DP-1, connected, primary): `2560x1440@120Hz` (logical `0 0`)
  - `SOPHIA-2` (DP-2, connected): `1920x1080@60Hz` (logical `2560 0`)

### Client Performance Observations
1. Running `glxgears` under display `:77` reported:
   ```text
   Running synchronized to the vertical refresh. The framerate should be approximately the same as the monitor refresh rate.
   152 frames in 5.0 seconds = 30.334 FPS
   ```
2. Running with disabled vertical sync (`vblank_mode=0 glxgears`) resulted in:
   ```text
   ATTENTION: default value of option vblank_mode overridden by environment.
   153 frames in 5.0 seconds = 30.526 FPS
   ```
3. Disabling DRI3 with `LIBGL_DRI3_DISABLE=1 glxgears` similarly yielded `30.458 FPS` under `vblank_mode=0`.

### Active Session Log Analysis
Grepping `events.0.log` during a background `glxgears` run showed both the `kitty` terminal (`client=1`) and `glxgears` (`client=35`) submitting and retiring frames:
- **`glxgears` (surface 73400322) timeline:**
  - Submission 221624 accepted at `1789823704022`
  - Frame retired/presented at `1789823704056` (exactly **34 ms** later)
  - CompleteNotify and Idle write-completion events dispatched to client at `1789823704056`
  - Next submission 221629 accepted at `1789823704056` (synchronous resubmission in the same millisecond)
  - Frame retired/presented at `1789823704089` (exactly **33 ms** later)
- **`kitty` (surface 2097166) timeline:**
  - Frame retired/presented at `1789823704072`
  - Next submission 221630 accepted at `1789823704072`
  - Frame retired/presented at `1789823704106` (exactly **34 ms** later)

This establishes that `glxgears` submits frames in the exact same millisecond it receives the Present `CompleteNotify` event. The client-side rendering takes less than 1 ms, and the `CompleteNotify` event is delivered instantly. The bottleneck is entirely compositor-side composition and retirement pacing.

### Supervised Component Failures
The events log also revealed that `slot=0` (Lom bar) was crash-looping every 1 second:
```text
sophia_shell_component schema=1 status=start_failed
```
Manual execution of Lom with `SOPHIA_SHELL_SOCKET` and `SOPHIA_SHELL_BAR_THICKNESS` confirmed a shell negotiation failure:
```text
lom: shell negotiation failed: Io("Resource temporarily unavailable (os error 11)")
```
This continuous retry/fail cycle creates unnecessary CPU overhead and IPC channel thrashing, further degrading interactive responsiveness.

## Finding and resolution

### The 30 FPS VSync Lock Cause
Under multi-monitor topologies, Sophia's `PrimaryFramePacer` interval is derived from the first head in the scanout heads list:
```rust
let primary_refresh_millihz = native_scanout
    .as_ref()
    .and_then(|native| native.heads.first())
    .map_or(60_000, |head| head.refresh_millihz)
```
If the 60Hz monitor (`DP-2`) resolves as the first head in the scanout table, `primary_refresh_millihz` is set to `60_000`, capping the compositor's composition cadence at exactly **60Hz** (16.66 ms interval).

Under double-buffering (which is the default Mesa DRI3 Present behavior), the client submits a frame and blocks until the previous frame is retired (i.e. until the compositor's KMS page-flip completes). This creates a two-frame feedback loop of latency:
1. Client submits Buffer B.
2. Compositor waits for the next pacer tick (up to 16.6 ms) to compose.
3. Compositor page-flips. Page flip completes on the next hardware vblank (16.6 ms).
4. Buffer B is retired and `CompleteNotify` is sent back to the client (33.3 ms total).
5. The client draws and resubmits Buffer B. However, because it was received *after* the 33.3 ms composition turn started, the client misses the current composition tick.
6. The client's frame must wait until the next pacer tick (at 50.0 ms).

This classic double-buffering latency feedback halving locks the client's frame rate to exactly half of the composition refresh rate (60Hz / 2 = **30 FPS**).

### Resolution Pathways
1. **Pacer Alignment / Per-Head Pacing:** Implement per-head pacing so that the 120Hz screen composes and page-flips at its own native rate, allowing clients mapped to DP-1 to retire at 120Hz/60Hz double-buffered rates instead of being capped by DP-2.
2. **Configuration Override:** Temporarily disabling the second screen (`DP-2`) in `desktop.kdl` removes it from `heads`, forcing `heads.first()` to resolve to DP-1 (120Hz), which lifts the composition rate and raises the double-buffered lock speed to a smooth 60 FPS.
3. **Stop Lom Thrashing:** Fix the Unix socket negotiation lock or disable the panel shell component in `desktop.kdl` when running manual testing to free up CPU and IPC channels.

## Validation and remaining work

### Verification Plan
- [ ] Disable `DP-2` in `desktop.kdl` and verify that the composition rate shifts to 120Hz and `glxgears` frame rate rises to `60 FPS` (the 120Hz double-buffered lock rate).
- [ ] Temporarily disable the `panel` bar in `desktop.kdl` to verify that `start_failed` loops stop and CPU usage is cleaned up.

## Connections

- [Brave watchdog repeats during live use](h0vxis10-brave-gpu-watchdog-repeats-during-live-use.md) — notes on general GPU process and slowness investigations.
- [GLX pixmap exports need coherent backing and reply ordering](uffn76nu-glx-pixmap-exports-need-coherent-backing-and-reply-ordering.md) — details on GLX visuals and Present submission behavior.
