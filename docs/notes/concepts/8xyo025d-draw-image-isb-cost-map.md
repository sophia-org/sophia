---
id: 8xyo025d
date: 2026-10-21
kind: concept
status: draft
tags: [concept, shell, draw, image, isb, cost]
---
# Draw, image, ISB, and where the CPU goes

Reference map for the t331/t341 shell image work: how the draw and
image upload paths relate to ISB, and which layer pays for what.
Measured figures are from the d4p-03 panel comparison (1920x24,
crunch); layer attributions are code locations, not per-layer
timings.

## Structure

```
                        CLIENTS
        ┌────────────────────────┐  ┌────────────────────────┐
        │ simple peer            │  │ full renderer          │
        │ (panel, status tile)   │  │ (Lom / Provlita)       │
        └───────────┬────────────┘  └───────────┬────────────┘
                    │                           │
        DrawBatch   │  draw-v1 (9P)             │  pixels
        rects/text  │                           │  owned ring r10/r11
                    ▼                           ▼
        ┌────────────────────────┐  ┌────────────────────────┐
        │ SESSION draw helper    │  │ SESSION image owner    │
        │                        │  │                        │
        │   VELLO CPU rasterizer │  │  validate geometry/    │
        │   commands -> pixels   │  │  grant, copy pixels    │
        └───────────┬────────────┘  └───────────┬────────────┘
                    │                           │
                    │    pixels (identical      │
                    │    custody transition)    │
                    ▼                           ▼
        ┌─────────────────────────────────────────────────┐
        │              ISB  (content store)               │
        │                                                 │
        │  sealed immutable resource: (grant, id, gen)    │
        │  custody, budgets, settlement barrier           │
        │                                                 │
        │  draw: adoptable only after DrawSettled         │
        │  ring: adoptable only after status +            │
        │        SlotReleased                             │
        └───────────────────────┬─────────────────────────┘
                                │
                                │  resource leases
                                ▼
        ┌─────────────────────────────────────────────────┐
        │  ENGINE / COMPOSITOR                            │
        │  candidates place resources from either path,   │
        │  mixed freely in one frame                      │
        └───────────────────────┬─────────────────────────┘
                                │
                                ▼
                           scanout / display

  DMA-BUF sources ──────────────────────► ISB (same adoption point,
                                            different backing)
```

Vello never touches ISB directly; it is a producer behind the draw
path. Draw and image are two front doors into one custody model:
a settled draw batch and a settled ring upload become the same kind
of sealed, immutable ISB resource and are placed identically by
candidates.

## Cost / where

Per-process CPU is what the native screens measure (Session, peer,
helper). Inside Session, the work splits by layer:

| Cost | Where |
|---|---|
| 9P wire protocol + journal | Shell transport (`ShellComponentTransport` / ShellFiles export): decode, fid custody, event journal |
| Byte validation + hashing | ISB side: `ShellIsbImageOwner` and its validation worker check geometry/format/content as pixels land in the store |
| Grant/custody bookkeeping | ISB side: `ContentEpochRegistry` and the resource store (begin/complete transfers, settlement evidence, leases, budgets) |
| Candidate composition | Engine: candidate admission, placement, `begin_content_submission` -> prepared -> presented, then scanout |

One cross-boundary note: the draw path rasterizes in the helper
subprocess, but validation of the resulting pixels still happens
ISB-side before the resource is adoptable. Helper CPU is reported
separately in native measurements and is never folded into Session
or peer figures.

## What the copy elimination bought

Panel workload, per delivered frame (d4p-03):

| CPU/frame | Sealed | Owned ring |
|---|---:|---:|
| Session | 1.460–1.480 ms | 1.340–1.360 ms |
| Producer | 0.413–0.433 ms | 0.240–0.260 ms |
| Combined | 1.873–1.913 ms | 1.580–1.620 ms |

The sealed path copied pixels repeatedly (producer fill, socket
write, Session staging, validation, copy into the sealed resource).
The ring removes the socket and staging copies: the producer fills
the shared slot directly and Session copies once while validating.
That saved ~13–17% combined CPU; presentation cadence and latency
were unchanged (no missed refreshes in either arm).

At panel size (1920x24 ≈ 184 KB) raw copy bandwidth was never the
wall — the remaining ~1.6 ms/frame is validation passes over the
bytes, wire/journal handling, custody bookkeeping, composition,
and kernel presentation. Copying would dominate again at
full-screen sizes (1920x1080 ≈ 8 MB/frame, ~45x the payload), and
the draw path trades copies for helper rasterization instead:
an ~80-byte batch versus ~184 KB of panel pixels, cheap for
text/rect content, wrong for photographic content.
