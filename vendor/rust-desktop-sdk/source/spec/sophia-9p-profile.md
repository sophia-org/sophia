# Sophia's 9P2000.L profile

This is the subset of 9P2000.L that Sophia's core (`crates/sophia-9p`) serves,
in Sophia's own words and under this repository's licence. Every role contract
(the WM files, the shell files, inspection) sits on top of it and adds only
nodes, bodies and admission. The independent oracle (`tools/9p-oracle`) and
any future client are judged against this page.

## Sources

- The base messages and their semantics are those of 9P2000, as described in
  the Plan 9 manual, section 5 (`intro(5)`, `version(5)`, `attach(5)`,
  `walk(5)`, `open(5)`, `read(5)`, `clunk(5)`, `flush(5)`, `remove(5)`).
- The `.L` messages (`Tlopen`, `Tgetattr`, `Treaddir`, `Rlerror`) and their
  numbering follow the pinned 9P2000.L text: diod `protocol.md` at
  `de51d1ee1bd5`, kept verbatim in
  [`references/diod-9p2000L-protocol.md`](references/diod-9p2000L-protocol.md)
  (see [the control bus](sophia-9p-control-bus.md#dialect-transport-and-api-identity)).
  The Linux v9fs client is the reference peer the dialect was made for.

Where this page and those texts differ, this page states what Sophia does, and
the difference is deliberate.

## Transport and version

- One stream per client over a Unix socket that the role owner supplies or
  admits. Framing is `size[4] type[1] tag[2]`, little-endian; a frame whose
  size is shorter than the header, larger than the negotiated `msize`, or
  inconsistent with its body ends the connection.
- `Tversion` resets the connection's state whatever its outcome. The dialect
  `9P2000.L` is accepted; `9P2000.L.Google.N` (sent by the pinned Go client)
  is answered as plain `9P2000.L`, with no extension claimed. Any other
  version is answered `unknown`. The reply never echoes a suffix.
- `msize` is the smaller of the client's offer and the export's bound. Bounds
  are set per export between 512 bytes and 16 MiB; the defaults are a 64 KiB
  maximum and a 4 KiB minimum.

## Served messages

| Request | Reply | Sophia's rules |
| --- | --- | --- |
| `Tversion` | `Rversion` | As above. |
| `Tattach` | `Rattach` | `afid` must be `NOFID` (no protocol authentication; `EINVAL` otherwise). A fid already in use or `NOFID` is `EBADF`; the fid table bound is `EMFILE`. `uname`, `aname` and `n_uname` reach the export as data and grant nothing; the export decides admission. |
| `Twalk` | `Rwalk` | At most 16 names. `..` asks the export for the parent, and from the attach root it stays at the root, as 9P requires. `.`/empty names, and names containing `/` or NUL, are `ENOENT`. Walking from a file is `ENOTDIR`. |
| `Tlopen` | `Rlopen` | Accepted flags: the access mode (read, write, read/write; mode 3 is `EINVAL`), `O_TRUNC`, `O_APPEND`, `O_DIRECTORY`, and `O_NOCTTY`, `O_NONBLOCK`, `O_LARGEFILE` and `O_CLOEXEC`, which change nothing. Any other bit is `EINVAL`. Opening a directory for writing is `EISDIR`; `O_DIRECTORY` on a file is `ENOTDIR`. The export decides each open. |
| `Tread` | `Rread` | May wait: a read at an event file's end is answered when data arrives, or never if flushed. Waiting requests are bounded (default 32). |
| `Twrite` | `Rwrite` | The export decides what a write means and how much it accepts. |
| `Treaddir` | `Rreaddir` | Directories only, within the reply size. Discovery grants nothing. |
| `Tgetattr` | `Rgetattr` | Mode, link count, inode (qid path) and size. |
| `Tclunk` | `Rclunk` | Releases the fid and tells the export. |
| `Tflush` | `Rflush` | Cancels a waiting request with that tag; the flushed request is never answered. |
| `Tremove` | `Rlerror EOPNOTSUPP` | The fid is still clunked, as 9P requires of remove. |

Every operation on a fid is first put to the export's check, so a revoked
authority ends at once, through fids that are already open as well.

## Refused messages

`Tauth`, `Tstatfs`, `Tlcreate`, `Tsymlink`, `Tmknod`, `Trename`, `Treadlink`,
`Tsetattr`, `Txattrwalk`, `Txattrcreate`, `Tfsync`, `Tlock`, `Tgetlock`,
`Tlink`, `Tmkdir`, `Trenameat` and `Tunlinkat` are answered
`Rlerror EOPNOTSUPP`. Any other message type is `Rlerror ENOSYS`. Clients
never create, rename or remove nodes: every node is the export's.

## Bounds

Per connection: waiting requests (default 32), fids (default 256), unsent
output (default four maximum-size messages, never less than one) and, per
export, connections (default 16). Exceeding a bound refuses the request with
the errno above or, for output the peer does not read, ends the connection.
Replies the core owes are reserved before a request is admitted.

## Access paths

Direct clients over the supplied socket are the accepted path. Mounting an
export through Linux v9fs is not yet accepted; see the control bus for the
open questions.
