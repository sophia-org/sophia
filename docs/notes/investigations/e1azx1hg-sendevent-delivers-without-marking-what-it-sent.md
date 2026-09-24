---
id: e1azx1hg
date: 2026-09-24
kind: investigation
status: resolved
tags: [investigation, x11, events, xts]
---
# SendEvent delivers without marking what it sent

## Question

XTS5 `pSendEvent 1` reports "Expected MSB set in event type ClientMessage;
got 0". The protocol sets the most significant bit of the event type on
every event SendEvent delivers: that bit is how a recipient tells a sent
event from one the server generated, and what Xt and every toolkit read as
`send_event`. Where was it lost?

## Evidence

`decode_send_event` (`wire/core/properties.rs`) copied the client's 32-byte
template verbatim into `XClientEvent::ClientMessage { bytes }`, and the
encoder (`client_output/events.rs`) adds only the sequence. Xlib's
XSendEvent leaves the template's bit clear, so the event arrived as if the
server had generated it. The SelectionNotify form of the request already
marked itself (`synthetic: true` in the decoder, `| 0x80` in the encoder);
the server's own WM_DELETE_WINDOW ClientMessage
(`x11_socket/connection/control_writer.rs`) sets the bit by hand.

## Finding and resolution

One line at the decode boundary: the copied template's type byte gets
`| 0x80`, so every SendEvent form is delivered as sent.
`tests/x11_wire/send_event_marking.rs` sends a ClientMessage with the bit
clear to the client's own window in both byte orders and reads it back
marked, with the request's sequence; the decode test's expectation for the
copied bytes carries the bit. Red on master, green after.

What this did not touch, filed as t182: the ClientMessage form is not
routed to the destination window's owner at all
(`connection/protocol_routing.rs` names only Selection*, PropertyNotify,
lifecycle and Focus), so a SendEvent between two clients reaches only the
sender. XDND, WM_TAKE_FOCUS from an external window manager and xdotool
need that delivery; it is its own seam with its own red.

## Validation and remaining work

- [x] Wire red then green in both byte orders.
- [x] `sophia-x-authority` suite and clippy under the gate's isolation.
- [x] XTS `pSendEvent 1` retires from the declared rows: the Xproto rerun
      (`.artifacts/xts-xproto/run-t168/`) moves that row and no other,
      288 passed and 101 declared.
- [ ] The gate on the committed candidate.

## Connections

- [Running XTS5 through the profile gate](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md) --
  the row this closes.
