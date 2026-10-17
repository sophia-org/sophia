---
id: cerml352
date: 2026-10-17
kind: investigation
status: investigating
tags: [investigation, shell, lock, 9p, performance, architecture]
---
# What a fast shell and lock image path looks like beside 9P

## Question

The matched B2/B3 pilot recorded in the
[t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md)
left every shell cost gate red, and the
[descriptor custody review](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md)
proposes a descriptor channel beside the 9P wire as the answer for large
images. On 2026-10-17 niltempus asked four things in turn: whether that
proposal deviates from 9P, what the performant alternatives are, whether a
protocol for this already exists, and where the answers leave the
architecture. This note records the reading and the diagram that came out of
it so the reasoning survives the conversation. It decides nothing. The design
remains proposed, and the choices named at the end are the owner's.

## Evidence

The measurements are the pilot's. A Session pass costs about 20 microseconds
and 40 allocations, and a 4 MiB image on the file wire costs about 69 pixel
writes, each of which the server reads in its own turn because the connection
read budget is one message per turn for fairness. On both wires every pixel
byte crosses a Unix socket, which by mechanism means a kernel copy on send and
another on receive; the pilot did not attribute cost to those copies
separately. Both wires pay the per-pixel premultiplied-alpha check on arrival.
After the B3 window the client waits on about a quarter of its pixel writes
rather than on each one, while the server still spends one read turn per
write, so server passes and client waits are different counts. The bar's frame has three
pixel writes, so its cost is pass count and per-request allocation rather
than transport.

The reading covered the display stacks and the file-protocol precedents.

X11 moved from MIT-SHM, where the server copied out of a shared segment, to
DRI3, where a DMA-BUF descriptor crosses the socket once and the server
imports the memory the client rendered to; Present adds pacing and idle
notifications. Wayland passes both of its buffer kinds as descriptors. A
wl_shm pool is shared memory the compositor maps and then usually copies into
a GPU texture on its main thread; KDE measured those uploads blocking KWin in
power-save mode, Phosh measured a third of a core spent on fullscreen shm
updates on a devkit, and Plasma 6.7 removes the copy by wrapping each shm
buffer in a udmabuf so the GPU imports it directly. linux-dmabuf passes GPU
buffers once and attaches them by object every frame, with a feedback protocol
to negotiate scanout-capable allocations, and explicit synchronisation
through DRM syncobj timeline points landed in 2024 with small gains except on
NVIDIA. No Wayland compositor enforces memfd seals, because older clients map
their buffers read-write. Android's BufferQueue never copies buffer contents;
gralloc handles cross Binder in a fixed acquire, queue and release cycle with
fences. Fuchsia's sysmem hands every participant a token, collects
constraints, and allocates only after all have answered; its image pipes then
stream a pool of two or three images with present and release fences. PipeWire
is the one transport-neutral protocol: it negotiates memfd or DMA-BUF buffers
by format and modifier, passes them by identifier per frame, and since 1.2
carries two extra descriptors per buffer for acquire and release timelines.
Arcan's SHMIF is a static shared region treated as contested memory. Plan 9's
draw device copied image bytes through a data file in 64 KiB units.

The kernel history matters in two places. File sealing was added in 2014
precisely so that zero-copy IPC could work without a trust relationship: the
sender seals against shrink, grow and write, and the receiver checks the seals
before reading in place. Read-only shared mappings of a write-sealed memfd
were refused until a fix merged in late 2023, so the contract needs a minimum
kernel. Linux 6.18 on crunch is fine; the C kernel fixture in the desktop SDK
proved the read-only mapping.

Two file-protocol precedents were checked against kernel sources by a second
review the same day. virtio-fs keeps FUSE file operations on its virtqueues
and, for DAX-eligible files, the guest kernel sends FUSE_SETUPMAPPING to have
host file extents mapped into a pre-established virtio shared-memory window;
later access touches host pages directly with no FUSE read. The mapping
requests are in-band; only the window itself sits beside the protocol. FUSE
passthrough, in Linux 6.9, has the server register a backing descriptor once
through an ioctl on /dev/fuse and answer open with a passthrough flag, after
which reads, writes and mappings bypass the daemon. Registration names a live
file whose content is whatever it holds at access time; revocation cannot cut
off files already opened; nobody validates content after delegation; and the
server needs CAP_SYS_ADMIN and a kernel FUSE mount. Both precedents are
"register once, control in band". Neither provides immutability, per-offer
admission, exactly-once release or revocation, and neither can express a GPU
buffer, which exists only as a descriptor.

## Finding and resolution

The costs separate into three problems that the week's work had treated as
one. How many passes a frame costs is wire shape, and the byte path is near
its floor after transaction reuse and the queued writes. What a pass costs is
the Session loop, which is t289. How many pixel bytes arrive and get checked
is validation plus the socket crossings, and no transport change touches the
validation half.

9P describes files and copies bytes. It fits everything in the shell and lock
contracts except bulk pixel transport, which it is worst at, because it has no
way to say "here is memory" or "here is a GPU buffer". Every performant stack
reaches the same shape: control over a message channel, pixels by reference,
constraints negotiated before allocation, a small pool of buffers, explicit
release, and fences instead of CPU waits. The t337 draft has the negotiation
in its limits and formats objects, the pool in its four-offer ceiling, the
release in SourceReleased, admission before send as backpressure that Wayland
lacks, and sealed immutability that Wayland cannot afford. It lacks fences,
registration for reusable GPU buffers, credits, and GPU import of sealed
memory. The diagram below shows the draft with those four added and the open
choices marked.

```
             SHELL COMPONENT / LOCK PROVIDER (supervised launch)
 ┌────────────────────────────────────────────────────────────────────────┐
 │ inherited at launch: 9P socket, endpoint dir, supervisor evidence,     │
 │ [OPEN B] shared CPU window (memfd from Sophia, mapped by both sides)   │
 │                                                                        │
 │ CPU render ──► [A] fresh sealed memfd per image                        │
 │             or [B] slot in the launch window, named by offset+gen      │
 │ GPU render ──► GBM buffer from a pool of 2-3, reused every frame       │
 └───────┬──────────────────────────────────────────────┬─────────────────┘
         │ 9P2000.L: all control, custody, events       │ SEQPACKET: descriptors
         │ (unchanged from the draft)                   │ only
         ▼                                              ▼
 ┌───────────────────────────────────────┐   ┌───────────────────────────────┐
 │ role root, per connection epoch       │   │ image.sock                    │
 │  image-channel / image-limits /       │   │  Hello  once per connection   │
 │  image-formats  (constraints before   │   │  [A] one sealed memfd per     │
 │  allocation, as sysmem does)          │   │      image, as drafted        │
 │                                       │   │  DMA-BUF: register a pool     │
 │  ImageBegin ──► ImageAdmitted, per    │   │      buffer once (phase 2)    │
 │    image, as drafted (grant check,    │   │                               │
 │    bytes, jobs, two journal slots)    │   └───────────────────────────────┘
 │  [OPEN] slot credits at ChannelReady  │                   │
 │    for the fixed-size part only       │                   │
 │  BufferRegistered ◄── new object,     │                   │
 │    DMA-BUF pool buffers (phase 2)     │                   │
 │  Present(buffer id, acquire point)    │                   │
 │  ResourceStatus / release point       │                   │
 │  SourceReleased  (exactly one per     │                   │
 │                   offer, as drafted)  │                   │
 └───────────────────────────────────────┘                   │
                    ▼                                      ▼
 ┌────────────────────────────────────────────────────────────────────────┐
 │  SOPHIA SESSION                                                        │
 │  admission: per image, as drafted; a credit could pre-reserve only the │
 │     offer slot and journal slots, never bytes, jobs or the grant check │
 │  sealed [A]: seal check, size check, map read-only, validate once      │
 │     sealed memfd ──udmabuf──► GPU image (an import, not a copy)        │
 │     [GAP] udmabuf import not in draft; today a CPU texture upload      │
 │  DMA-BUF, as drafted: capture job copies the client buffer into a      │
 │     renderer-owned immutable image; SourceReleased when capture is done│
 │     per present: wait acquire fence, capture, signal release fence     │
 │     [GAP] fences forbidden in phase 1; producer must finish on CPU     │
 │     [NOT PROPOSED] sampling the client buffer directly at composite    │
 │       time would be a separate custody change, not an optimisation     │
 │  [B] window slots are mutable → validate per present or copy once      │
 │  release: SourceReleased per offer; pool buffers stay registered       │
 └────────────────────────────────────────────────────────────────────────┘

 STEADY STATE PER IMAGE    9P: Begin, Admitted, Present or Source, status,
                           release (a handful of server passes on the model)
                           side socket: nothing for a registered pool buffer,
                                        one 200-byte packet + 1 fd for [A]
                           kernel: no pixel bytes on any socket
```

What the diagram keeps from the draft: the supervised launch, the
authenticated role root, discovery with negotiated constraints, admission
bounded to four live offers, the journal, exactly one release per admitted
offer, byte fallback, and the rule that a packet must name something 9P
already admitted.

What it adds. Credits issued at ChannelReady can pre-reserve only what is
fixed in size, the offer slot and its two journal slots. Bytes, validation or
capture jobs, and the grant check against current permission, topology and
format must still be admitted per image, so ImageBegin and ImageAdmitted stay
as drafted; the latency available is pipelining, a client holding admitted
offers ahead of the images it has not yet produced, not the removal of a
round trip. A registration object and a present-by-identifier record let a
DMA-BUF pool buffer cross the side socket once and be presented many times,
which is the wl_buffer, BufferQueue and image-pipe pattern; it is deferred to
the DMA-BUF phase, because nothing measured yet shows descriptor passing
itself to be a cost at four outstanding offers. The drafted custody stays
with it: each present is a capture job that copies the client's buffer into a
renderer-owned immutable image, and SourceReleased follows the capture.
Sampling the client's buffer directly at composite time would be a separate
custody change and is not proposed here. Acquire and release fences replace
the producer-completed rule once t289's completion seam exists. Importing a
sealed memfd into the GPU through udmabuf removes the compositor's texture
copy for CPU content; the kernel's precondition is the shrink seal, which the
draft already requires.

What it does not change, after the second review. For sealed memory a
descriptor per image is inherent, not an inefficiency: seals are whole-file
and irreversible, so a sealed backing cannot become a pool slot, and every new
immutable image is a new file. Showing the same image twice needs no second
descriptor either, because an accepted image is a retained resource that
candidates already name by identifier. Register-once applies where the
backing outlives the content, which is the GPU pool and nothing else.

Option B, the launch-time window, is recorded as considered and
distinguished. Sophia already hands a component its socket, directory and
supervisor evidence at launch, and it launches the lock provider too, so it
could hand over a shared memory window as well; images would then be named
over 9P by offset and generation with no second socket, which is the
virtio-fs and passthrough shape. The cost is that window slots are mutable:
Sophia must validate at every present or copy once, and the window provides
none of the immutability, per-offer admission, exactly-once release or
revocation the draft specifies. The kernel community's own path, from the DAX
window to passthrough, moved away from slot management. B cannot carry GPU
buffers, so choosing it means two mechanisms.

No standalone protocol exists to adopt. Wayland's buffer protocols are the
nearest and are inseparable from its display, registry and surface objects;
PipeWire is the only transport-neutral one and brings a media daemon with it.
Speaking either on the side socket would put a second object model inside
Sophia, which the native-9P rule forbids. The alternative is to mirror the
semantics of linux-dmabuf and syncobj one to one in 9P records, so reviewers
can check the design against a well-trodden specification and a future
compatibility layer for applications becomes a translation.

The shape is portable where it should be. Records, objects, admission and
release are the same on any system; descriptor passing over a Unix socket is
POSIX; memfd sealing exists on Linux and FreeBSD 13; DMA-BUF is Linux only.
Source kinds are a negotiated bitmask, so a platform advertises the kinds it
has and a client that sees none uses bytes. The normative text should describe
kind 1 by its guarantees, sealed shared memory that cannot change, rather than
by the name memfd.

On performance the honest claim is a model, not a result. Removing about 68
server read turns per 4 MiB image is worth roughly 1.4 ms and 2,700
allocations on the pilot's per-pass costs, which would put the bulk arm below
the IPC baseline rather than within 10% of it; the client's dependent waits,
already cut to about a quarter of the writes by the B3 window, go with them.
The bar gains three passes out of about eighteen and nothing else. On the pass
model its allocation gate needs fewer passes than the IPC path itself takes,
which is an inference from measured per-pass deltas, not a proved bound, and
the gates themselves are unchanged. The new floor is the validation read and,
until udmabuf import exists, the texture upload.

## Validation and remaining work

Nothing in this note is measured on Sophia beyond the pilot it cites. Before
anyone claims the model, t336 must measure the memfd page-fault and zeroing
cost on the producer, which is about a thousand faults per 4 MiB at 4 KiB
pages unless prefaulted or backed by huge pages, the validation worker's cost
per MiB, and the fixed cost per offer. The paired evidence planned for t338 is
where the bulk arm's claim is proved or not.

If the owner wants the diagram's additions in the design, the edits to the
[descriptor custody review](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md)
are a credits field on ChannelReady, a registration object and a
present-by-identifier record for the DMA-BUF phase, udmabuf import of sealed
memory on the Session side, fences after t289, a minimum kernel, and a
precedent paragraph using the corrected virtio-fs wording with passthrough as
a considered alternative. Two decisions are the owner's and block nothing yet:
A alone or A for GPU content with B for CPU content, and the panel allocation
gate in t331, which on the pass model no transport satisfies.

## Connections

The [descriptor custody review](../plans/x6jp929h-review-shared-lock-and-shell-image-descriptor-custody.md)
holds the proposed design this note draws; the
[descriptor channel ADR](../decisions/qujwzrdz-admit-shell-content-through-a-negotiated-descriptor-channel-beside-the-9p-wire.md)
records why the channel sits beside the wire; the
[lock DMA-BUF plan](../plans/r8z5a3sk-negotiated-dma-buf-lock-images-with-bounded-capture-and-release.md)
is the lock-side origin that t337 merged with the shell side. The
[Plan 9 techniques investigation](2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md)
holds the pass ledger and pilot figures this note relies on, and the
[prior-art investigation](yhsocuvc-what-prior-art-on-9p-transport-cost-says-about-the-shell-wire.md)
covers the 9P-family transports that this note does not repeat. The shell and
lock contracts are [shell files](../../sophia-shell-files.md) and
[lock files](../../sophia-lock-files.md).

External sources read on 2026-10-17:
[KDE on making wl_shm fast](https://planet.kde.org/xavers-blog-2026-05-06-making-wl_shm-fast/),
[Phosh texture-copy issue](https://source.puri.sm/Librem5/phoc/issues/81),
[DRI3000](https://mail.keithp.com/blogs/DRI3000/),
[DRI3 protocol](https://mirrors.mit.edu/NetBSD/NetBSD-release-11/xsrc/external/mit/xorgproto/dist/dri3proto.txt),
[linux-drm-syncobj-v1](https://wayland.app/protocols/linux-drm-syncobj-v1),
[memfd sealing patchset](https://lkml.iu.edu/hypermail/linux/kernel/1403.2/02109.html),
[sealed memfd and Wayland compositors](https://lkml.iu.edu/hypermail/linux/kernel/2104.3/02179.html),
[write-sealed memfd read-only mappings](https://lkml.iu.edu/2310.0/07610.html),
[Android BufferQueue and gralloc](https://source.android.com/docs/core/graphics/arch-bq-gralloc),
[Fuchsia sysmem](https://fuchsia.googlesource.com/fuchsia/+/20316514c031/docs/concepts/graphics/sysmem/sysmem.md),
[Fuchsia image pipe](https://fuchsia.googlesource.com/fuchsia/+/HEAD/sdk/fidl/fuchsia.images/image_pipe2.fidl),
[PipeWire DMA-BUF guide](https://docs.pipewire.org/1.4/page_dma_buf.html),
[PipeWire sync timeline](https://docs.pipewire.org/1.4/structspa__meta__sync__timeline.html),
[Arcan SHMIF](https://arcan-fe.com/2024/11/21/a-deeper-dive-into-the-shmif-ipc-system/),
[Plan 9 devdraw](https://plan9.io/sources/plan9/sys/src/9/port/devdraw.c).
