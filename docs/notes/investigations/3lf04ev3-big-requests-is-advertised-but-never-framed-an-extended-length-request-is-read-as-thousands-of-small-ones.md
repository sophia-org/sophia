---
id: 3lf04ev3
date: 2026-09-23
kind: investigation
status: resolved
tags: [investigation, x11, transport, extensions, xts]
---
# BIG-REQUESTS is advertised but never framed: an extended-length request is read as thousands of small ones

## Question

When t165 let the 120 `TOO_LONG` purposes rejoin the Xproto scenario, the
110 that reached a verdict all failed the same way, and it was not the
deadlock: `INFO: Processing a big request`, then `Expect: wanted NOTHING
but got at least 42 unexpected, malformed or out of sequence
replies/errors/events`. The purpose wants one `BadLength`. What does the
authority owe a client that has enabled BIG-REQUESTS?

## Evidence

**What the authority said and did.** `QueryExtension` reports
`BIG-REQUESTS` present (`dispatch.rs`), and `BigReqEnable` is answered
with a maximum of 65535 units, the setup's own
(`dispatch/extensions/shm.rs`). The reader (`connection/io.rs`) took a
zero length field as a four-byte request, whatever the connection had
enabled: the comment beside it deferred extended frames "until a captured
client requires them".

**What the suite sends.** XTS5's Xst library is Xlib of X11R6 lineage. It
sends `BigReqEnable` at open and stores whatever the reply says as
`dpy->bigreq_size`; `_Send_Req` (`xts5/src/libproto/SendReq.c`) then
frames `TOO_LONG` as a big request: a zero length field, a 32-bit length
of `bigreq_size + 1` = 65536 units, and the body to 262 144 bytes. The
authority answered the four-byte header, then the 32-bit length as a
request, then some 65 000 zero requests, each with an error.

**What the reference does** (`xserver/os/io.c`, `ReadRequestFromClient`;
`Xext/bigreq`). `BigReqEnable` sets `client->big_requests`. From then a
zero length field means the 32-bit length after it. A request beyond
`maxBigRequestSize` is marked `ignoreBytes` to its end and `Dispatch`
answers `BadLength`, once. A request within it has its header moved over
the 32-bit field and `req_len` reduced by one unit, so every handler sees
the ordinary layout.

**Who is exposed.** xcb uses the extended encoding only for a request
longer than the setup's maximum, and only when the `BigReqEnable` reply
allows more; Xlib over xcb zeroes `bigreq_size` when the reply is no
larger than the setup's. With 65535 in the reply neither ever sends the
frame. The exposure was a client of the suite's lineage, or a hand-rolled
one, and the inconsistency of advertising what was not framed.

## Finding and resolution

Frame it, and keep the maximum. The maximum stays the setup's 65535
units, which the extension's reply repeats, so a frame within it always
fits the ordinary length field: the reader removes the 32-bit field,
rewrites the length as one unit fewer than the frame's, and hands on the
ordinary request. No decoder changes. A frame beyond the maximum, or
shorter than its own eight-byte header, is read to its end through a
64 KiB scratch and dropped -- the reference ignores the bytes the same
way rather than let a client's declared length end its connection -- and
the request is owed one `BadLength`, which the dispatcher answers from the
frame alone without decoding anything
(`XWireParseError::BeyondMaximumLength`).

The enable is per connection and tracked by the connection loop from the
`BigReqEnable` request itself, before its reply leaves: the reader needs
it from the next request, and a client cannot use the encoding before the
reply anyway. Until then a zero length field means what the core protocol
says, a four-byte request, which is the control test.

`tests/x11_wire/big_requests.rs`, in both byte orders: a frame one unit
past the maximum -- the suite's own 262 144 bytes -- is answered with one
`BadLength` naming its opcode, and the request after it with its reply,
nothing between; a frame within the maximum is served as the `InternAtom`
it carries, interning the same atom as the ordinary request; a zero-length
header on a connection that never enabled the extension stays a four-byte
request. The first two were red on the unframed reader.

## Validation and remaining work

- [x] Wire red then green; the control green throughout.
- [x] `sophia-x-authority` suite and clippy under the gate's isolation.
- [x] Xproto rerun (`.artifacts/xts-xproto/run-4-framed/`): the 110
      declared purposes pass and nothing else moves, 287 PASS of 389; the
      declarations removed and the manifest re-declared with 102, all t166
      to t169 or the suite's own.
- [x] Both scenarios through the gate on the committed candidate
      (`.artifacts/x11-profile-84c7a8e1-{selected-core,xproto}/`):
      selected-core 76 passed and 23 declared, xproto 287 passed and
      102 declared, both PASS.

## Connections

- [A client that writes without reading deadlocks its connection](l1z9cldd-a-client-that-writes-without-reading-deadlocks-its-connection-replies-are-written-blocking-on-the-reading-thread.md) --
  the deadlock the same purposes exposed first, and the rejoin that
  showed this.
- [Running XTS5 through the profile gate](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md) --
  the scenario, its declarations, and the third run's table.
- [Core X11 protocol coverage](../concepts/3wbcpd5c-core-x11-protocol-coverage.md) --
  the decided opcodes; framing sits before all of them.
