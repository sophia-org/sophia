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

Only the shell role is here today. WM, output and admin modules arrive as their
file contracts are implemented; nothing here claims a contract Sophia does not
serve.

The protocol crate also carries the descriptor, tabs, shortcut/reference and
revision-4 launcher records, native file codecs and pure validation. This
descriptor client role follows `spec/sophia-shell-files-v1.kdl` and
`spec/sophia-shell-descriptors.md`. Scripted-peer tests cover custody and
disclosure; Sophia separately tests these clients against its production export.

## Contracts

`spec/sophia-shell-files-v1.kdl` is a pinned copy of the file contract Sophia
owns (`protocol/sophia-shell-files-v1.kdl` in the Sophia repository).
`spec/sophia-shell-files.md` is the normative text of the file
contract (custody, retry, snapshot pins, role outcomes) that the client
implements, `spec/sophia-wm-files.md` the envelope, custody and retry rules
it adopts, `spec/sophia-9p-profile.md` Sophia's 9P2000.L subset, and
`spec/references/diod-9p2000L-protocol.md` the pinned dialect text, which is
GPL-licensed and kept for reference only under its own notice.
`spec/SHA256SUMS` holds every digest; Sophia's gate refuses a vendored SDK
whose copies differ from its own.

The dialect is plain 9P2000.L; wire error numbers are the Linux values
9P2000.L defines, whatever the host (see `PLATFORMS.md`).

## Release 0.2.0

The SDK uses the file wire exclusively. The socket compatibility crate,
feature, connection API and frame corpora are removed. `connect_from_env`
requires `SOPHIA_SHELL_9P_SOCKET` and refuses any `SOPHIA_SHELL_SOCKET` value.
Recovery uses a complete compatible older desktop release.

Shell event loops should drain typed observations and call `wait_for_io` with
their next application deadline when idle. It wakes on socket readiness or an
SDK retry deadline. A fixed sleep between `poll_io` calls adds that sleep to
each dependent file operation; an update can require many such operations.
Both I/O servicing and the wait are bounded. Do not wait while application
work is ready to run.

## Provenance

`PROVENANCE.md` records the Sophia commit each crate was extracted from.
