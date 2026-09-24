---
id: 8vq35mxx
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, core-protocol, events, routing]
---
# A SendEvent reached only its sender

## Question

t182, left open by the SendEvent marking seam (t168): the ClientMessage
form of SendEvent was validated and echoed back to the sending client, and
routed to nobody else. XDND, a window manager's WM_TAKE_FOCUS and
WM_DELETE_WINDOW, EWMH requests aimed at the root with a mask, and xdotool
all depend on a sent event arriving where it was aimed. Who is owed a sent
event, and what did routing it require?

## Evidence

`decode_send_event` produced `SendSelectionNotify { destination,
event_mask, event }` for every form, and the dispatcher
(`dispatch/core/properties.rs`) validated the destination and returned the
event as the requester's own output. The routing passes
(`connection/protocol_routing.rs`) named Selection*, PropertyNotify, the
lifecycle events and Focus; a ClientMessage among the outputs fell through
every one and was written back to the sender. The writer's sequence stamp
did not know the event either, so a copy routed to a peer would have
stopped the writer. The wire test's first run: a peer's window was sent a
message and the peer read nothing in five seconds, while the sender read
the message it had itself sent.

## Finding and resolution

- The record now carries where and how it was aimed: `ClientMessage`
  gains the resolved destination, the event mask and the propagate flag,
  none of which reach the wire. The dispatcher resolves the two special
  destinations: PointerWindow is the window the pointer is in (the root
  when this authority knows of none), InputFocus the focus window, or the
  pointer's when the focus is PointerRoot or None.
- A new routing pass, `route_sent_events`, runs first among the protocol
  passes. With no mask the destination's owner is owed the event
  (`client_for_resource`); with one, every client selecting any of those
  events on the destination, climbing the ancestors through the routing's
  parent map when the request propagates and nobody on the window
  selected. Peers are delivered through the contained route (t090); the
  sender keeps its copy only when it is among the recipients.
- The writer stamps a routed ClientMessage with the recipient's sequence
  like every other routed event.

`tests/x11_wire/send_event_routing.rs`, both byte orders: a message to a
peer's window reaches the peer, marked as sent, with its window and datum
intact, and the sender keeps no copy; one to the sender's own window
reaches the sender; an unknown window is BadWindow as before; with a mask,
the client selecting those events on the window reads it and one nobody
selected reaches nobody without an error; with propagate, a mask nobody on
the window selected climbs to the root's selector, and without propagate
it does not. Red before, green after. The existing SendEvent tests (the
sent bit, the SelectionNotify form) are unchanged.

## Validation and remaining work

- [x] Wire red then green in both byte orders.
- [x] `sophia-x-authority` suite and clippy under the gate's isolation.
- [x] The core profile reads PASS, 154 of 154.
- [x] The gate on the committed candidate
      (`.artifacts/x11-profile-2af52ef3-{selected-core,xproto}/`): both
      scenarios PASS, xproto with 339 passed and 50 declared, selected-core
      with 76 passed and 23 declared.
- [ ] A ClientMessage aimed at PointerWindow with no pointer position falls
      back to the root, whose owner is nobody: recorded here rather than
      hidden, and the input authority's pointer window is what a session
      supplies.

## Connections

- [SendEvent delivers without marking what it sent](e1azx1hg-sendevent-delivers-without-marking-what-it-sent.md) --
  t168, where the gap was found and t182 filed.
- [A stalled protocol recipient can escape X11 client containment](psf52z1x-a-stalled-protocol-recipient-can-escape-x11-client-containment.md) --
  t090, the contained route every peer delivery uses.
