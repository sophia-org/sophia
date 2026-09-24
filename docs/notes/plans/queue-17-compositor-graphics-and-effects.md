---
id: queue-17
date: 2026-09-06
kind: plan
tags: [plan, milestone]
---
# Compositor graphics and effects

This plan retains the scope, constraints, and task details from the roadmap
cutover. Task status and order live only in [todo.md](../../../todo.md)
and the [monthly completion history](../../../done.md). Follow the
[work-tracking contract](../../work-tracking.md).
Historical candidate identities in the details require revalidation before use.

[Parent scope](queue-12-candidate-queue.md).



## t047

Before broadening the shell schema with effects, model capability admission,
bounded parameters, supersession, Engine-clock cancellation, provider
absence/failure, deterministic fallback, and atomic multi-head presentation.


## t048

Implement one protocol-neutral Engine effect registry and private
build-linked provider seam. Prove one scene-sampling effect and one
Engine-clocked transition, including damage/pixel gates and direct-scanout
fallback. Do not expose shader programs or reopen frozen WM revision 3.


## t049

Settle remaining display-list vocabulary from a driving client: generic
target regions, desktop background, and only measured additions beyond
client-rasterized textures.

Background work means an authorized placement/lifetime contract, not a wallpaper
application or a new drawing primitive for images clients can rasterize. Reuse
t109 admission and existing content resource owners. Exit: below-application
placement, output/topology replacement, denied/foreign grants, input exclusion,
bounded ownership and disconnect/retirement tests with a minimal producer.


## t050

Retain only measured rendering follow-ups: cross-drawable `CopyArea`, bounded
raster storage, upscale filtering, linear-light blend/opacity, mirror remode,
presented-extent raster demand, CPU GBM pooling, configurable semantic cursor
themes (theme, nominal size, named shapes, hotspots, and deterministic
fallback), concurrent producers, and equal-mode scanout cloning. Comparison
profiles must pin one cursor theme and size across Sophia and references.

Cross-drawable `CopyArea` (done 2026-09-24): a copy from a pixmap into a
window, which is how every double-buffered client presents, poisoned the
window's density journal as `UnsupportedCrossDrawableCopy`. It is now
journaled as the source pixels it wrote, inside the source's bounds, under
PutImage's retention rule, so an unconditional copy replays exactly and
anything else is still refused. Red first:
`a_copy_from_a_pixmap_replays_the_pixels_it_carried` in
`x11_wire/replay_stacking.rs` fell back before and replays exactly after.
