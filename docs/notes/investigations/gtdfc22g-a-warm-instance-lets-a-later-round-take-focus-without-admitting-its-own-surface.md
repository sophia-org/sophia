---
id: gtdfc22g
date: 2026-09-22
kind: investigation
status: investigating
tags: [investigation, x11, input, focus]
---
# A warm instance lets a later round take focus without admitting its own surface

## Question

On an instance that has already served a connection pair and seen it depart,
a second pair takes the focus and receives injected keys without admitting
its own window as a surface. On a fresh instance the same sequence cannot.
What does the second round inherit from the first, and should it?

## Evidence

Found under t134 on `428dfa51` while building
`a_key_after_a_departure_reaches_the_live_observer_on_a_shared_instance`
(`crates/sophia-session/tests/support/xtest_acceptance/departure_witness.rs`),
by mutating the witness rather than by observing a failure.

`admit_surface` in `tests/support/xtest_acceptance/groups.rs` documents the
rule it exists to satisfy:

> here nothing does unless the group does, and a key resolves its recipient
> through the focused window's surface, which only an admission creates.
> Without this a key is planned against no target and silently goes nowhere,
> which is not a delivery failure the wire can see.

Two mutations of the witness, each a single call removed:

| round the admission was removed from | instance state | result |
| --- | --- | --- |
| second | already served a departed pair | passes: focus is taken and both injected keys arrive at the new observer's window with the right keycode |
| first | fresh | fails in setup, inside `Client::reply` at `client.rs:224`, before any key is injected |

So the documented rule holds on a fresh instance and does not hold on a warm
one. The difference is not the byte order and not the keycode: it is whether
the instance had previously admitted a surface for a connection that has
since departed.

## Finding and resolution

Not yet diagnosed. The candidates worth separating before anything is
changed:

- The engine retains scene or output state from the first admission that
  makes a later window viewable without an admission of its own, in which
  case the second round is relying on something real but undeclared.
- The focus path reads a different store from the one the admission writes,
  so the second round's `SetInputFocus` succeeds against state the departed
  round established. The focus work closed 2026-09-20 found exactly this
  shape once already -- fixtures "announced a window as mapped to the core
  event selection state, which is a different store from the one the focus
  rules read".

The second is the more likely and the more serious: it would mean a window's
focusability outlives the connection whose admission granted it.

## Validation and remaining work

Open as t152 in [todo.md](../../../todo.md). This did not affect t134's
result -- the round whose delivery t134 asserts admits its surface normally,
and the witness passes with the documented sequence intact. What is unproven
is whether the inherited state is benign retention or a window that stays
focusable after its owner has gone. Reproduce first by narrowing the two
candidates above; do not change `admit_surface` or the witness to suit
either until one is established.

## Connections

- [Private native input authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md) --
  owns t134, whose witness found this.
- [M5 XTEST adapter accepted and the one obligation left open](../milestones/vlcrn30a-m5-xtest-adapter-accepted-and-the-one-obligation-left-open.md) --
  records the focus-store distinction this resembles.
