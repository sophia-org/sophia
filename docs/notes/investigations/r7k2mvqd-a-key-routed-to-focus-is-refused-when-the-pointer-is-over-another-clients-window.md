---
id: r7k2mvqd
date: 2026-09-20
kind: investigation
status: resolved
tags: [investigation]
---
# A key routed to focus is refused when the pointer is over another client's window

## Question

XTEST key injection targets the focused surface unconditionally. Does it
matter where the pointer happens to be when the key goes in?

## Evidence

Measured by the obligations lane while writing the stalled-reader test, and
reported to the adapter lane on 2026-09-20. On one private instance, a key
routed to the FOCUSED surface while the pointer sat over another client's
surface was answered `RouteRejected` in 13 ms rather than delivered. After a
scroll routed to the focused client's own surface, the same key was
`Flushed`.

The ordering is the tell: nothing about the key changed, only where the
pointer was.

## Finding

The refusal is in `key_pointer_path`,
`routing/private_native_key_routing.rs:29-38`. Before a key is routed, the
pointer's surface is resolved against a projection, and only two are
admitted:

```rust
let source = if focus_selected.geometries.contains_key(&position.surface_window) {
    focus_selected
} else if selected.geometries.contains_key(&position.surface_window) {
    selected
} else {
    return Err(R::Applied(PrivateAppliedRefusal::HierarchyMissing));
};
```

`selected` is the recipient's selection state and `focus_selected` the focus
client's. When the pointer is over a THIRD client's window neither projection
contains it, so the else arm is taken and the key is refused. The scroll in
the measurement moves the observation onto the focused client's own surface,
the first arm matches, and the same key goes through.

**This is not a question about the key's target.** The key's target is
already decided and is correct; what refuses it is the *pointer's* position,
which the routing consults in order to build the path.

## Why it matters for XTEST specifically

The adapter resolves a key's target to the focused surface with no pointer
involvement at all -- `connection/xtest.rs:285-289` takes
`(Some(surface), _) => surface` and falls back to the root surface only for
motion. So an admitted injector's key is correct by construction and is then
refused by the executor for a reason that has nothing to do with the
injection: wherever the user last left the pointer. On a real desktop that is
most of the time.

The XTEST wire profile does not catch it, and would not: the wire is
answered, and the refusal is behind it. The profile reads 40/40 with this
present.

## Why the acceptance groups do not cover it

They pin the two adjacent cases and neither is this one.

- A key with the pointer over the focus client's own surface -- the first arm.
- A key over the bare root, which is the branch at
  `private_native_key_routing.rs:13-27`. That branch was added during M5
  precisely because the initial pointer observation is over the root at
  screen centre and every key was refused before it existed.

Cross-client was never exercised, because the XTEST acceptance instances are
effectively single-client: the pointer is never over somebody else's window
when a key goes in.

## What this needs

A decision, not a patch, and the note should not pretend otherwise. Two
readings, and they differ in what a key means:

1. **A key routed to focus should not consult the pointer's projection at
   all.** The pointer's position is not part of what a focused key addresses;
   consulting it is what introduces a dependency on an unrelated client's
   geometry.
2. **The projection should fall back the way the bare-root branch already
   does** -- read the path from the focus client's own projection, descending
   from the root, when the pointer's surface belongs to neither party.

Reading 2 is the smaller change and has a precedent in the same function.
Reading 1 is the one that says what a focused key is. Whichever is taken, the
group that was missing is the same: a key injected while the pointer is over
a third client's window.

## Resolution

Repaired 2026-09-20, and by neither of the two readings above: the machinery
to do the right thing was already there and the refusal was simply reaching
the decision first.

`normal_key_target` already handles a pointer in a branch the focus window is
absent from -- "A known disjoint pointer branch means delivery starts at focus
itself", which is the core protocol's own rule. A key press is reported to the
pointer's window only when the focus window is one of its ancestors, and
otherwise to the focus window. A pointer over a third client's window is
exactly that second case.

So `key_pointer_path` no longer refuses when neither projection holds the
pointer's surface. It answers with the root alone, which is what the client
can actually establish -- the pointer is under the root, in a branch outside
its projection -- and the resolver's existing disjoint path delivers at focus.
Reading 1 was wrong to suggest the pointer should not be consulted at all: a
key event carries the pointer's coordinates and the core rule depends on the
hierarchy, so it must be. Reading 2 was close but described a fallback that
did not need writing.

A window that has simply gone is handled the same way and deliberately: from
the client's side an unseeable branch and an absent one are one fact, and
refusing a key is worse than delivering it in either.

The control is
`native_key_reaches_focus_while_the_pointer_is_over_another_clients_window`.
Its first version passed without the repair, because it moved the selection
state's pointer snapshot while the routing reads the input authority's
observation -- a test bound to a claim it did not make. Corrected, it fails
with the production symptom, `KeyboardPreparation(Applied(HierarchyMissing))`.
It also pins that no child is named in the event: a window in a branch the
client cannot see is not one of its children, and naming it would hand a
client another client's resource id.

## Connections

- [Binding the last native obligations](njr7sd2q-binding-the-last-native-obligations-what-each-proves-and-what-stays-unmet.md) --
  where the measurement was recorded, as a routing-model question beside the
  obligation work that turned it up.
