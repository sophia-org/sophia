# Sophia desktop SDK for Rust

Clients for the Sophia desktop's role contracts over 9P2000.L. A shell, window
manager, output or admin client links these crates; none of them needs a
Sophia checkout to build.

| Crate | What it is |
| --- | --- |
| `sophia-9p-records` | 9P2000.L value records (tags, fids, qids, wire errno, requests, replies), shared with Sophia's server core. |
| `sophia-9p-client` | Bounded 9P2000.L clients: a blocking read-only `client` and the nonblocking, pipelined `pipeline` the role clients drive. |
| `sophia-desktop-ids` | Identifiers every role contract shares. |
| `sophia-shell-protocol` | The shell's typed records and the `sophia_shell_fs_v1` file contract codec. |
| `sophia-shell-client` | The shell client over the file contract. |
| `sophia-shell-ipc` | Compatibility codec for the retiring `sophia_shell_v1` socket frames; used only by `sophia-shell-client`'s `ipc-compat` feature. |

Only the shell role is here today. WM, output and admin modules arrive as their
file contracts are implemented; nothing here claims a contract Sophia does not
serve.

## Contracts

`spec/sophia-shell-files-v1.kdl` is a pinned copy of the contract Sophia owns
(`protocol/sophia-shell-files-v1.kdl` in the Sophia repository);
`spec/SHA256SUMS` holds its digest, and Sophia's gate refuses a vendored SDK
whose copy differs. This crate's conformance tests read the pinned copy.

The dialect is plain 9P2000.L; wire error numbers are the Linux values
9P2000.L defines, whatever the host (see `PLATFORMS.md`).

## Features

`sophia-shell-client` builds the file wire only by default. The `ipc-compat`
feature adds the Unix-socket `sophia_shell_v1` wire as a rollback path; it is
removed when Sophia retires that transport.

## Provenance

`PROVENANCE.md` records the Sophia commit each crate was extracted from.
