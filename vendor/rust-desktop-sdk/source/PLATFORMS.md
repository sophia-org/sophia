# Platforms

| Target | Status |
| --- | --- |
| Linux x86_64 | Supported and tested: Sophia's gate builds and runs these crates. |
| Linux aarch64 | Expected to work (no architecture-specific code) but not tested; not claimed. |
| FreeBSD | Planned next, with native CI. Not claimed until that CI passes. |
| OpenBSD, NetBSD | Not qualified; each is qualified separately. |
| macOS, Windows | Out of scope. |

## Rules

- Codecs, records and client state machines are portable Rust with no OS
  calls. Public APIs name no Linux-only type.
- Wire error numbers (`sophia_9p_records::Errno`) are the fixed Linux values
  9P2000.L carries (`EAGAIN` = 11, `ESTALE` = 116, ...). They are never
  converted to or from the host's errno; host failures surface as
  `std::io::ErrorKind`.
- OS calls go through `rustix` and `std::os::unix`. The generic client owns
  connect/poll; `sophia-launch-client` sets CLOEXEC on its adopted generation
  channel and shuts down its exclusive streams on authentication failure.

## Known gaps

- OS calls are not yet gathered behind one adapter module: `client_codec`
  (nonblocking connect) and `pipeline` (poll) each call `rustix` directly.
- The nonblocking connect relies on `SOCK_NONBLOCK | SOCK_CLOEXEC` at socket
  creation and on Linux's `EAGAIN` for a full listen queue; BSD behaviour
  (`ECONNREFUSED` for a full queue) is untested.
- Generic and launch clients have independent scripted-wire tests. Sophia
  separately owns interoperability against its actual core and authority owners.
- No BSD build or test has run.
