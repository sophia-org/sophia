---
id: 8xgoow54
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, isolation, namespace, getimage]
---
# A root readback showed one namespace another's windows

## Question

While deciding what drawing on the root owes a client (t181), the root's
readback path came into view: GetImage on a window composites the window's
inferiors over its own pixels, and the walk that finds them was deliberately
namespace-blind. The root is every namespace's parent. Could a client read
another namespace's pixels off it?

## Evidence

Yes. `read_drawable_image_region` in `runtime/drawing/image_ops.rs` called
`composite_inferiors`, which walked `direct_children_bottom_to_top_any_namespace`,
commented "what covers it is a fact about the screen rather than about who
owns the windows". The root is readable from any namespace and has no CPU
backing of its own, so a root readback was exactly the composite of every
namespace's mapped top-level windows. A wire probe on master `0cd24c0e`:
namespace B maps a window and fills it with `0x00c0ffee`; namespace A reads
one pixel of the root inside it and gets `0x00c0ffee` back.

## Finding and resolution

For a window inside one namespace the old rule was harmless; at the root it
is a screen capture across the isolation boundary the namespaces exist to
keep. The readback now carries the reader's namespace and composites only
that namespace's windows (`direct_children_bottom_to_top`); the
namespace-blind walk had no other caller and is gone. ClassicShared clients
share a namespace and so still see each other, as X expects; a confined
client sees its own windows on the root and nothing else. What the root
itself holds when a namespace draws on it is t181's decision: a private
per-namespace root, never presented.

## Validation and remaining work

`x11_wire` `a_root_readback_shows_only_the_readers_namespace`: the owner
reads its window off the root, the other namespace reads zero (red before the
change). On `cc5e5076`: the core profile passes 152 of 152 and x11bench 57 of 60,
its stacking tests reading the root from one client as before. Not examined
here: other requests that read pixels across windows (CopyArea from a window
reads only that window's own backing, so it does not composite inferiors).

## Connections

- [x11bench as a pixel oracle](4ky4jyb9-x11bench-as-a-pixel-oracle-what-an-independent-drawing-suite-says-about-the-software-rasterizer.md):
  t181 is filed there.
