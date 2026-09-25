---
id: 09ywbgwt
date: 2026-09-08
kind: investigation
status: closed
tags: [investigation, rendering, x11]
---
# Measured modifiers exposed an RGB stride check on compression metadata

## Incident and reproduction

After `33e81b8f` began advertising measured DRI3 modifiers, newly started sessions
admitted Kitty and Quickshell windows without visible app content. The user
reported the same failure after the topology and VT follow-ups through
`5affd852`. Session `00000001788911152132-e75d5a37-2b10-4df0-a49c-864ff2ff509e`
ran the installed `0.1.0-5affd852c7a7` binary; this was not a stale-build report.
Kitty and Quickshell processes remained alive.

A private `XServerFrontend` using the production render-device provider,
pixmap allocator and measured `/dev/dri/renderD128` capabilities reproduced the
failure with system Mesa and `glxgears`. It used no KMS, live display, input or
session restart. Mesa submitted two DRI3 `PixmapFromBuffers` requests, each with
two FDs. Both received core X error 3; neither reached Present. Temporary
instrumentation at the descriptor validator captured `InvalidStride` for:

| Field | Value |
| --- | --- |
| Image | XR24, 300 × 300 |
| Modifier | `144115188757872388` |
| Plane 0 | offset 0, stride 2048 |
| Plane 1 | offset 1048576, stride 1024 |

The descriptor validator applied `width * 4 = 1200` to every plane. The second
plane carries compression metadata, so its 1024-byte pitch is not another RGB
image row. The diagnostic run is recorded in
`/tmp/sophia-first-buffer-before.log`; this is a temporary observation path,
not a durable archive. The instrumentation was removed after the capture.

## Cause and boundary

`DmaBufDescriptor::validate` incorrectly inferred every plane's extent from the
image's width and height. The measured modifier change exposed this latent
defect by allowing Mesa to select compressed buffers. A one-output-device
reproduction establishes the failure without the proposed multi-device
modifier intersection mechanism.

The fix belongs to generic descriptor validation and the received-FD boundary.
Explicit modifiers may change layout, allocation size and plane count; the
driver that imports the buffer owns their interpretation. Protocol validation
retains handle, dimension, format, plane-shape, nonzero-stride and byte-budget
checks. Linear and legacy implicit descriptors retain their existing row
bounds. Opaque planes retain bounded offsets and pitches without extrapolating
image-height rows. The socket checks their actual FD lengths before creating a
pixmap, without mapping memory or moving a shared file offset. Native import
still has to succeed before the pixels can be used.

No device names, application identities or modifier-specific decoding enter
Engine. The negotiated modifier list and the direct GPU path remain enabled.

## Corrections to earlier reasoning

The logs contained four leased frame slots before the VT switch. Reading only
their tail and reporting zero in every sample was wrong. Absence of app
retirement records did not prove that no scanout work had been submitted.
The topology and VT edits did not establish a rendering repair, and the user's
repeated failures refuted that acceptance claim.

The two-GPU intersection theory did not explain the one-device reproduction.
Omitting an implicit sentinel from explicit modifier advertisement also does
not prohibit legacy implicit imports. Neither theory justifies inventing
fallback modifier support.

## Validation and limits

The pre-fix real-client test fails before the first Present, with the exact
descriptor rejection above. Protocol tests cover opaque auxiliary planes and
preservation of malformed-descriptor and packed-row refusals.

Both post-fix GLX and EGL clients submitted the exact two-plane descriptor above
and passed native capture, composition and readback of all 90,000 pixels. The
same pixels remained exact after client exit and frontend teardown; the retained
image then evicted cleanly. The combined run in
`/tmp/sophia-first-frame-after.log` explicitly required and reported an auxiliary
stride smaller than the color row. `tools/check_client_first_frame.sh` reproduced
both passes and is now part of `cargo xtask check`; it rejects an empty or partial
test selection. These are GPU pixel proofs, not merely resource admissions.

The socket suite also proves positive geometry replies for shared and separate
plane allocations. Offsets at or beyond their own FD's end, lengthless FDs and
allocations above the byte cap refuse with correlated `BadValue` errors. A
subsequent import at the same XID on the same connection succeeds, and file
positions remain unchanged.

`SOPHIA_FIRST_FRAME_REQUIRE_AUX=1 cargo xtask check` passed in full: workspace
tests, formatting, clippy without warnings, archive verification, buffer-age
pixel equivalence, both client first-frame pixel checks and verifier fixtures.
The repair is signed commit `009498fb0aae5baedc2b98a1142f06ba448c5de6`,
based on `5affd852`.
Before/after logs, the standalone hardware gate and the full check are retained
under `.artifacts/t069-first-frame-20260908/`; the immutable release manifest
records packaged source and binary identity. New note links resolve. The
notebook-wide broken-link query still lists two unchanged pre-existing notes.

## Physical acceptance

Session `00000001788913317427-5bb740cc-5302-444b-bf91-7f8204bb0f09`, started
on 2026-09-08, runs installed release `0.1.0-009498fb0aae`. The running
process and `/opt/sophia/current` independently identify that release. A
read-only sample recorded 914 retired frames for surface `4194318` and 96 each
for `2097170` and `2097173`, with recording active, no discarded records and no
storage errors. The identity and sample are retained in the evidence directory's
`live-session.json`.

niltempus explicitly confirmed that Kitty and the Quickshell bar are visible and
responsive in this session. This accepts the blank-app regression repair.
It does not establish Brave video, cross-device migration, device-loss recovery
or the other exits of [t069](../../../todo.md), which remains open. The
investigation's probes and diagnostics did not inject live input or restart the
desktop.

## Connections

This repairs the regression exposed by the
[measured capability checkpoint](../milestones/f24uuvwu-measured-dri3-import-capabilities-and-exact-legacy-exports.md)
within the [universal negotiation plan](../plans/6tewvlbh-universal-device-negotiation-across-sophia-clients.md).
It preserves the [DRI3 contract](../../sophia-x-authority.md) and does not address
the separate [client-internal media-device mismatch](uqnx2t2b-brave-gpu-restarts-after-va-buffers-fail-gbm-import.md).
