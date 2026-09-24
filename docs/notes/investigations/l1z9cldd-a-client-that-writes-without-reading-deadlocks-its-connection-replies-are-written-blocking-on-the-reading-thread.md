---
id: l1z9cldd
date: 2026-09-23
kind: investigation
status: resolved
tags: [investigation, x11, transport, xts]
---
# A client that writes without reading deadlocks its connection: replies are written blocking on the reading thread

## Question

The first all-Xproto XTS5 run against the fixture host stopped inside its
first case, `pAllocColor`, after two of three purposes, and stayed there.
What was the third purpose waiting for?

## Evidence

The third purpose is `Bad B 1`, test type `TOO_LONG`: a request whose
length field is one more than the server's advertised maximum. Our setup
advertises 65535, so `_Send_Req` (`xts5/src/libproto/SendReq.c`) writes a
header with length 0 and then `(65535 + 1) * 4` = 262144 bytes of body in
one synchronous write, and only then reads, expecting `BadLength`.

Reproduced without XTS, against a fresh `x11_conformance_host`, with a
Python client (`.artifacts/xts-xproto/`): a bare zero-length AllocColor
header alone gets `BadLength` at once and the connection is served
afterwards. The same header followed by the 262 KB body:

```
WRITE BLOCKED after 5.0s: the server stopped draining without closing
first record: type 0 code 16 seq 1
```

The server answered `BadLength` for the four-byte request, then read the
body as requests: the colormap id's bytes made one garbage header, and the
zeros after it made some 65 000 four-byte requests with opcode 0, each
answered with an error. Replies are written with `write_all` on the
connection's own thread (`connection/io.rs`, `writers.rs`), so the server's
writes filled the client's receive buffer while the client, still inside its
262 KB write, was not reading; the client's write filled the server's
receive buffer while the server, blocked in a write, was not reading.
Neither side moves again, and nothing times out. TET has no per-case
timeout by default, so the whole scenario waited on the adapter's deadline.

The reference server does not deadlock here: it reads requests and buffers
output independently, so the errors accumulate until the client reads them.

## Finding and resolution

The seam is the one mutex every writer of a connection's socket already
serialises on. `X11ClientOutput` (`connection/output_spill.rs`) now owns the
socket behind it: every write becomes a non-blocking send, whatever the
kernel refuses is kept whole and in order in a per-connection spill, and a
drain thread moves the spill into the kernel as the client reads. Wire order
is the mutex order, as before; the reader never waits on the recipient. The
existing writers were kept byte for byte by an `io::Write` impl on the new
type -- one write call is one record, accepted whole into the kernel or the
spill -- and the private ordered writer drains the spill before its own
send and otherwise keeps its custody rules and six-second policy.

Two bounds end a client that will not take its output, both in the
departed-peer vocabulary and both with one record,
`sophia_x11_client_output schema=1 status=ended cause=… client=…`:

- **saturated**: more than 16 MiB owed. A client that keeps writing and
  never reads.
- **silent**: output owed, and neither drained nor any request read for
  six seconds. A client that stopped reading and stopped asking. The drain
  measures it; the reader's request reads reset it. A client merely a
  buffer's worth behind is not yet a failed endpoint, and the reference
  server keeps it too.

Modelled first: `validation/tla/X11ClientOutputSpill.tla`, its positive
config and three negative controls (today's blocking writer, no byte bound,
no silence allowance), each failing its named property.

The reproduction, against a fresh `x11_conformance_host`: 262 144 bytes
written in 0.24 s, `BadLength` first, 65 505 errors, then the reply to the
request sent afterwards, sequence numbers consecutive throughout.

What changed for the tests that assumed a blocking writer: the stalled
XFixes watcher floods past the kernel's buffer and stays silent past the
allowance, and is ended by it; the input-recovery test asserts that a
writer facing a full kernel buffer returns at once and that the disconnect
still refuses further output; the private-control tests were timing
sensitive to teardown, which now wakes the drain the instant it is told to
stop rather than at its next fifty-millisecond slice.

## Validation and remaining work

- [x] Wire red then green, `tests/x11_wire/flooding_client.rs`: the burst
      client is served and receives one answer per request in order; the
      never-reading client is ended at the bound while a peer is served.
- [x] The model, registered in `tools/check_tla.sh`; the full check passes.
- [x] `private_stalled_reader`, `routed_service` backpressure,
      `graceful_disconnect`, `peer_write_failure` and the record tests stay
      as they were.
- [ ] The 120 excluded `TOO_LONG` purposes rejoin the Xproto scenario:
      recorded in [fy4a5tes](fy4a5tes-running-xts5-through-the-profile-gate-what-the-core-protocol-suite-says-about-the-authority.md).

## Connections

- [Independent X11 socket conformance exposes missing client completions](wzxlxbok-independent-x11-socket-conformance-exposes-missing-client-completions.md) --
  the XTS adapter and its selected-core scenario.
- [xterm as an oracle](vm14kz5r-xterm-as-an-oracle-for-what-the-frontend-delivers-to-a-widget.md) --
  the other way a real client reports on the frontend.
