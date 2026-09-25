---
id: uqnx2t2b
date: 2026-09-07
kind: investigation
status: resolved
tags: [investigation, rendering, x11]
---
# Brave GPU restarts after VA buffers fail GBM import

## Question

Why does Brave lose its GPU process during GBM buffer import after installing
the AMD VA-API backend? Identify the failing descriptor and device before
changing Sophia's renderer or selecting a browser workaround.

The [2026-09-10 producer trace](#producer-attribution-and-stock-chromium-on-2026-09-10)
now correlates the failed buffer with its VA allocation on the integrated GPU.
The exact descriptor imports on that GPU and fails on the discrete GPU,
regardless of the scanout flag. Stock Chromium reproduces the same device split.
The normal-launch repair remains outstanding.

## Observed on 2026-09-07

Installed Sophia is `a44d1e163ceb7188edfebefc9871e1517576f07b`. Brave Origin
1.94.121 uses Chromium 152.0.7977.83. System Mesa, libgbm and the newly installed
mesa-vaapi package are version 26.1.8_1.

The user's `~/brave-vaapi.log` records successful loading of
`radeonsi_drv_video.so`, VA-API 1.24.0 and `va_openDriver() returns 0`.
Installing the driver resolved the earlier initialization failure. This does
not establish working hardware video playback.

Four GPU processes then fail at `gbm_wrapper.cc:465`: 21733 at 20:17:10,
24349 at 20:22:17, 5853 at 20:22:21 and 6666 at 20:22:22. Each failure is
followed by shared-image backing creation failure, lost context and GPU-process
exit 8704. Replacement GPU process 6919 runs with `--use-gl=disabled` and holds
no DRM descriptors. Its later failure to find a VA render node follows that
fallback; it is not evidence of the original cause.

PID 22962 is a renderer. Its error-level `DecoderStatus::0` record denotes
success, not a decoder error. The failed GPU processes have exited, so their
device descriptors cannot be reconstructed from the current process list.

## Source and device evidence

The matching [Chromium GBM wrapper](https://chromium.googlesource.com/chromium/src/+/152.0.7977.83/ui/gfx/linux/gbm_wrapper.cc)
reaches the reported error after its format-support check and a failed
`gbm_bo_import` with `GBM_BO_IMPORT_FD_MODIFIER`. Its error omits the descriptor,
importing device and `errno`. The log cannot distinguish a bad descriptor from
an unsupported modifier or device pairing.

Brave's ELF dynamic table names `libgbm.so.1`; its undefined dynamic symbols
include `gbm_bo_import`, `gbm_create_device` and `gbm_device_get_fd`. The current
GPU process maps system Mesa's `/usr/lib/libgbm.so.1.0.0`. This supports an
import-call interposer for diagnosis. It does not by itself establish which
compile-time branches Brave enabled.

| Node | PCI device | GPU |
| --- | --- | --- |
| renderD128 | 0000:03:00.0, 1002:744c | Navi 31, discrete |
| renderD129 | 0000:16:00.0, 1002:164e | Raphael, integrated |

A private VA allocation and GBM import probe exercised NV12 at 640×480 and
1920×1080, without an X connection or KMS changes. Same-device imports
succeeded on both nodes with flags 0 and SCANOUT. All cross-device imports
failed with `errno=38` (ENOSYS). Exported modifiers were
`0x0200000028a01f04` on renderD128 and `0x0200000000401b03` on renderD129.
These are different AMD tiling layouts.

This demonstrates an incompatibility, **not Brave crash attribution**. The
probe's NV12 format-support query returned false even where import succeeded;
Chromium checks support before the failing call. The browser's actual format,
modifier and importing node remain essential evidence.

The [VA wrapper](https://chromium.googlesource.com/chromium/src/+/152.0.7977.83/media/gpu/vaapi/vaapi_wrapper.cc)
filters render nodes against the active GPU only when passed a non-null
`GPUInfo`. Unconditional pinning has not been established. Also,
[platform_video_frame_utils.cc](https://chromium.googlesource.com/chromium/src/+/152.0.7977.83/media/gpu/chromeos/platform_video_frame_utils.cc)
reads `--render-node-override`: it is not a VA-only control. Two override runs
would not establish that only the VA display's device changed.

## Diagnostic prepared

Temporary artifacts are `/tmp/sophia-va-gbm-probe.py` and its `.log`, plus
`/tmp/sophia-brave-gbm.KpbC7NfW/` for a GBM interposer and its fixtures.
`/tmp/sophia-brave-gbm.sh` launches a fresh diagnostic Brave instance when the
user runs it. These are working artifacts, not a durable evidence archive.

The interposer records the importing DRM device number, dimensions, fourcc,
modifier, plane count, strides, offsets, result and returned `errno`.
It never maps buffer pixels. It records the first 64 imports and all subsequent
failures. A linked fake GBM fixture verified unchanged arguments, return values
and `errno`, the successful-log bound, and failure capture after that bound.
Compilation passed `-Wall -Wextra -Werror`.

The launcher creates a private, uniquely named log, retains existing preload
entries and browser sandbox settings, and uses the wrapper's `-no-update --`
argument boundary. Check-only mode was verified against the host process list:
it refuses while Brave remains open. The restricted tool environment hides
those host processes, so its apparent empty list was not used as evidence.
No browser was launched, stopped or reconfigured by this investigation. A real
browser run has not yet proved that sandboxed import calls reach the interposer.

## Instrumented restart and exact-format reproduction

The user restarted through the diagnostic launcher. The wrapper reached the
real import calls. GPU PIDs 17004, 18278 and 18401 imported on DRM device
226:128 (renderD128), with AR24 `0x34325241`, modifier
`0x0200000000401b03`, one plane, zero offset, and SCANOUT flags `0x1`.
The 640×360 buffers had stride 2560; the 1024×682 buffer had stride 4096.
All three imports returned null with `errno=38`, immediately before context
loss and GPU exit 8704. Replacement PID 19451 reached the later VA fallback.

A second private probe allocated **ARGB with explicit modifiers**, removing
the earlier NV12 mismatch. On renderD129 it produced exactly the browser's
format, modifier, dimensions, stride and offset. Import on renderD129 passed;
import on renderD128 reproduced ENOSYS. Both nodes reported ARGB format support.
The reverse device pairing also failed for the GFX11 modifier. These results
establish that the captured descriptor layout is incompatible with its importing
GPU. They do not identify who exported the browser's buffer. A width×4 stride
is valid for the observed tiled layout at these dimensions; the probe directly
refutes an inference that this pitch must indicate a malformed descriptor.

An initial ARGB probe using implicit modifiers returned DRM_FORMAT_MOD_INVALID
and imported successfully on both devices. It did not reproduce the browser
failure and was retained alongside the explicit-modifier result.

Chromium's [GBMSupportX11](https://chromium.googlesource.com/chromium/src/+/152.0.7977.83/ui/gfx/linux/gbm_support_x11.cc)
obtains its importing device from DRI3 Open. Sophia constructs its render-device
provider and server pixmap allocator from the same native-scanout device source;
the provider resolves the matching PCI render node. Source review found no
selection split in those two providers. This does not establish provenance for
a buffer imported by the browser.

The next requested user run adds `--render-node-override=/dev/dri/renderD128`
to the same diagnostic launcher. This is a local alignment experiment for the
current topology, not a portable desktop default. It changes the media device
selection paths and tests whether their buffers then import on the existing
X11 GBM device. The subsequent result is recorded below.

Redacted diagnostics, probe sources, fixtures and SHA-256 manifest are retained
under `~/.local/state/sophia/development-evidence/t068-gbm-import-20260907/attempt-001/`.
The source browser log was `/tmp/sophia-brave-gbm.KpbC7NfW/brave-VYYhYhM2.log`.

## Device-aligned run

The user relaunched with `--render-node-override=/dev/dri/renderD128`. The new
log, `/tmp/sophia-brave-gbm.KpbC7NfW/brave-wSKBw9l4.log`, captured 64 successful
imports and no failed imports or GPU exits. The interposer caps successful
records at 64 but continues recording every failure. Imported ARGB buffers now
use GFX11 modifier `0x0200000028a01f04` on node 226:128. GPU PID 4991 retained
seven DRM descriptors, all on renderD128; its command line carries the override
and does not carry the previous `--use-gl=disabled` fallback.

The failing import, exact-format reproduction and successful alignment run
support a browser media-device mismatch as the cause of these GPU crashes.
The importer uses the X server's advertised device; the media override produces
buffers whose layout that device accepts. The exact browser allocation call
that chose the other device was not instrumented, so this does not attribute
the exporter to a particular decoder or image-processing function.

The user reports that border flicker stopped after this relaunch, before any
Sophia repair was installed. Preserve that physical result separately from the
[repaint focus defect](h0vxis10-brave-gpu-watchdog-repeats-during-live-use.md).
It is not evidence that a pending code change has been physically accepted.

The successful comparison and manifest are retained in
`~/.local/state/sophia/development-evidence/t068-gbm-import-20260907/attempt-002/`.
This establishes a working launch override for the current machine. Ordinary
future launches still need that argument; no persistent launcher change was
made. A portable repair must choose a device matching the current X display or
negotiate an importable cross-device layout, rather than assume renderD128
always names the intended GPU.

## White video after device alignment

The user then reported white YouTube video. The aligned run still has no GBM
import failures or GPU restarts, but contains 3178 matching error chains:
`native_pixmap_egl_x11_binding.cc:98` reports rejected DRI3 PixmapFromBuffer;
`ozone_image_backing.cc:320` cannot create a GL representation; and
`shared_image_manager.cc:269` cannot produce a Skia image from that backing.
These failures were present in the aligned run before the white-video report;
the earlier import-success count did not establish complete image rendering.

Chromium selected its X11 pixmap fallback because direct EGL DMA-BUF import
was unavailable on that EGL display. The fallback supports BGRA8888, uses
plane zero and forwards its stored byte size. The matching VA wrapper at lines
2829–2832 deliberately stores zero as the plane size to prevent CPU mapping.
Sophia's pure DRI3 validation rejects stride×height greater than the declared
size. The browser wire size itself has not been captured; this chain is traced
from the matching source.

A private live probe sent the same real tiled ARGB buffer through DRI3 with
three declared sizes. Its kernel-reported length was 1572864 bytes, geometry
640×360 and stride 3072. Size zero produced core BadWindow (3), major 137,
minor 2. Size 1105920 (stride×height) and size 1572864 both succeeded. The probe
created only its own unmapped pixmaps and changed no application input or
window state. This establishes the zero-size compatibility failure independently
of the browser's graphics stack.

The log's `Glx::BadPixmapError` label does not identify a GLX request: Sophia's
GLX error base is zero, so Chromium labels core error 3 as GLX error index 3.
The DRI3 request identity and actual core error code were retained by the probe.
Also, an implicit DRM modifier does not mean linear pixels. The driver may
recover tiling from the FD; DRI3 PixmapFromBuffer permits nonlinear buffers.
The local X server reference forwards the FD with an implicit modifier and does
not use the request's size field.

The candidate handles only a zero declared size at the frontend FD boundary,
before pure dispatch, using the kernel-reported length. It preserves explicit
nonzero sizes and the existing descriptor, stride, dimension and namespace
checks. Failed or unusable size queries remain refusals. This avoids admitting
a pixmap first and discovering its bounds later, and does not map pixels or
seek a client-shared file description. Renderer-private import ownership stays
unchanged.

The real-socket regression passes all five cases: zero declared size with a
sufficient backing, short backing and empty backing; an explicitly short
nonzero declaration; and a valid nonzero declaration. Successful imports must
answer GetGeometry with 640×360 at depth 32. Refusals must return an X error
without a geometry reply. Every case preserves the shared file offset of 17.
Mutations that remove normalization, overwrite nonzero sizes, substitute an
unmeasured size, or seek the descriptor each fail the relevant assertion.
The X11 wire suite passes all 330 tests.

The combined candidate, based on `a44d1e163ceb`, passes `cargo xtask check`,
including workspace tests, clippy, archive verification and host buffer-age
equivalence. The gate also includes the separate repaint-focus repair recorded
in the [border investigation](h0vxis10-brave-gpu-watchdog-repeats-during-live-use.md).
Socket admission does not prove that Chromium subsequently displays the
imported image.

Evidence is retained under
`~/.local/state/sophia/development-evidence/t068-gbm-import-20260907/attempt-003/`.

## Playback observation on the installed repair

The user reported video playing in session
`00000001788830790138-53ca7f8e-e19c-4b19-8ede-698643da2576`, whose manifest
identifies commit `559b3907e5e2290ad1c47deb0f6a77567e3b1db1`.
However, the subsequent process inspection found GPU process 30652 running
with `--use-gl=disabled`. Browser process 26216 had no diagnostic logging or
render-node override flags, and stderr pointed to `/dev/null`. No new diagnostic
wrapper log existed. Playback is therefore confirmed, but accelerated playback
through the repaired import path is not. This observation does not establish
why that process uses disabled GL. A fresh diagnostic launch with the previously
successful device alignment is still needed to accept the GPU path.

## Accelerated playback still fails at EGL binding

The user restarted with the render-node override and again reported white
video. In `brave-TVVaImZk.log`, GPU process 2368 records 64 successful GBM
imports on renderD128 with the GFX11 modifier, no GBM failures and no GPU
exits. The old PixmapFromBuffer rejection is absent. Instead, 1483 failures
occur at `native_pixmap_egl_x11_binding.cc:205`, followed by the same GL
representation and shared-image errors. DRI3 admission now succeeds; the
remaining failure is initialization of the EGL pixmap binding.

That initializer either fails to select an RGBA8 pixmap config with texture
binding or fails to create its pixmap surface. Its single diagnostic does not
distinguish the two. Sophia offers GLX drawable mask `0x5` (windows and
pbuffers), no GLX pixmap constructors and no texture-from-pixmap extension.
The matching ANGLE revision `7df613367a1d4ca9aea9ece344d4580d32d132a9`
derives its GLX-backed EGL pixmap and binding support from those capabilities.
Brave maps Mesa GLX libraries and no external EGL library, consistent with
that backend; the browser's EGL vendor/backend was not directly queried.

A private connection to the same display confirms three GLX configurations,
all with drawable mask `0x5`. Separately, system Mesa EGL initializes there,
advertises DMA-BUF import and modifier import, and supplies a matching RGBA
pixmap config. These are different client implementations; the Mesa query is
not a query of Brave's EGL display.

The next controlled candidate adds `--use-angle=gl-egl` to the existing
render-node override. Matching Chromium source selects ANGLE's EGL device
backend for this flag; ANGLE's EGL backend forwards native DMA-BUF import
support. This could bypass the GLX pixmap fallback. It remains untested in
Brave and does not justify advertising unimplemented GLX capabilities.

## Reference code for the remaining interop work

Inspected local yserver commit `a1e33aa8176c65b418c1b6bef4c11f4598d5c2cc`.
Its current implementation includes GLX pixmap resource tracking,
texture-from-pixmap configuration attributes, capability-gated extension
advertisement, and modifier-aware DRI3 export. Useful references are
`crates/yserver-core/src/core_loop/process_request.rs` (GLX dispatch/configs),
`crates/yserver/src/kms/vk/target.rs` (export allocation), and
`crates/yserver/src/kms/vk/dri3.rs` (export metadata and sync-file operations).
The original design notes differ from current code; they are not proof of
implemented behavior.

`crates/yserver/tests/glx_tfp_export.rs` has concrete red-to-green pixel
liveness assertions and a backend lifetime test that retains an export across
FreePixmap and removes its registry entry after GLX release. Those tests are
ignored by default, require Vulkan, and can skip on unavailable setup. They
were read, not run. They do not prove the complete GLX wire path or concurrent
GL/Vulkan fence ordering. Adapt their assertions into Sophia's own tests rather
than treating their existence as acceptance evidence.

Also inspected local wayland-rs commit
`0813584ea50379bd22e95dc1e0b50f02a4b36ca2`. This is Smithay's protocol-library
repository, not the Smithay compositor toolkit. `wayland-egl` wraps client-side
`wl_egl_window` ownership; it supplies no reusable DMA-BUF importer for this
X11 failure. `wayland-backend/src/rs/socket.rs` is useful for owned received
FDs, close-on-exec reception and separate byte/FD accounting on partial sends.
`wayland-protocols/src/wp.rs` documents acquire/release timeline semantics;
the referenced protocol XML submodule is empty in this checkout, so its full
contracts were not inspected here.

Both repositories use MIT licenses; copied substantial code must retain the
respective notices. GLX/XID validation belongs in Sophia's X authority, buffer
storage and synchronization in its renderer/Engine, and neither in the WM.
No reference source was copied into production. Test native EGL selection
first; use yserver as an implementation reference if Sophia needs to support
the GLX pixmap fallback without browser-specific flags.

## Acceptance and architectural boundary

Capture a failing browser import, then reproduce its descriptor/device
combination in a bounded probe. Repair the fault where that evidence places
it: client allocation/import, frontend device advertisement, or Engine import.
The WM has no role in choosing VA devices or repairing buffer formats. Keep
namespace and sandbox boundaries intact.

Acceptance requires a fresh GPU-enabled Brave process, successful imports
through the formerly failing interaction, and no crash-driven software
fallback. Preserve the exact configuration and evidence. The local override
has passed the captured interaction. The subsequent GLX candidate passes its
private pixel gate; normal launch without manual graphics-selection flags and
broader daily-use acceptance remain open. An automatically derived flag from the
launch adapter below is part of the intended normal launch configuration.

## Client launch alignment, 2026-09-08

The accepted implementation boundary is a CLI client adapter, not a browser fork
or Chromium policy in Engine. Matching Chromium sources show independent device
selection: X11 GBM opens the DRI3 device, while VA initialization receives the
preliminary GPU identity before later graphics-context identification. Both
`render-node-override` and `hardware-video-device-path` propagate to the GPU
process. The earlier captured descriptor/device failure and successful override
establish device alignment as the concrete intervention; they do not identify
the exporter in every subsequent uninstrumented crash.

`sophia client-launch --adapter=chromium` derives the override through an
authenticated local DRI3 Open(root,0), with a one-second launch deadline. Kernel
device numbers, sysfs physical-device identity, and reopened device-node identity
replace enumeration guesses. Explicit device switches are preserved and
validated; conflicts refuse rather than silently rewriting the user's choice.
The Go launcher and browser remain unchanged. See the
[client launch configuration](../../configuration.md#client-launch-adapters)
for direct and separator-consuming wrapper recipes.

The registered Brave recipe is prepared separately from the active user config:
the installed binary must support this command before the recipe takes effect.
Physical acceptance still requires a fresh browser launched through the normal
binding, an accelerated GPU process, video playback with no repeated GBM import
failure or GPU restart, and no white frames. Playing video after crash-driven
software fallback does not meet that gate.

Validation on the candidate based on `c5ce11c0f10798c4290baf3f1eb23b10bb81fc82`:

- `cargo xtask check`: 2,745 tests passed, zero compiler/Clippy warnings,
  all 20 archived proofs reverified, and buffer-age pixel equivalence proved.
- Nine argument tests, eight device-identity tests and five private socket
  tests cover separator handling, explicit overrides, two same-vendor physical
  identities, node renumbering, inode replacement, unrelated disappearing nodes,
  MIT-cookie selection, malformed DRI3 replies and bounded failure in normal and
  check-only modes.
- Four private hardware tests passed with each AMD render node advertised in
  turn. They verify the actual helper reports the server's chosen device,
  preserves matching explicit selections, refuses the other GPU, and execs a
  fixture with the same PID and exact arguments without discovery FD leaks.
  These tests do not launch Brave or interact with the live session.
- The prepared core configuration validates with digest
  `b74c43bad36cb75ce7a173f499f05f8e9921174116bcdcece99fe266d57c119c`.
  It replaces registered application 5's recipe and adds that registration to
  the installed catalog. The active user configuration remains unchanged.

The source snapshot, binary digest, device identities, final gate and private
hardware logs, and prepared configuration/patch are archived under
`~/.local/state/sophia/development-evidence/t068-client-launch-20260908/attempt-001/`.
`manifest.json` binds the uncommitted candidate to exact source bytes. The helper
was not installed and no browser was launched or restarted during this work.

## Generic-device requirement and withdrawn launch integration, 2026-09-08

After installing `c7d355d2`, the normal Super+B registration still bypassed the
explicit CLI helper. A read-only process check found Brave's GPU process running
with `--use-gl=disabled` and no DRM descriptors despite playing video. A manual
helper recipe was subsequently activated for the next login, with the prior core
configuration backed up; that did not change the already-running session.

An uncommitted automatic-prefix implementation covered all three managed launch
paths and passed `cargo xtask check` (2,768 tests, zero compiler/Clippy warnings,
20 archived proofs and buffer-age pixel equivalence). Six private hardware tests
also passed with each render node advertised, including a real Go parser/exec
fixture. It depended on a known-executable table. niltempus explicitly rejected that
maintenance model and required generic behavior for arbitrary clients. The
entire automatic integration was withdrawn before commit or installation; its
source and validation logs were saved under
`/tmp/sophia-t068-automatic/` as temporary investigation evidence. Passing tests
did not satisfy the revised architectural requirement.

`t069` is now the top-priority [universal device-negotiation plan](../plans/6tewvlbh-universal-device-negotiation-across-sophia-clients.md).
It prohibits application-specific Engine adapters and supported-app lists
anywhere. The existing explicit CLI helper is temporary compatibility tooling,
not the universal solution or its acceptance evidence.

Independent reviews of Chromium 152.0.7977.83 and ANGLE
`7df613367a1d4ca9aea9ece344d4580d32d132a9` establish why server negotiation alone
cannot repair the captured client-internal import. ANGLE
[enumerates PCI devices](https://chromium.googlesource.com/angle/angle/+/7df613367a1d4ca9aea9ece344d4580d32d132a9/src/gpu_info_util/SystemInfo_libpci.cpp#124)
and its [active-device heuristic](https://chromium.googlesource.com/angle/angle/+/7df613367a1d4ca9aea9ece344d4580d32d132a9/src/gpu_info_util/SystemInfo.cpp#265)
selects the first device when both are AMD. Chromium passes that preliminary
identity into VA initialization before collecting GL context identity. DRI3
controls its separate GBM import device; neither DRI3 modifiers nor a server-side
copy influences the earlier independent allocation. This establishes the
selection mechanism and control boundary, not the exact producer of every
uninstrumented failed buffer. Restricting device access alone also does not prove
successful accelerated allocation on the permitted device.

## Controlled no-override acceptance on 2026-09-09

Installed candidate `76ed2fddf31a` still fails this gate. niltempus authorized opening
and closing applications for testing. Three isolated fresh-profile runs used the
same existing Brave binary, a local 1280x720/30 H.264 loop, and DevTools media/GPU
observations. No browser source or normal launch configuration was changed.
All test browsers closed normally; the original browser remained running.

| Run | 30-second result | GBM import failures | GPU exits |
| --- | --- | ---: | ---: |
| No override, uninstrumented | Two video-frame callbacks; time fixed at 0.143 s | 1 | 1 (8704) |
| Existing renderD128 override | 903 callbacks; zero reported dropped/corrupted frames | 0 | 0 |
| No override, import tracing | Two callbacks; time fixed at 0.130 s | 1 | 1 (8704) |

Both configurations reported `VaapiVideoDecoder` and a platform decoder. The
control's screenshots contain changing test-pattern pixels. These callbacks and
screenshots are browser-side evidence, not a count of physical flip retirements.
The initial probe serialized VideoPlaybackQuality as an empty object; the latter
two read its individual fields. No quality assertion relies on that first object.

Before video, GPUInfo listed Raphael `[1002:164e]` first and Navi31 `[1002:744c]`
second, while `glRenderer` identified ANGLE/OpenGL on RX7900 GRE/Navi31. The
traced initial GPU process held FDs on both render nodes. The failed import ran
on DRM `226:128` with AR24, size 1280x720, one plane, modifier
`0x0200000000401b03`, flags 1, and `errno=38` (ENOSYS). That modifier describes
GFX10_RBPLUS tiling; this pins the importer and descriptor, not the allocation
call or producer identity. The VA display hook did not fire. The override affects
multiple allocation/selection consumers and does not isolate VA alone.

The exact [GPU initialization sequence](https://chromium.googlesource.com/chromium/src/+/79460ebecaa5625e57a5fb679a735659e73dc687/gpu/ipc/service/gpu_init.cc)
collects basic GPUInfo, initializes GL, passes the earlier snapshot into VA
pre-sandbox initialization, and later collects context-derived GPUInfo.
[VA selection](https://chromium.googlesource.com/chromium/src/+/79460ebecaa5625e57a5fb679a735659e73dc687/media/gpu/vaapi/vaapi_wrapper.cc)
uses that snapshot's PCI IDs; the
[X11 GBM importer](https://chromium.googlesource.com/chromium/src/+/79460ebecaa5625e57a5fb679a735659e73dc687/ui/gfx/linux/gbm_support_x11.cc)
uses DRI3 Open. The failing buffer never reaches Sophia's import/copy path.
Earlier X11 initialization may already have completed; the boundary applies to
the failing buffer, not to all browser/server communication.

A robust upstream repair would acquire one explicit native-pixmap device
capability before sandbox entry and share it with VA and media allocation for
the GPU-process generation. Moving context collection earlier alone is
insufficient: [IdentifyActiveGPU](https://chromium.googlesource.com/chromium/src/+/79460ebecaa5625e57a5fb679a735659e73dc687/gpu/config/gpu_info_collector.cc)
still matches vendor strings. Identical PCI IDs, render offload, FD ownership,
lazy initialization, device loss and restart all need coverage. Selecting the
same device also does not replace layout validation.

Evidence, the exact private probe and hashes, screenshots, and an unsent upstream
report are retained under `.artifacts/t069-no-override/`; `summary.json` maps all
three runs. The browser report contains no profile data or browsing history.
XLibre `56be9f43` and yserver `a1e33aa8` both ignore SetDRMDeviceInUse; that
reply-less client hint cannot provide a missing selection to VA. The
[t069 exit](../plans/6tewvlbh-universal-device-negotiation-across-sophia-clients.md)
therefore remains unmet by this candidate, despite its passing server tests.

## Native Wayland and X11 comparison on 2026-09-09

Two further fresh-profile runs used the same browser, local video and 30-second
probe. Weston 15.0.1 ran its GL renderer on renderD128/Navi31 through its nested
X11 backend. One browser connected to native Wayland; the other connected to
Xwayland 24.1.13 hosted by Weston. Sophia remained the outer display server.
This compares client-facing protocols without claiming a standalone physical
Wayland session or a native XLibre run.

| Client-facing path | Browser argv override | GPU-process override | Callbacks | GBM failures / GPU exits |
| --- | --- | --- | ---: | ---: |
| Sophia X11, earlier uninstrumented run | None | None | 2, then stalled | 1 / 1 |
| Xwayland X11 | None | None | 1, then decode disconnected | 1 / 1 |
| Native Wayland | None | renderD128, added internally | 895 | 0 / 0 |

The native Wayland run reported `VaapiVideoDecoder`, 909 total video frames,
seven dropped frames and zero corrupted frames. Both comparison browsers and
nested compositors exited normally; the user's original browser was untouched.
The additional summaries and exact probe hashes are in
`.artifacts/t069-no-override/cross-server-summary.json` and
`cross-server-probe-sha256.json`. As above, these are browser playback observations,
not physical scanout measurements.

The internally added override has an existing protocol source.
[Wayland DMA-BUF main-device handling](https://chromium.googlesource.com/chromium/src/+/79460ebecaa5625e57a5fb679a735659e73dc687/ui/ozone/platform/wayland/host/wayland_zwp_linux_dmabuf.cc)
resolves the compositor's device to a render node.
[SetRenderNodePath](https://chromium.googlesource.com/chromium/src/+/79460ebecaa5625e57a5fb679a735659e73dc687/ui/ozone/platform/wayland/host/wayland_connection.cc)
validates that node through GBM and appends the selection to Chromium's in-memory
command line when no explicit selection exists. The
[GPU-process launcher](https://chromium.googlesource.com/chromium/src/+/79460ebecaa5625e57a5fb679a735659e73dc687/content/browser/gpu/gpu_process_host.cc)
copies that switch to its child. This explains an unchanged browser argv and a
selected GPU child without a user recipe. The X11 GBM importer consumes DRI3
Open without the corresponding media-selection bridge.

The paired failure is therefore not unique to Sophia's X11 implementation.
The successful Wayland path also prevents a broader claim that Chromium cannot
negotiate this hardware: its native Wayland client already participates in the
compositor's selection. Extending that participation to X11 is a client-side
boundary; adding a server advertisement alone cannot make an existing media
allocator consume it. Native XLibre remains untested, and neither the nested
comparison nor source similarity establishes its runtime outcome. XLibre and
yserver remain the X11 references; Xwayland is not a Sophia dependency or a
proposed implementation strategy.

## Installed 9611131e comparison

The new installed owner was rechecked with the same Brave 1.94.121 binary and
local H.264 loop, two fresh profiles, and no interposer. Session
`00000001789000283295-eca6d35b-d625-47a8-ae46-f8fa1a3f5667` runs
`9611131e7bb268156e8fe1da89c0fd0f518ff7fb`, with the validated release's exact
binary hash.

The no-override run remained at one video-frame callback and media time zero
through all six five-second samples. Its original GPU process held descriptors
on both render nodes, logged one `gbm_bo_import` failure, then exited with 8704.
The paired `renderD128` control reached 904 callbacks, with 908 total frames,
zero dropped/corrupted frames, no GBM import error and no GPU exit. Both reported
`VaapiVideoDecoder` and a platform decoder. Both probe browsers closed normally;
the test used only its own local page and profiles.

The new server's normal buffer/preference path separately passed 720 exact
Copy/Idle pairs across both outputs, including two-plane compressed XR24/AR24
buffers. That server result does not change this pre-submission browser failure.
No descriptor interception was used in the new browser run, so it confirms the
failure signature and control outcome, not a new producer-device attribution.

Exact browser/probe/video identities, six-sample observations, isolated-page
screenshots, logs and exit records are retained under
`.artifacts/t070-live-9611131e/browser/`. These are browser playback observations,
not physical pixel acceptance. The normal no-override exit remains unmet on
`9611131e`.

## Normal launcher on installed 17134504

On 2026-09-10 the installed paired release was verified as Sophia
`171345049bf620a40b24c48d738329b1f63decaf` and Hagia
`3459a85d5dd7a1943efcf526fa5d4ec297d246cd`. With no existing Brave process,
the test invoked the ordinary `brave-origin` launcher with no arguments, then
opened a local H.264 page through a second ordinary URL invocation. It used
the normal browser profile and added no GPU/device flags, interposer, debug
endpoint, autoplay override or profile override.

The page reported AMD RX 7900 GRE/Navi31 through ANGLE/OpenGL, then stopped at
three video callbacks and media time 0.139265 seconds with
`PIPELINE_ERROR_DECODE`. All six five-second samples retained that state.
niltempus subsequently opened YouTube and reported working playback. The captured
browser log recorded three GBM import failures and three GPU exits with 8704;
the resulting GPU process carried `--use-gl=disabled`, held no DRM descriptors,
and mapped no Radeon driver. Working playback therefore does not pass the
hardware-acceleration gate: this run reached software fallback.

Evidence and the exact launch script remain in
`.artifacts/t069-normal-17134504/run-20260910-183449/`. The initial process
sampler incorrectly assumed NUL-separated arguments after Brave rewrote its
process title; its empty GPU arrays are unusable. The final process snapshot
matches delimited flags in that title and records the fallback directly.
No new descriptor interception was performed, so the run confirms the failure
signature without adding a producer-device attribution. The browser remains
open for niltempus's use. Task t069's normal-launch exit remains unmet.

## Producer attribution and stock Chromium on 2026-09-10

The installed Sophia/Hagia pair remained `171345049bf6` / `3459a85d5dd7`.
niltempus authorized independent browser tests and installed Void's stock Chromium
`151.0.7922.108_1`. Brave remained `1.94.121`, Chromium `152.0.7977.83`.
Both used the same X11 session, system Mesa and local 1280×720/30 H.264 video.
Each test owned a temporary profile, a debugging endpoint and its own browser
process. No sandbox-disabling option or production configuration change was
used. Tests closed only their own browser instances.

### Allocation identity closes the missing link

Chromium resolves VA entry points with `dlsym` against explicit library handles.
That bypassed the earlier direct `vaGetDisplayDRM` preload. The new diagnostic
interposes those lookups, forwards their actual resolved functions and records
VA display, surface creation, export and GBM import. FD correlation uses
`fstat` device/inode identity, not FD numbers or an inferred modifier origin.
It does not map pixels. A standalone allocation/import comparison produced
identical results with and without the tracer.

In the passive Brave trace, the failed allocation was a 1280×720 RGB32 VA
surface on the Raphael render device. Its exported DMA-BUF and the later GBM
import had the same device/inode identity and 3,932,160-byte allocation size.
The destination GBM device was Navi31. The import descriptor carried AR24,
stride 5120, offset 0 and modifier `0x0200000000401b03`; it failed with
`ENOSYS`, followed by GPU-process exit 8704. Stock Chromium's trace independently
correlated its failure to the same allocation/import device split.

This establishes producer attribution for these failures. It supersedes the
earlier limit that only the modifier and importer were known. The raw VA export
also records its layer format; Chromium's subsequent VPP/import representation
must not be inferred solely from that initial layer name.

A separate diagnostic retained a duplicate of the actual VA render descriptor
before sandbox entry. After the original Brave import failed, it tried the
**same incoming DMA-BUF descriptor and FD** on both devices, destroyed any
temporary imported objects and returned the original failure unchanged:

| Import device | Flags 0 | SCANOUT flag |
| --- | --- | --- |
| Navi31, original destination | ENOSYS | ENOSYS |
| Raphael, measured VA allocation device | Success | Success |

The test changed neither the modifier nor the image format between attempts.
This is stronger than the older standalone NV12 comparison: the tested object
is the browser's actual failed buffer. Removing SCANOUT cannot repair this
failure. The observation probe performs extra driver calls and retains one
extra render FD until process exit; it is separate from the passive trace and
the uninstrumented playback controls.

### Stock browser and alignment controls

All runs selected `VaapiVideoDecoder` and reported a platform decoder. The
stock `/usr/bin/chromium` launcher prepends its packaged GPU-rasterization
option; that wrapper and its environment were identical across its final
no-override and aligned controls. An earlier direct-binary Chromium run also
failed, excluding dependence on that wrapper option.

| Browser run, about 20 seconds | Frame callbacks | GBM failures / GPU exits |
| --- | ---: | ---: |
| Stock Chromium, direct binary, no override | 3, then stalled | 1 / 1 |
| Stock Chromium launcher, no tracer or override | 1, then stalled | 1 / 1 |
| Stock Chromium launcher, traced, no override | 1, then stalled | 1 / 1 |
| Brave, passive trace, no override | 1, then stalled | 1 / 1 |
| Brave, exact-buffer comparison, no override | 2, then stalled | 1 / 1 |
| Stock Chromium launcher, aligned device | 599 | 0 / 0 |
| Brave, aligned device | 602 | 0 / 0 |

The aligned controls used `--render-node-override=/dev/dri/renderD128` and no
tracer. Chromium reported 605 total video frames, two dropped and none corrupt;
Brave reported 606 total, none dropped or corrupt. These are browser-side
playback observations, not measurements of physical scanout. Different browser
versions prevent a strict Brave-versus-Chromium patch comparison, but both
independently demonstrate the same client-internal failure mechanism.

Evidence, source, hashes, screenshots, Media/SystemInfo observations, exact
per-run launch records and machine-checked FD correlations remain under
`.artifacts/t069-producer-trace/`. `trace-original.c` / `trace.so` own the
passive and destination-retry runs; `trace-source-check.c` /
`trace-source-check.so` own the exact-buffer comparison. The source-check
probe never substitutes a successful import into the browser. Stock Chromium
was initially downloaded and checksum-verified for isolated extraction; the
browser runs above used niltempus's subsequently installed package.

### Repair boundary and contrary evidence

The failing call is inside the client, before this video buffer is submitted
to Sophia. The measured split is between the client's VA allocation device and
its DRI3-backed GBM importer. A server copy path cannot intercept that call.
This identifies the failure in Chromium's X11 device integration, reproduced
in Brave, rather than an inherent inability of X11 to accelerate video. It does
not certify unrelated Sophia paths or constitute a native XLibre runtime test.

The robust client repair is to acquire the X11 native-pixmap device capability
before VA initialization and share its owned identity with the VA and media
allocation paths for the GPU-process generation. Explicit offload requires a
validated compatible transfer before import; it cannot assume that an arbitrary
tiled buffer crosses devices. Keep modifier/import validation, respect explicit
device policy, handle unavailable acceleration without repeated GPU-process
crashes, and rebuild the capability after GPU restart. Vendor strings or PCI
enumeration order are insufficient identities. No application-name handling or
Chromium-specific code belongs in Sophia Engine. No browser patch is included.

Claude's online review found a related [Brave report](https://github.com/brave/brave-browser/issues/53738)
with the same errors on AMD/X11; that reporter says Chrome works. It remains a
symptom comparison, not evidence against the controlled local Chromium result.
The [2023 Chromium allocation workaround](https://chromium.googlesource.com/chromium/src/+/20f14755d4b4a5eddd05fb17fc61de53272cd45e)
replaced an import retry with allocation-time modifier checks. Its SCANOUT
concern is real, but the exact-buffer test above excludes flag removal as the
repair for this incident. No matching XLibre runtime report was established.

Chromium's [GPU identification code](https://chromium.googlesource.com/chromium/src/+/79460ebecaa5625e57a5fb679a735659e73dc687/gpu/config/gpu_info_collector.cc)
explicitly acknowledges that its vendor-based match can select the wrong GPU
when two devices share a vendor. That limitation is not AMD-specific. The
measured import incompatibility here is specific to the tested AMD pair and
buffer; no Intel or NVIDIA reproduction was performed. The source limitation
does not establish an exact public issue for this X11 VA-to-GBM split, nor does
the trace establish which identification heuristic selected the VA device.

Root cause is now measured; the normal, unmodified accelerated-video acceptance
gate is still unmet. The temporary device override is a diagnostic control,
not completion of `t068` or `t069`.

## Upstream submission preparation on 2026-09-10

The recovered Chromium issue draft is retained at
`.artifacts/t069-upstream-submission/report.md`, with diagnostic sources and
normalized observations in
`.artifacts/t069-upstream-submission/chromium-x11-va-gbm-evidence.zip`.
Its title is "X11: VA-API and DRI3 select different GPUs, causing GBM import
failure and GPU-process restart". The report includes stock Chromium controls,
the exact-buffer comparison, source pointers and the limits above.

The 13,317-byte archive has SHA-256
`5f931de695c64e213f842114c2d7b22f451164b7528d36b098b85e6741d47035`.
All six payload checksums were verified against its `SHA256SUMS`, and the
archived report matches the submission draft. The bundle contains no browser
profile or authentication material.

The [Chromium filing page](https://issues.chromium.org/new) was reopened in the
submission browser. Google refused sign-in in the separate Chromium and Brave
submission profiles. niltempus was already signed in through his normal Brave
Origin profile and chose to file the issue manually.
`open-brave.mjs` and `brave-connection.json` in
`.artifacts/t069-upstream-submission/` retain its launcher and connection.
The existing `cdp.mjs` accepts that connection through
`SOPHIA_SUBMISSION_CONNECTION`. These are preparation artifacts, not a pending
authentication step. No issue number or successful submission was verified.
Filing the upstream issue does not satisfy the normal, unmodified
accelerated-video gate for `t068` or `t069`.

## Upstream patch proposal at 92802374, 2026-09-25

This section re-reads current Chromium. The normal, no-override exit is still
unmet. Chromium `main` was resolved to
`92802374ec246ea2c764a47c0f09d7893c56c029`, and the cited files were retained
with SHA-256 values under `.artifacts/t069-upstream-proposal/` in the t069
worktree.

The selection is unchanged in substance:
- [`VADisplayStateSingleton::PreSandboxInitialization`](https://chromium.googlesource.com/chromium/src/+/92802374ec246ea2c764a47c0f09d7893c56c029/media/gpu/vaapi/vaapi_wrapper.cc#1618)
  in `media/gpu/vaapi/vaapi_wrapper.cc` (lines 1618-1682) honours the two
  device switches.
- Without them, it keeps the `drmGetDevices2` entry whose PCI identity matches
  `GPUInfo`'s [active GPU](https://chromium.googlesource.com/chromium/src/+/92802374ec246ea2c764a47c0f09d7893c56c029/media/gpu/vaapi/vaapi_wrapper.cc#1679).
- [`ui/gfx/linux/gbm_support_x11.cc`](https://chromium.googlesource.com/chromium/src/+/92802374ec246ea2c764a47c0f09d7893c56c029/ui/gfx/linux/gbm_support_x11.cc#33) is
  byte-identical to the measured `79460ebe` copy. Its
  [`dri3.Open`](https://chromium.googlesource.com/chromium/src/+/92802374ec246ea2c764a47c0f09d7893c56c029/ui/gfx/linux/gbm_support_x11.cc#53) descriptor stays inside
  its `GbmDevice`.
- Wayland's [`WaylandConnection::SetRenderNodePath`](https://chromium.googlesource.com/chromium/src/+/92802374ec246ea2c764a47c0f09d7893c56c029/ui/ozone/platform/wayland/host/wayland_connection.cc#820)
  (lines 820-833) still appends `--render-node-override` from the
  compositor's validated device.
- `GpuProcessHost` still [copies that switch](https://chromium.googlesource.com/chromium/src/+/92802374ec246ea2c764a47c0f09d7893c56c029/content/browser/gpu/gpu_process_host.cc#322)
  to the GPU process.

The draft proposal (`proposal.md`, not filed) mirrors the Wayland behaviour in
the X11 platform's browser-side
[`InitializeUI`](https://chromium.googlesource.com/chromium/src/+/92802374ec246ea2c764a47c0f09d7893c56c029/ui/ozone/platform/x11/ozone_platform_x11.cc#247):
1. DRI3 Open on the root.
2. Name the render node with `drmGetRenderDeviceNameFromFd`.
3. Validate it with `gbm_create_device`.
4. Append the switch only when neither device switch was given.

It states its limits:
- The path is fixed at browser start.
- It carries a path rather than an identity.
- The server's device remains advisory, and explicit offload keeps
  precedence.
- The vendor-string active-GPU limitation stays separate.

No browser patch was built, and no issue, comment or message was sent.

One evidence mismatch remains open. On 2026-09-09 renderD128 was the Navi31
discrete GPU: the Weston renderer and the GBM import destination. The
2026-09-20 note above reports GPU0 `0x164e` (Raphael) as ACTIVE under the same
renderD128 override. Neither date retained a node-to-PCI map, and the kernel
changed between them. The probe that would settle it was not authorized. The
mismatch is another reason a node-number override is not an identity-stable
selection, and not an acceptance of `t069`.

## Connections

The [default-visual investigation](g930kzbe-default-x-visual-excluded-rgba-pixmap-configurations.md)
records the white-video reproduction on `7dd74c5c5183` and a GLX call trace
identifying the display's alpha-zero configuration restriction. After private
pixel tests passed, niltempus confirmed normal video in a fresh session on
`b43d23d0bb15` on 2026-09-09. This accepts the white-video repair. The session
still used the explicit device override, and discarded browser stderr prevents
an error-history check; the wider accelerated-video gate remains open.

The [implicit-modifier investigation](n5i1x7iv-implicit-dma-buf-exports-used-the-wrong-drm-modifier-sentinel.md)
records the white-video report on installed `009498fb0aae`, the matched-launch
EGL initialization errors, and a separately reproduced export-encoding defect.
Its private EGL pixel repair does not by itself establish Brave acceptance.

`t068` in [todo.md](../../../todo.md) owns this work. The earlier
[delayed-publication investigation](vwo9wmie-window-switches-reveal-delayed-visual-updates.md)
concerns Sophia frame scheduling. A browser GPU crash is a separate failure;
neither diagnosis establishes the other.

The subsequent [GLX pixmap export investigation](uffn76nu-glx-pixmap-exports-need-coherent-backing-and-reply-ordering.md)
records the configuration, pixel-publication and retained-storage repair.

## Accepted on the installed session, 2026-09-20

The remaining work named above -- the normal-launch repair, and accelerated
playback failing at EGL binding -- is superseded by what the live session now
shows. Read from `chrome://gpu` in the operator's running Brave Origin
1.95.104:

- **Video Decode: Hardware accelerated.** Canvas, Compositing, Rasterization,
  WebGL and WebGPU likewise. Video Encode stays software-only by blocklist,
  which is Chromium's Linux default and outside this note's subject.
- **GPU0 `0x164e`, Mesa 26.1.8, ACTIVE** -- the integrated device this note
  established the descriptor imports on -- rather than GPU1 `0x744c`, the
  discrete Navi 31 it failed on.

The device split this note spent its length identifying is resolved by
selecting the device rather than by changing the renderer:
`--render-node-override=/dev/dri/renderD128`, configured in
`~/.config/hagia/config.kdl` and `lom-workspaces.kdl`, with `sophia-cli`'s
`client_launch_socket` tests covering the injection. That is a durable
configuration, not a flag typed once.

And the symptom that opened the note is gone rather than quieter. It was four
GPU processes failing at `gbm_wrapper.cc:465` within minutes. Measured in
ordinary use: one gpu-process, up 20h39m32s against a browser up 20h39m33s --
started with it and never replaced -- and no `gbm_wrapper` or GPU-crash line
in any session log.

t068 is closed on this. The interop reference kept above is left standing: it
describes work that was never done, and the device selection is what made it
unnecessary rather than complete.
