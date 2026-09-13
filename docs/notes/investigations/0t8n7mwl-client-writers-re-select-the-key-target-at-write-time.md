---
id: 0t8n7mwl
date: 2026-09-12
kind: investigation
status: investigating
tags: [investigation]
---
# Client writers re-select the key target at write time

## Retraction

This note was filed as "Asynchronous client writers publish seat modifier
state out of order". **That finding is withdrawn.** It was wrong, not merely
unreproduced, and the original title asserted the thing that is false.

The claim was that `xkb_modifiers` is one seat-global `Arc<AtomicU16>` cloned
into every client's writer, so a slow writer could publish an older modifier
projection over a newer one from a different client.

It is allocated **per connection**. `connection/dispatch.rs:451` constructs it
inside `serve_x11_core_socket_client_with_trace_observer_and_input`, the
function that serves one admitted client, and clones it only into that
connection's own writers at `:517` and `:542`, reading it back at `:1441`
within the same connection. Two clients never share one.

So there is no cross-client last-writer-wins on that atomic, and the
`XkbStateNotify` change mask computed from it is computed against that
client's own predecessor, which is correct. The original note reasoned from
the declaration site without establishing the allocation scope, and repeated
the error in its own summary.

What this leaves: the per-client notification cache is a per-client concern.
It should not be moved into the common authority as though it were seat
state. The genuinely shared XKB state is the worker's, which is seat-keyed,
and the execution-ordering requirement rests on that rather than on this.

## The finding that stands

The same writer re-selects the delivery target rather than using the one the
event was sent to.

`x11_socket/connection/writers/input.rs:75` loads
`focused_surface_window: Arc<AtomicU64>` at write time, and `:79-91` build
`focused_fallback` and `routed_fallback` from that current value.

So an event admitted under one focus can be targeted using a focus committed
afterwards. For ordinary delivery that is the existing behaviour and is not
being changed here. What it means for the private path is that a recorded
recipient cannot be recovered by re-running selection: doing so resolves
against new focus and names the wrong client.

## What this means for the private executor

A first press resolves and records its recipient at final authoritative
execution, not at request admission. Admission reserves the request and the
cell its completion is written into; focus and grabs may still change between
then and the moment the press becomes runnable, so a recipient chosen at
admission would name a window the press never reached.

Every later release and retirement uses that recorded reached recipient. The
writer's re-selection path is not a substitute, because it answers a different
question: where would this go now, rather than where did this actually arrive.

## Status

The surviving finding is source-confirmed by reading, not reproduced. No live
regression is claimed and no ordinary-mode change is proposed.
