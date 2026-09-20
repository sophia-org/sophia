---
id: 4m65c17q
date: 2026-09-19
kind: investigation
status: investigating
tags: [investigation, x11, session]
---
# Primary selection does not reach another client from xterm

## Question

Selecting text with the mouse in xterm and pasting it into another window does
not transfer it. Why?

## Evidence

Reported from a live Hagia session on release `0.1.0-84d906f148d6`, by hand:
select with the mouse in xterm, then middle-click or shift+Insert in a kitty
window. Nothing arrives.

Ruled out already: xterm's `ctrl+shift+c` and `ctrl+shift+v`. Stock xterm 411
binds neither. Its only selection bindings are `Shift <KeyPress> Insert` and
`~Ctrl ~Meta <Btn2Up>`, both `insert-selection(SELECT, CUT_BUFFER0)`, plus
mouse drag to PRIMARY and CUT_BUFFER0. So the earlier key-based attempt never
set CLIPBOARD and never reached Sophia. **The mouse path is different: it is
the ordinary PRIMARY transfer and it should work.**

Not yet established: whether the owner is recorded
(`SetSelectionOwner`), whether the requestor's `ConvertSelection` produces a
`SelectionRequest` to the owner, whether the owner's reply is routed back as
`SelectionNotify`, and whether the two clients being in different namespaces
sends the transfer through the clipboard portal
(`crates/sophia-x-authority/src/clipboard.rs`, which has
`SameNamespace`, `UnknownRequestorNamespace` and `Portal` refusal cases).
CUT_BUFFER0 is a property on the root window and is a second, independent path
worth checking separately.

The live session emitted no selection records, but the only one that exists,
`sophia_live_selection`, is written at teardown, so its absence says nothing.

## What the code says, 2026-09-20

Three of the four questions above are answered, and together they move the
suspect rather than narrow it.

**Two X clients on one display are always in the same namespace, so the
clipboard portal is not implicated.** The namespace belongs to the listener,
not to the connection: `run_x11_core_socket_server(path, namespace)` takes one
and serves every client that connects on it. A cross-namespace transfer
describes two displays, not two clients. So the `SameNamespace`,
`UnknownRequestorNamespace` and `Portal` cases in `clipboard.rs` are not on
the path this report walked, and the question of one namespace against two
answers itself.

**The same-namespace transfer already works, and a gate already proves it.**
`x_server_frontend_routes_selection_notify_to_the_requestor_client`
(`tests/x11_wire/admission_frontend.rs`) runs two real clients over a real
socket through the whole round trip -- SetSelectionOwner, ConvertSelection,
the SelectionRequest routed to the owner, ChangeProperty, SendEvent of
SelectionNotify routed back to the requestor, GetProperty -- and asserts the
bytes arrive. It passes. The routing is in
`connection/protocol_routing.rs`, which sends SelectionRequest to the client
owning the owner window and SelectionNotify to the requestor.

So the earlier note was wrong to say nothing in the gates exercises a
cross-client selection transfer. What is missing is a *real-client* smoke,
which is a different thing and still missing.

**CUT_BUFFER0 works too, and is now covered.** Added
`cut_buffer_zero_on_the_root_is_readable_by_another_client`
(`tests/x11_wire/selection_cut_buffer.rs`): one client writes the buffer on
the root, a different client reads it back with its type and format intact.
The second path is sound, independently of selection ownership, targets,
timestamps and the portal.

## Where that leaves it

Every mechanism this report blamed is demonstrably working. The remaining
possibility is that the transfer never begins -- that **xterm never takes
PRIMARY at all**, because the mouse drag that would make it do so does not
reach xterm as a drag. A selection gesture is a button press, motion while
held, and a release; if any of those is not delivered the way xterm expects,
there is no SetSelectionOwner to route and nothing downstream is at fault.

That is now a testable claim rather than a guess, because XTEST can drive the
gesture. It could not before: this needed a hand on a mouse in a live
session, which is why the original report is a by-hand one.

Read [a key routed to focus is refused when the pointer is over another
client's window](r7k2mvqd-a-key-routed-to-focus-is-refused-when-the-pointer-is-over-another-clients-window.md)
beside this. It is not the same defect -- that one is about keys -- but it is
the same shape: input refused for a reason about the pointer's position, at a
layer no wire profile sees, and invisible to every gate because the wire is
answered.

## Validation and remaining work

- [x] Establish whether one namespace behaves differently from two: they
      cannot differ here, because a display is one namespace.
- [x] Check CUT_BUFFER0 independently: it works, and is now covered.
- [x] Establish that the same-namespace selection round trip works: it does,
      and a socket-level gate already proved it.
- [ ] Drive a real mouse selection into a real xterm with XTEST and establish
      whether SetSelectionOwner is ever sent. This is the question the
      evidence now points at, and the first one to answer.
- [ ] Add the real-client smoke once that is known. The wire is covered; a
      client that behaves like xterm is not.

Open work is tracked as t124 in `todo.md`.

## Connections

- [A kitty window waits out a four-second layout budget before it opens](cnbxdj48-a-kitty-window-waits-out-a-four-second-layout-budget-before-it-opens.md) --
  same session, and the same reading method.
