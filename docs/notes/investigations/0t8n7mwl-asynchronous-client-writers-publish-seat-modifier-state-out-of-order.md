---
id: 0t8n7mwl
date: 2026-09-12
kind: investigation
status: investigating
tags: [investigation]
---
# Asynchronous client writers publish seat modifier state out of order

## Question

Each X client has its own writer thread, and every one of them publishes seat
modifier state after writing its own event. Can a slower writer publish an
older projection over a newer one, and does the same pattern affect which
window a key is delivered to?

## What the source shows

Two findings, both read directly rather than reproduced. Neither is a claim
about observed live behaviour.

### Modifier publication is last-writer-wins across threads

`x11_socket/connection/writers/input.rs:648` performs

```rust
let previous = xkb_modifiers.swap(u16::from(key.modifiers_after), Ordering::AcqRel);
```

after the event record has already been written to that client's stream. The
target is `xkb_modifiers: Arc<AtomicU16>` (`writers/input.rs:9`), allocated once
per seat and cloned into every client writer, so it is seat-global rather than
namespace-scoped.

The swap is unconditional. Nothing compares the incoming `modifiers_after`
against what is already published, and nothing orders the write by the event's
sequence. Writers run independently, so if one is delayed between its stream
write and its swap while another completes both, the delayed writer's older
projection lands last and stands. The seat then reports modifiers belonging to
an event that has already been superseded.

`XkbStateNotify` emission is derived from the same swap: `changed` is computed
as `previous ^ modifiers_after`, so an out-of-order swap also computes a change
mask against the wrong predecessor.

### Key target selection is re-derived at write time

The same writer re-selects the delivery target rather than using the one
recorded when the event was admitted. `writers/input.rs:75` loads
`focused_surface_window: Arc<AtomicU64>` at write time, and `:79-91` build
`focused_fallback` and `routed_fallback` from that current value.

So an event admitted under one focus can be targeted using a focus committed
afterwards. For ordinary delivery this is the existing behaviour and is not
being changed here, but it means a recorded recipient cannot be cleaned up by
re-running selection: doing so would silently resolve against new focus and
retire the wrong recipient.

## What this means for the private executor

The private executor must own authoritative modifier publication. Per-event
XKB notifications to each client stay where they are, since those are that
client's own view of its own delivery; what must not continue is asynchronous
writer completion overwriting current seat state that the executor is
responsible for.

Recorded-recipient cleanup must use the recipient recorded at admission. The
writer's re-selection path is not a substitute, because it answers a different
question: where would this go now, rather than where was this sent.

Both belong to the private `Option` integration alongside final recipient
ordering. Ordinary mode is unchanged.

## Status

Source-confirmed, not reproduced. No live regression is claimed and no
ordinary-mode change is proposed here.

Related: the control-epoch coordinator, which owns transition sequencing for
the same private path.
