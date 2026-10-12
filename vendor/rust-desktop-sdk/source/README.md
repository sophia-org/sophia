# Sophia desktop SDK for Rust

Clients for the Sophia desktop's role contracts over 9P2000.L. A shell, window
manager, output or admin client links these crates; none of them needs a
Sophia checkout to build.

| Crate | What it is |
| --- | --- |
| `sophia-9p-records` | 9P2000.L value records (tags, fids, qids, wire errno, requests, replies), shared with Sophia's server core. |
| `sophia-9p-client` | Bounded 9P2000.L clients: a blocking read-only `client` and the nonblocking, pipelined `pipeline` the role clients drive. |
| `sophia-launch-client` | Sophia's local launch-channel authentication: correlated afid exchange, bounded progress and an admitted role root. |
| `sophia-desktop-ids` | Identifiers every role contract shares. |
| `sophia-shell-protocol` | The shell's typed records and the `sophia_shell_fs_v1` file contract codec. |
| `sophia-shell-client` | The shell client over the file contract. |
| `sophia-lock-protocol` | The lock provider's `sophia_lock_fs_v1` file contract codec (experimental; see Contracts). |

The shell role is the only one with a full role client here. The lock provider role has
an experimental codec and no client. WM, output and admin modules arrive as
their file contracts are implemented; nothing here claims a contract Sophia
does not serve.

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
`spec/sophia-lock-files-v1.kdl`, `spec/sophia-lock-files.md` and
`spec/golden/sophia-lock-files-v1.records` are the lock provider contract, its
text and Sophia's golden records, which `sophia-lock-protocol` must render
byte for byte. They are pinned from Sophia's signed master merge `61d545c90`,
but the contract still marks itself revision 1 (draft): the lock codec is
experimental and can change incompatibly until Sophia accepts the contract
explicitly. A lock provider only renders: the contract carries no secret and no
unlock.
`spec/SHA256SUMS` holds every digest; Sophia's gate refuses a vendored SDK
whose copies differ from its own.

The dialect is plain 9P2000.L; wire error numbers are the Linux values
9P2000.L defines, whatever the host (see `PLATFORMS.md`).

## Release 0.3.0

The t317 development branch adds `Pipeline::auth` and `attach_with_afid`,
plus authentication qids and records. `sophia-launch-client` builds Sophia's
local launch conversation on those generic wire primitives. This is not a
claim that deployed roles require authentication.
New public enum variants require downstream exhaustive matches to be updated.
The profile copy describes Sophia's candidate opt-in export seam.

## Local launch authentication

An authorized supervisor supplies the private generation channel; the
application supplies its already-connected role stream. Adopt those descriptors
in the platform bootstrap and transfer exclusive ownership of both Unix streams
to `sophia_launch_client::Client::over`. Descriptor adoption from an environment
variable is outside this API; a number is not an authority claim.

```text
role stream                     private generation channel
    |                                      |
    +-- Tauth -> afid                       +-- attach/open rpc
    +-- read own challenge ----------------+-- relay exact challenge
    |                                      +-- read relay result
    +-- read afid status
    +-- attach WITH afid -> role root
    +-- clunk afid                          +-- clunk rpc/root
    |
    +-- return role pipeline + root, retain generation pipeline
```

`over` negotiates both 9P versions with bounded blocking I/O. That time and the
subsequent nonblocking exchange share one five-second deadline; `with_timeout`
may shorten it. Drive `poll`, then `wait_for_io` when idle, until `ready`.
`into_authenticated` returns both pipelines and the admitted root. Use that
root directly; do not issue a plain attach. Claims are bounded to 255 bytes and
must match Session policy; they do not choose a principal or namespace.

Shell clients pass that role pipeline and root to
`ShellConnection::from_attached`. It takes exclusive ownership and starts with
the shell `api` read, without another version or attach. The pipeline must be
healthy and quiescent, with the root as its only live fid. Keep the separate
generation-channel pipeline alive for the launch. Failure closes the role
connection; it does not trigger an anonymous fallback. The shell negotiation
has its own configured timeout after authentication. Scripted tests cover bar,
launcher, dock and descriptor handoffs; a production authentication join and
application migration remain separate qualifications.

Only `Tauth`'s `EAGAIN` is retried, without renewing the deadline. Malformed
records, refusals and cancellation never fall back to anonymous admission.
Dropping an unfinished conversation shuts both streams. Keep the returned
generation pipeline for the launch lifetime. `Authenticated::reauthenticate`
requires no outstanding requests, resets both wire conversations and verifies
the same claims again. Old role fids are invalidated; Session retains its
immutable transport identity. The generic pipeline does not interpret any of
this policy.

The crate's tests use independently scripted peers. Real protected-process
interoperability belongs in Sophia and must be recorded separately; scripted
success is not a Factotum verification claim. Linux x86_64 is the qualified
platform; no remote authentication or prevention of intentional delegation is
claimed.

Adds `sophia-lock-protocol`, the lock provider's experimental file contract
codec (no client), and `Pipeline::write_secret` in `sophia-9p-client`, which zeroes
every copy of a secret request it leaves behind. Both are additive.

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
