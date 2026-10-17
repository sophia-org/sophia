---
id: qujwzrdz
date: 2026-10-17
kind: adr
status: proposed
tags: [adr, shell, gpu, performance, 9p]
---
# Admit shell content through a negotiated descriptor channel beside the 9P wire

## Context

The shell content contract carries pixels as premultiplied BGRA bytes in bounded
chunks over the authenticated 9P stream. [ADR 6ndjwffd](6ndjwffd-content-capability-design-for-sophia_shell_v1.md)
chose that path for the first experiment and recorded that shared memory,
descriptor passing and DMA-BUF "remain candidates requiring measured need and a
separate specification for ownership, synchronization, format, damage, lifetime,
fallback, and renderer failure" ([content-shell](../../content-shell.md)). The
[shell files contract](../../sophia-shell-files.md#file-descriptors-and-gpu)
keeps the same rule: no descriptor crosses the wire, every `api` line announces
`fd_transfer=none`, and Lom and Provlita render on the GPU, read back, and upload
bytes.

The measured need now exists. The [t331 native pilot](../investigations/2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md)
after the reusable writer puts Session CPU for the panel workload at 9.54 ms per
uploaded MiB on the files wire against 7.75 on the IPC comparator, and the 4 MiB
bulk workload at 1.688 against 1.379. The transport difference is the smaller
part of each figure. The larger part is work the byte path does whatever the
transport: a GPU-rendered client reads its frame back, the kernel copies it into
the socket and out again, the export copies canonical chunks into the store, and
the renderer uploads the accepted bytes into a texture for composition. The lock
design draft in `t302-integration-01/DESIGN-lock-dmabuf-phase1-01.txt` attributes
the lock screen's cost to the same four steps, and the
[wmbench investigation](../investigations/djo84ohx-repeated-kms-software-mappings-account-for-the-wmbench-fault-storm.md)
attributes Sophia's X client cost to repeated software mappings and copies rather
than to request transport. The t331 pass model adds that every round trip costs
a Session loop pass with fixed allocations and polls, so re-uploading an unchanged
frame is expensive even when the wire is quiet.

Sophia already holds the pieces. [t304](../plans/r8z5a3sk-negotiated-dma-buf-lock-images-with-bounded-capture-and-release.md#t304)
designs a separately admitted `SOCK_SEQPACKET` descriptor channel for lock
images; its revision-2 draft settles admission, release and fallback for the lock
role and leaves nine decisions open. The
[renderer import boundary](../../renderer-import-boundary.md#generational-renderer-image-residency)
defines how a client DMA-BUF is captured into a renderer-owned image and never
retained. `crates/sophia-sysv-shm/src/lib.rs` already creates and seals memfd
segments for the X shared-memory path. The content model's ordered tile
placements let a client re-offer one small tile instead of a whole frame.

## Decision

Proposed: extend the t304 descriptor channel from the lock role to shell
content, as one contract answered once for both roles, with two source kinds and
the byte path kept as the required fallback.

A shell component holding a content grant may open a second endpoint beside its
9P connection, admitted the same way: the peer's pidfd must be the launched
component, the first record carries the bound 9P connection epoch, and a 9P
disconnect or a new epoch closes it. The channel carries descriptors and the
record that describes them, nothing else. Every status, event and state change
stays on the 9P files and their journal. No descriptor ever travels on the 9P
stream.

A client that renders on the CPU offers a memfd sealed against shrinking,
growing and writing, with the seal set itself sealed. Sophia verifies the seals
and the declared size before mapping; a descriptor without them is refused by
status with a distinct reason. The sealed mapping is the resource: Sophia reads
it directly for store and texture work, keeps it for the resource's lifetime, and
closes it on retire or revocation. Immutability comes from the kernel rather than
from a copy, so the existing custody rules of immutable acceptance, exact
dimensions and rejection rather than resampling apply unchanged.

A client with a GPU grant offers a DMA-BUF with format, modifier, planes, offsets
and strides for an output composed on its granted device. The buffer is a capture
source only. Sophia imports it on the composing device, copies it into a
same-format renderer-owned image through the existing capture path, destroys the
import, closes every received descriptor, and journals `SourceReleased`. The
provider must not write the buffer before that event. The first phase offers
finished buffers with no sync file and stays same-device, as the t304 revision-2
draft proposes. Importing establishes no zero-copy composition or scanout claim.

`ResourceBegin` names the transport: upload, sealed memory or DMA-BUF. The
contract review decides between a selector field and a new record kind. Sealed
memory counts against resident bytes as uploads do; captured images count against
the renderer-image table; offers in flight are bounded like upload slots. A
per-output formats file lists the pairs of fourcc and modifier each composing
device accepts. A client without the capability, without a grant, on another
device, or on a connection that cannot carry descriptors uploads bytes as today.
The byte path stays mandatory in both SDKs.

No delta mechanism is added. A client that updates part of a presented surface
places a new small resource over the base through the existing ordered tile list;
with a sealed memfd per tile that costs one descriptor record instead of a
chunked upload. t335 remains the candidate for a true sub-rectangle write if
tiles prove insufficient.

## Alternatives

Keeping the byte path is the measured status quo. Larger chunks or a larger
msize, the B4 that t331 excluded, reduce the number of writes but copy the same
bytes the same number of times and leave the GPU readback and the texture upload
in place.

Passing descriptors on the 9P socket with `SCM_RIGHTS` is a side channel outside
the protocol, and a v9fs mount could not carry it. A separate channel leaves 9P's
semantics intact and makes the descriptor path its own auditable admission.

Unsealed shared memory with fences loses immutability: a fence does not stop
later producer writes ([mn4mzcnf](mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md#optional-optimization-and-mediation)),
so every consumer would have to defend against a client rewriting accepted
content. Sealing removes the problem for CPU buffers and copying removes it for
GPU buffers.

Compositing client GPU buffers without a copy would save one GPU copy per frame
but would compose from memory the client still owns, crosses devices badly, and
is the zero-copy claim the import boundary declines to make. It stays a later
measurement once the captured path exists.

A private engine endpoint, or borrowing X11's buffer contract, is ruled out by
content-shell; the shell keeps one authority boundary.

## Consequences

For a GPU-rendered client the path loses the readback, both kernel copies through
the socket, the store copy and the per-composition texture upload, and gains one
import and one GPU copy per frame per output in Sophia plus descriptor passing.
For a CPU-rendered client it loses the socket and store copies and gains a map
and a seal check. CPU per frame should fall to a few syscalls and records on each
side. That is an expectation to measure, not a result, and the t331 ratio gate
cannot measure it because its comparator pairs two byte paths. This change needs
its own paired evidence reporting both process CPU, frame delivery, input
response and, on the laptop, package C-state residency and wakeups, which is the
battery figure.

Acceptance changes the contract's descriptor section and `fd_transfer`
announcement and supersedes the transport choice in ADR 6ndjwffd while leaving
its content model in force; normative text is written only after acceptance. The
nine open t304 decisions, covering capability bit against revision, channel
admission, `SourceReleased` and `Superseded` semantics, synchronization, capture
failure policy, same-device scope, renderer-image identity, store share and the
acceptance point, are answered once for the lock and the shell before either
implementation. The renderer integration waits on t289's snapshot storage and
import reuse decision, as t304 already does. Both SDKs gain the channel half,
and the C SDK's public structs change layout again. Revocation stops admission
and closes Sophia's end of the channel; it cannot retract a descriptor the client
already holds, which is the existing rule for GPU grants.

### Candidates

#### t336: measure the shell copy path and its power cost

Record on the installed desktop, per frame and per component for Lom, Bemenu and
Provlita, the readback time where one exists, the bytes copied through the
socket and the store, the texture upload time and the Session passes. On the x13
record package C-state residency and wakeups per second for an idle desktop and
for a bar-only workload under Sophia and under XLibre with the same panel. This
is the measured need ADR 6ndjwffd asked for and the figure the t331 gate does not
give. Report dimensions, scale, update rate, hardware and driver as content-shell
requires.

#### t337: answer the descriptor channel decisions once for lock and shell

Resolve the nine t304 questions as one contract covering both roles, add the
sealed-memory source kind and the shell admission, write the proposed contract
text with golden records and generated rows, and record acceptance in this ADR.
No implementation before acceptance.

#### t338: sealed-memory shell resources

Receive and validate first, with every offer refused as unsupported so the
fallback works end to end; then seal checks, mapping, budgets, retire and
revocation in Sophia and the channel half in both SDKs. Red controls cover an
unsealed descriptor, a short descriptor and a write attempted after acceptance.
Paired CPU evidence against the byte path on the panel workload closes it.

#### t339: DMA-BUF shell resources with renderer-owned capture

After t289's snapshot reuse decision and sharing t304's renderer work: import on
the composing device, capture into a renderer-owned image, `SourceReleased` after
the capturing flip retires, the capture-failure policy and the same-device
formats file. Lom and Provlita render into GBM buffers and keep the mapped
readback as fallback. Paired CPU evidence on the bulk workload closes it.

## Acceptance and connections

Proposed October 17, 2026 at niltempus's request, after the t331 first-slice
native pilot showed the transport share of the shell frame cost to be the smaller
part. Not accepted. The [current contract](../../sophia-shell-files.md) and
ADR 6ndjwffd remain authoritative until acceptance is recorded here.

- [Content capability design](6ndjwffd-content-capability-design-for-sophia_shell_v1.md), the transport choice this would supersede.
- [GPU execution permission](mn4mzcnf-separate-shell-presentation-from-gpu-execution-permission.md), whose optional image transport this instantiates.
- [t304 lock DMA-BUF plan](../plans/r8z5a3sk-negotiated-dma-buf-lock-images-with-bounded-capture-and-release.md), the channel this extends.
- [t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md) and [prior-art survey](../investigations/yhsocuvc-what-prior-art-on-9p-transport-cost-says-about-the-shell-wire.md), which hold t334 and t335.
- [Sophia CPU plan](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md), which owns t289.
