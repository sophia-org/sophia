---
id: 746b2np8
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, session, gpu, acceptance]
---
# Contained GPU proof separates observed device exclusion from native retirement

## Question and scope

Which parts of [t097](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md#t097)
are implemented or independently observed, and what is the smallest useful
current contained proof? This investigation starts from accepted `caca7e37`.
No hardware execution, live session, installation, main-tree operation or task
closure is authorized by this checkpoint.

## Evidence reconciliation

| Exit | Reusable evidence | Limit or remaining check |
| --- | --- | --- |
| Explicit default-denied policy and old quota migration | Existing config policy, typed `ShellGpuLaunchPolicy`, exact inode/device handoff and GPU model | Current focused checks pin the implementation; permission is not a VRAM quota |
| Selected adapter and real rendered bytes | Retained September 18 preflight, exact DRM identity and two nonuniform Vulkan outputs | Evidence belongs to those exact binaries and driver |
| Exclusion | Retained Lom record observed exactly one DRM entry | It did not observe every named input/display path or inherited descriptor |
| Resource credit | Held render lease survives disconnect; dropping it permits zero resource/backing credit | First Presented was synthetic; no physical native frame retired |
| Production retirement | Existing component/backend controls exercise real owners with simulated completion | No simulated completion establishes a physical KMS result |
| Kernel prerequisites | Current package/kernel/config inventory and real harmless protected-child launch | No custom GPU-quota kernel patch is required; inventory is not cryptographic attestation of the running kernel |

The retained preflight is
`sophia/.artifacts/lom-panel-native/20260918T170952Z/gpu-content`.
Its manifest pins Sophia `4049fea8f265e50ae6f72976e011f68231e72e50`
(binary SHA256 `492698b92eaf6229cc624030e2f1c7f749794379f2ee1a139d43a3dcb31d0234`)
and Lom `2b791037887ec75b31fa67265f6b41b008a458fe`
(binary `48f85c5c3d6b5c6bc55a371406b5b6b125d231cf242ab4cfb9c4cce5edec1919`).
The Minimal config hash is
`a0c88fc211df32888b8ec19df06dd839f925adcbdb5e0fc7a73b71a22db754fa`.
The current legacy-mode verifier accepts the retained schema-2 log.

That log reports RADV on RX 7900 GRE, PCI `0000:03:00.0`, vendor/device
`1002:744c`, exact DRM `226:128`, and only `renderD128` in the private DRI
inventory. It contains two 24,576-byte 256-by-24 nonuniform renders. The first
completion is expressly `presented_synthetic`; the second is RendererFailed 9.
The runner retains a source lease through disconnect, then drops it and checks
both source and resource-backing credit reach zero. Later session failures do
not undo these narrow successful measurements. They also do not convert this
preflight into native-session acceptance.
The old parent's discovery API also opened other render devices on the seat;
the retained child observations are not evidence of selected-only parent access.

## Current proof boundary

The proof still uses the production `ProtectionDomainSpec`, GPU policy,
`ProcessSupervisor`, protected-peer authentication and shell content transport.
Only the isolated proof starts the CLI's observation mode before Lom. After
bounded checks, `Command::exec` replaces that observer with the pinned client,
preserving PID, namespace and grant. No normal session path, backend API, wire
format, device-negotiation owner or scheduler changes.

The observer checks the selected character device's actual `dev_t`, exactly
one DRI entry, absence of `/dev/input`, `/tmp/.X11-unix` and `/run/user`, and
absence of DISPLAY, XAUTHORITY, WAYLAND_DISPLAY and WAYLAND_SOCKET. Recursive
device and inherited-descriptor inventories each have a 64-entry bound.
Extra hardware devices, block devices and inherited sockets refuse. Standard
null/zero/full/random/urandom pseudo-devices are allowed; the private device
inventory additionally permits the standard tty/ptmx pseudo-devices. Inherited
tty/ptmx descriptors are not allowed. These allowances do not grow for fixtures.
The `inherited_devices=none` record means no nonstandard/hardware device, not
absence of the permitted stdin null descriptor.
Descriptor classification uses file type: anonymous-inode objects such as
dma-buf, sync_file, drm_syncobj, eventfd, pidfd and memfd are not classified by
this check. It does not establish absence of inherited GPU objects. Protection
builder inheritance remains a separate mechanism and control.

The observation is not admission authority. A fresh random observation ID is
correlation data. The parent emits a matching binding only after the existing
protected peer is authorized and negotiation succeeds; that binding names its
host peer/supervisor PIDs, device and grant epoch. The verifier's
evidence for those host PIDs is the parent record's position after authorization,
not an independent correlation with the child's namespace PID. The verifier's
`--require-domain` mode requires exactly one observation and one matching parent
binding, alongside the existing actual Lom adapter and completion records.
Grant environment alone cannot satisfy this evidence path. The checks describe
the enumerated resources at exec, not all future driver behavior. Sysfs,
other filesystem and network containment continue to come from the production
domain contract and its focused controls; the `/dev` inventory is not an
exhaustive access-exclusion proof.

`SOPHIA_LOM_GPU_EXPECTED_DEVICE=226:128@0000:03:00.0` additionally refuses a
different selected identity before launching the child. The proof parent uses
`snapshot_seat_render_inventory`, the existing metadata-only inventory, rather
than the API that opens every render device on the seat. It selects the exact
node, revalidates node identity and
one-device sysfs projection, and carries its validated inode/device identity
into the launcher. This narrows replacement races; it does not atomically pin
all hotplug transitions.

The old shell build launcher is not the proposed entrypoint: its target-directory
and worktree `.git` assumptions remain outside this slice. The reviewed command
uses copied, checksummed prebuilt binaries directly. A new observation is
required for the new gate; the historical log is not rewritten or reclassified.

## Prerequisites and accounting limits

Read-only inventory reports Void Linux, kernel `6.18.52_1`, installed package
`linux6.18-6.18.52_1` / source revision `a52dd0f42ba`, bubblewrap 0.13.0,
kernel taint 0, user namespaces available, the standard namespace/cgroup config
options enabled, and DRM/AMDGPU modules configured. `prerequisites.json` retains
exact values and the installed kernel-config checksum. Actual protected-child
controls exercise these standard mechanisms without a GPU. This is not an
attestation that no local kernel bytes changed; other driver combinations remain
untested.

The existing GPU model proves logical grant freshness and current authority in
its finite model. Its Stop transition is not a promise that a blocked kernel
returns. Source/resource backing is conservative per accepted resource; copied
head buffers and driver VRAM are different owners. Neither the isolated
RendererFailed release nor the component fixture's simulated retirement can
close the outstanding physical native-retirement exit.

## Validation and proposed execution

Worktree evidence lives in `sophia-borders/.artifacts/t097-audit`. Current baseline
checks pass: ten GPU policy/projection tests, ten protection controls and fourteen
resource-accounting controls. The pinned `ShellGpuLaunchAdmission` model passes;
its two deliberate faults violate ActiveGrantNamesCurrentAuthority and FreshGrantEpoch as
expected. The existing verifier corpus passes, and the original CLI refuses an
unarmed invocation without accessing a device.

New controls cover a real protected harmless child using `/dev/null` as a fake
selected character device, then exec of a marker client. Extra/missing DRM,
input/display paths, display environment, wrong device identity and an oversized
inventory prevent exec. A direct invocation with grant environment alone is
refused. Supplied descriptor-directory controls check socket rejection and the
exact inventory bound. These controls do not claim a Lom/GPU process ran.
Compiled negatives bypass device observation or permit an inherited socket;
both fail their intended assertions. Verifier mutations exercise missing,
duplicate and mismatched parent/child records.

Before the metadata-only inventory correction and CLI domain extraction, full
CLI/Session all-feature suites passed 1,033 tests with 41 ignored; their default
suites passed 359 with 21 ignored. Both had zero failures. After that correction,
five focused Session controls, the protected CLI fixture, default all-target
compilation, strict all-feature/all-target Clippy and the verifier corpus pass.
The inventory check in that focused run matched source text, not hardware
syscalls. Integration removes that check because it repeats the implementation;
the metadata-only call boundary is established by source review. The behavioral
identity and protected-child controls remain. Three profile permission/migration
controls pass. Formatting, whitespace,
metadata and layout pass. The initial layout failure (test cfg placement and
the CLI file crossing its ceiling) is retained; no debt limit was raised.

Exact source, binary, config and command identities accompany the signed
candidate evidence. Main integration and hardware execution are separate gates.

The proposed outer wall deadline is 30 seconds with a five-second forced-stop
grace; the internal content loop has a 15-second deadline after negotiation,
which itself is bounded. `ProcessSupervisor::Drop` requests termination and
polls reaping, but can remain blocked by an uninterruptible kernel task. A timeout
is a failed proof with potentially unresolved cleanup, never a success or a
claim that kernel/VRAM resources were forcibly freed. Keep the process identities
and logs for operator inspection. No main compositor, TTY, KMS card or input
seat takeover is part of this command.

A pinned execution script/manifest will be supplied for review, not executed.
It clears live-display and smoke variables, redirects stdin to null and both
stdout/stderr to the evidence log (never a terminal), uses the
explicit device expectation, pins both binaries and the Minimal config, and
requires the domain-aware verifier. Physical execution still requires separate
authorization. Broader t097 retirement evidence and all other task exits remain
open.

## Connections

- [GPU permission decision](../decisions/mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md): explicit execution permission is independent of content and hard quotas.
- [Critical path](../plans/1m3z9q0j-lom-and-sophia-portable-gpu-shell-critical-path.md#t097): owns the acceptance exits.
- [Reconnect investigation](vup982br-retained-panel-pixels-can-block-fresh-component-admission-after-disconnect.md): retained source credit and simulated production-owner retirement remain distinct.
