# Sophia desktop SDK for C

Native C99 libraries for Sophia desktop components. This repository builds
without a Sophia checkout and uses no Rust code. Nim clients can use its C API.

## Current coverage

The development snapshot provides a generic nonblocking 9P2000.L client and
shell file records and sessions for bar (r6), native launcher (r7), and persistent
catalog/dock (r8). The existing shell IPC backend remains available for rollback
and comparison. The imported WM socket codec is compatibility code; WM files,
output authority, and admin SDK modules are not implemented here yet.

Reusable launcher lifecycle, queueing, connection bootstrap and event-loop APIs
are under development. The record-level session API is available now. See
[the shell API notes](src/README-shell.md) for buffer lifetimes and wire behavior.

## Build and test

Requires a POSIX system, a C99 compiler, GNU make, ar, and sha256sum. Tests use
Unix socket pairs; they do not discover or connect to a desktop.

```sh
nice -n 19 make -j2
nice -n 19 make -j2 check
make install PREFIX=/usr/local DESTDIR=/path/to/staging
```

`make WITH_IPC=0` omits the compatibility library. The static libraries are:

- `libsophia-9p.a`: generic transport, link with `-lsophia-9p`.
- `libsophia-desktop.a`: native shell file codecs/session; link with
  `-lsophia-desktop -lsophia-9p`.
- `libsophia-desktop-ipc.a`: optional shell and WM socket compatibility.

Headers install under `include/sophia-desktop`; pkg-config packages are
`sophia-9p`, `sophia-desktop`, and optional `sophia-desktop-ipc`.
The package name uses SDK terminology; `-dev` is reserved for a distribution's
development package. No stable ABI or release is claimed for this snapshot.

## Contract and integration

Sophia owns the normative contracts. `spec/` holds immutable copies pinned in
[PROVENANCE.md](PROVENANCE.md) and checked by `make check-spec`. Changes require
an explicit source revision and digest update. Local tests cover literal file
vectors, pipeline failure sequences, and the legacy frame corpus. Sophia owns
the real-export integration tests and the independent Go oracle.

Local queue admission, server Submitted custody, and semantic outcomes are
separate stages. A sent record without observed Submitted has unknown custody
after disconnect. Clients must not replay it into a new attach epoch.

License: BSD-3-Clause; see [LICENSE](LICENSE).
