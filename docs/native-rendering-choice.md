# Choosing CPU pixels or GPU buffers for native Sophia clients

This guide covers application renderer choice for native shell/image clients.
It adds no protocol capability: the negotiated profile and
[image channel contract](sophia-image-channel.md) still define admission and custody.

## Two pipelines; application and user choice

| Pipeline | Application supplies |
| --- | --- |
| CPU pixels | Pixels in CPU-accessible memory, offered through the owned ring |
| GPU buffer | A completed exportable GPU image, admitted through DMA-BUF |

The application chooses and may offer a user preference among its supported
renderers. Sophia enforces GPU permissions and exact capabilities, formats and
limits. A preference cannot grant GPU authority. This guide introduces no general
Sophia renderer-override API.

An application can deliberately offer CPU fallback when GPU admission fails, but
should make that policy explicit and report the selected mode. A refused GPU
request must not silently become a different benchmark or rendering path.

CPU describes client pixel production, not a GPU-free desktop: Session may still
use GPU composition. Both current paths retain Session's custody copy. DMA-BUF
does not promise direct scanout or end-to-end zero-copy presentation.

## Which should an application try?

| Workload | Starting point to measure |
| --- | --- |
| Narrow bar, modest text, simple small scenes | CPU; GPU submission/synchronization overhead may dominate |
| Pixels already decoded into CPU memory | CPU slice upload; avoid a GPU round trip solely to recover pixels |
| GPU permission unavailable | An explicitly supported CPU renderer |
| Large animated surfaces, expensive effects, content already on GPU | GPU, with its actual format/transfer path qualified |

These are hypotheses, not performance guarantees or size thresholds. A 1920×24
bar is 46,080 pixels, or 180 KiB at four bytes per pixel. CPU rendering may finish
it sooner; a complex scene at the same dimensions may favor GPU rendering.

Measure real scenes and sustained workloads. Compare client and Session CPU
µs/frame separately and combined, elapsed render-to-ready time including required
transfers, presentation cadence and missed deadlines. Latency comparisons need
matching endpoints. Measure GPU execution and energy separately if they matter.
CPU accounting is neither wall-clock latency nor GPU time or power: a sleeping
fence wait costs elapsed time with little CPU accounting. Finishing earlier need
not alter display time when both paths meet the same presentation deadline.

## Hybrid rendering fits these two pipelines

CPU/GPU cooperation inside a renderer does not require a third Sophia transport.
Current upstream Sparse Strips `vello_gpu` prepares paths and strips on CPU, then
rasterizes and composites on GPU; `vello_cpu` produces a CPU pixmap. See
[Vello architecture](https://github.com/linebender/vello/blob/main/ARCHITECTURE.md)
and [Vello GPU](https://github.com/linebender/vello/blob/main/vello_gpu/README.md).
These upstream descriptions are not the identity of Sophia's pinned renderer.

An app can also upload CPU-rendered or decoded content into a GPU scene. Its final
image still follows the CPU-pixel or GPU-buffer path. Account for those transfers
and synchronization. No automatic per-object CPU/GPU scheduler is implied.
The GPU lane under qualification uses the older compute-based Vello through
Lom, not the newer Sparse Strips GPU backend; changing it is a separate intervention.

## Available APIs versus planned adoption

Rust SDK `0706ea2c78cd16ba59ed209620354f30a2c78906` provides
`image_upload_ring` for existing slices and synchronous `image_upload_ring_fill`
for writing a slot through a callback. Its lower-level `RingSlotFill` borrows its
producer. Respect negotiated pixel format, logical byte length and release rules;
do not move that borrow to a worker while continuing to use the connection.

Lom `32e0bab31594f70c3d945547d7e81e2806b6fae8` still requires a GPU grant for
native `--serve`. Its `RenderImage::Cpu` is GPU-readback data, not CPU rasterization.
Vello CPU scene tests do not establish a production CPU worker.

The [CPU direct-fill design](notes/investigations/i9ljdlt4-how-can-lom-render-vello-cpu-directly-into-an-owned-ring-slot-across-its-worker-boundary.md)
adds an explicit CPU renderer and proposes worker-safe owned slot custody. Direct
fill is the intended default **when CPU is selected**, after qualification.
Slice upload remains for callers already holding pixels.

The Rust owned-loan API now has a signed local device-free implementation
(`5d895652`, parity documentation `99b05e34`). C `9adb3c5` independently proves
the existing pixels/offer sequence under documented caller ownership; C callers
must enforce the worker lifetime that Rust's loan protects. Neither is a verified
Sophia SDK pin update. Lom integration, consumer lifecycle and native qualification
remain open. Consult the linked design's source-gate receipts before adoption.

GPU DMA export remains subject to its experimental profile and exact format/device
admission. The current producer renders RGBA then blits into a LINEAR XR24 export
target. Rendering directly into that target first needs format/usage qualification;
a texture-view API alone does not establish interoperability. See the
[GPU qualification matrix](notes/investigations/nb3mgbwc-what-must-match-before-comparing-gpu-buffers-across-sophia-niri-and-xlibre.md).
Neither planned optimization is a measured gain or a default change yet.
