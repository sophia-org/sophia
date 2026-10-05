---
id: jweorh0z
date: 2026-10-04
kind: investigation
status: investigating
tags: [investigation, rendering, validation]
---
# Headless Sophia validation and capture with VKMS writeback

## Question

Can Sophia run a complete desktop on a virtual KMS device and capture its
composed output for CI and remote inspection, without a physical display?
The user requested this backlog item on 2026-10-04.

## Evidence

Source checked at Sophia `41b602000` on 2026-10-04:

- The production desktop backend is `sophia-backend-live`, using DRM/KMS.
  There is no implemented host VKMS/writeback capture path; a search of
  `docs/` and `crates/` found no `vkms` or `writeback` references before this
  note. This needs backend and capture work, not just a configuration change.
- Engine has `HeadlessCompositorBackendAssembly` and CPU composition fixtures.
  Those test facilities do not provide a remotely viewable native desktop.
- `tools/qemu_session_harness.sh` already runs Sophia on virtual DRM devices
  in a headless guest, with a Unix-socket VNC display sink. That can expose
  the guest compositor's display; it is distinct from capturing the host's
  running Sophia session.
- Forwarding individual clients through X11 or waypipe does not expose
  Sophia's composed desktop. A VNC viewer alone cannot capture that desktop;
  it needs a compositor output source, as QEMU supplies for a guest.
- On crunch, `/usr/lib/modules/6.18.54_1/kernel/drivers/gpu/drm/vkms/vkms.ko.zst`
  exists. Read-only `modinfo -p` reports `enable_writeback`, `enable_cursor`
  and `enable_overlay`. The module was not loaded or tested for this note.

## Finding and resolution

VKMS with writeback is a candidate for exercising the native KMS path and
reading back composed frames without a physical display. Module availability
does not prove that Sophia can use it today. Determine whether to extend the
existing backend or add a separate backend after inspecting device discovery,
initial modesets, completion handling and output capture.

Keep the output source separate from a remote viewing transport. First prove
headless composition and capture; then decide how a viewer receives those
frames. Remote input and access policy need their own explicit design.

## Validation and remaining work

### t303

Scope and exit criteria for the candidate:

1. Inventory the virtual card's modes, pixel formats, clock/completion behavior
   and writeback support. Select an explicit virtual device and define setup
   and teardown that leave physical outputs and the live session untouched.
2. Run Session, a WM and clients on that device. Capture actual composed
   output through writeback and compare known pixels, including overlapping
   surfaces and the chosen cursor behavior.
3. Cover capture completion, buffer custody, resize, cancellation and failure
   with bounded tests. Reuse existing QEMU controls where they cover the same
   behavior, and state the additional coverage VKMS provides.
4. Document a repeatable CI command and the remaining work for remote viewing.
   Keep virtual-device correctness evidence distinct from real-GPU performance
   and physical display acceptance.

Task state and priority live in [todo.md](../../../todo.md). No implementation
or architecture choice is admitted by recording this candidate.

## Connections

- [Remote authority candidate](../../protocol-frontend-candidates.md#candidate-3-sophia-remote-authority-headless-streaming-frontend)
  discusses streaming a desktop; this task first establishes its output source.
- [Rendering foundation](../../rendering-foundation.md) separates headless
  rendering evidence from physical acceptance.
