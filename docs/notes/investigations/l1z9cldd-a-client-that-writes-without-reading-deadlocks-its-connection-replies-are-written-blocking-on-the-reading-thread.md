---
id: l1z9cldd
date: 2026-09-23
kind: investigation
status: investigating
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

Open. The frontend needs an egress the reading side never waits on: an
outbound queue per connection, drained without blocking the reader, bounded
so a client that never reads is dropped at the bound rather than buffered
without limit. That is a transport ownership change and gets a model check
first. Until it lands, the XTS Xproto scenario excludes `TOO_LONG` purposes
by name (`xts_select.py --exclude-test-type TOO_LONG`, the exclusions
written beside the manifest) and tcc runs with `-t 120` so a deadlock costs
one case, not the run.

## Validation and remaining work

- [ ] Wire red: zero-length header plus 262 KB body, then read; `BadLength`
      arrives and the connection is served afterwards. Today the client's
      write never completes.
- [ ] The 120 excluded `TOO_LONG` purposes rejoin the Xproto scenario.
- [ ] A model for the outbound bound and the drop it decides.

## Connections

- [Independent X11 socket conformance exposes missing client completions](wzxlxbok-independent-x11-socket-conformance-exposes-missing-client-completions.md) --
  the XTS adapter and its selected-core scenario.
- [xterm as an oracle](vm14kz5r-xterm-as-an-oracle-for-what-the-frontend-delivers-to-a-widget.md) --
  the other way a real client reports on the frontend.
