# Experimental Vello draw producer

This is the default-off D1 prototype selected in
[oofo1zrq](notes/investigations/oofo1zrq-a-portable-9p-core-with-optional-drawing-and-surface-clients.md).
It produces immutable CPU content from bounded rectangle/text commands. It does
not select a default client class, allocate a stable ABI, implement GPU Vello,
or change placement, presentation, input or release authority. It is independent
of the D4 producer-ring exploration.

## Run the development demo

From this worktree:

```sh
cargo-slot sophia cargo run --offline -p sophia-runtime \
  --features experimental-draw --example vello_draw_panel -- /tmp/vello-panel.ppm
```

The optional argument receives a 1920 by 24 PPM preview made from the actual
accepted resource leases. The harness drives a real private Unix socket and 9P
export, with supplied authentication decisions and simulated display completions.
It does not launch a protected client, replace a live shell or exercise native
scanout. The example shares its fixture with `tests/shell_draw.rs` deliberately.

The workload presents 64 candidates, retaining the background/label resource
throughout and replacing a separate 96 by 24 counter tile every eight candidates.
It sends nine distinct draw batches totaling 693 payload bytes and deliberately
retries every batch once (1386 payload bytes including retries). These numbers
exclude 9P framing and all ordinary negotiation, candidate, pacing, journal and
release traffic. The client sends no pixel payload. The fixture verifies accepted
bytes against an independent raster invocation, stable static backing identity,
old-tile retirement, outcome ordering and zero resident bytes after disconnect.
Its loop is not a 60 Hz native benchmark; no CPU saving or latency claim follows.

## Admission and custody

The `experimental-draw` build feature is forwarded through CLI, Session and
runtime. Default builds have neither the draw file nor its runtime renderer
dependency. In an enabled build, an authenticated content-granted shell sees
`experimental-draw-v1` after negotiation. The file is write-only; each complete
batch is one offset-zero write. Normal export authentication and revocation checks
still apply. It is an experimental extension discovered by file name, without a
new public protocol revision or capability bit. Production negotiation must be
settled separately before promoting this interface.

`Rwrite` acknowledges custody of commands, not resource acceptance or presentation.
The export holds one queued batch plus the exact last batch for retry checking.
An exact retry of the latest transaction succeeds without enqueueing or extending
its deadline. A changed/reused or older transaction fails with EALREADY; another
batch while the queue is occupied fails with EAGAIN. Malformed/oversized commands
fail with EINVAL; a mismatched grant fails with ESTALE. Transactions increase
within this file; an earlier transaction is not replayable after a later one.

The persistent image owner checks current grant, negotiated resource layout,
resource/control budgets and worker capacity before submitting raster work. The
same four-slot worker serves sealed ISB validation and drawing. It lazily creates
one rasterizer; each draw produces BGRA8 premultiplied pixels, writes a fresh
memfd, seals it against writes/size changes, and validates its retained mapping
through the existing pixel validator. Only then can the resource store accept it
and publish ordinary ResourceStatus. This prototype removes client pixel transfer;
it still allocates/seals a destination and needs the ordinary renderer upload.

The accepted resource uses normal `(id, generation)` identity, resource leases,
candidate placement checks, retirement and ResourceReleased. A content grant is
not a placement grant; candidates still require their allocation and pacing
permits. There is no separate draw compositor or presentation acknowledgment.

One command queue is bounded separately from store bytes. Before raster submission,
the exact output size is charged as staging/backing and reserved resident bytes.
The resource store retains these charges through worker completion, including
cancellation, timeout and disconnect. A cancelled transfer cannot become accepted
because the worker happened to finish before cancellation was observed. The
persistent owner settles completions against the exact retired epoch after peer
loss. A queued ResourceCancel for the same resource cannot overtake draw admission;
unrelated retirement/control progress remains available. Revoking the export
discards queued commands; no job or destination exists for those commands yet.

## Disposable DRA1 encoding and work limits

Integers are little-endian. No padding, extensions or trailing bytes are accepted.

| Field | Encoding |
| --- | --- |
| Magic | Four bytes `DRA1` |
| Transaction | Nonzero u64 |
| Grant | Connection epoch u64, content-grant epoch u64 |
| Resource | Nonzero id u64, generation u64 |
| Dimensions | Width u16, height u16 |
| Commands | Count u16, then that many tagged commands |
| Rectangle, tag 1 | x/y/width/height u16 each, RGB u8 each |
| Text, tag 2 | x/y u16 each, font size u8, RGB u8 each, text byte count u8, text bytes |

A batch is at most 4096 bytes, 32 commands and 2048 by 64 pixels (512 KiB output).
Rectangles must be nonempty and contained in the canvas. Text is 1–64 printable
ASCII bytes per command and at most 256 bytes per batch, size 8–24, with its origin
inside the canvas and glyphs clipped to it. Colors are opaque RGB; untouched
pixels remain transparent. Text uses the bundled JetBrains Mono NL font, Parley
0.11.1 and vello_cpu 0.2.0, with no client fonts, images, shaders or arbitrary paths.
The fixed font is registered once per worker rasterizer.

Queue arrival fixes an absolute deadline from the current transfer timeout; retries
cannot renew it. Cancellation is checked between commands, before/after Vello's
render call, and during existing chunked pixel validation. The Vello call and OS
allocation/sealing calls are not interruptible by this worker API: input bounds
are not a hard execution-time bound. Runtime control work stays off that thread.

The resource ledger accounts for accepted backing, not every transient Vello,
Parley or allocator byte. During one raster, scene/glyph data, pixmap, output vector
and sealed destination can overlap. Input/output ceilings and a single worker
limit work but do not establish an audited total heap bound or allocator failure
recovery. Those limits, font/render cache behavior and failure containment require
qualification before exposing this class in a production build.

## Validation and next native discriminator

Focused controls cover malformed/truncated/oversized batches, epoch admission,
retry custody, queue backpressure, cancellation ordering, retained charges,
expiry, disconnect and resource-budget refusal. Existing runtime tests cover the
shared ISB worker/store and transport. The example is also the wire regression.

The next native test needs a protected draw client using this file and the normal
Session display path, paired with a client producing equal pixels through sealed
ISB. Match content, updates, permits and presentation completion. Record both
processes' CPU, raster/upload work, memory peaks, wakeups and latency; include text
updates and a dynamic workload. Do not substitute headless timings or the 693-byte
payload count for that evidence. This branch is not installed or enabled in the
live desktop.
