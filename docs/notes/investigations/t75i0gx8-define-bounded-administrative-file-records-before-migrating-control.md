---
id: t75i0gx8
date: 2026-10-14
kind: investigation
status: investigating
tags: [investigation]
---
# Define bounded administrative file records before migrating control

## Question

How can t254 begin moving existing administrative commands to the shared 9P
core without introducing new command authority or execution owners?

## Current resumption

The first slice adds bounded invocation/outcome text codecs in
`sophia-protocol::admin_files`, with literal and malformed-input controls and
the [version 1 grammar](../../sophia-admin-files.md). It preserves existing
command and outcome types. This is not a running administrative export, and
the legacy service remains active. No endpoint, default or installed session
has changed. t254 remains open.

Next pin discovery/status initialization and file lifecycle, move the passive
control values out of the legacy codec, then connect the export to the existing
ControlTicket claim/settlement owner. Preserve HostDomain checks at attach and
dispatch. Model cancellation/replay and terminal custody before changing those
owners; a line parser does not prove their temporal invariants.

## Evidence

The t254 plan explicitly allows this lane to proceed independently of t252
after t323. Inspected `crates/sophia-runtime/src/control.rs`, its `transport.rs`
and `client.rs`, and `crates/sophia-protocol/src/ipc/control_v1.rs`. Current
commands include registered policy actions and revision-2 Session operations;
the deferred output invocation is not part of this slice.

## Finding and resolution

Command names may contain spaces, so splitting every word or adding shell
quoting would change their meaning. A counted UTF-8 tail preserves exact text
and keeps the record human-readable. Positive canonical identities avoid
alternate replay spellings. The parser rejects extra lines, control characters,
unknown enum tokens and oversized records before allocating their owned text.
Successful decoding establishes syntax only; authorization and freshness remain
owner checks.

## Validation and remaining work

The three focused protocol tests pass. They cover literal bytes, invalid
records, exact size boundaries and every outcome, including Indeterminate.
The device-hidden `cargo xtask check` passed (exit 0), including workspace tests
and strict Clippy. Hardware-dependent proofs were skipped. The log is
`~/.local/state/sophia/development-evidence/t252-owner-review-20261014/check-device-hidden-private-runtime.log`;
the [t252 investigation](lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md)
records the validation environment and earlier attempts. `git diff --check`
passed. Runtime export,
catalog, independent client, CLI migration, performance and rollback gates
remain; no command service migration is claimed.

## Connections

- [t254 scope](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t254--migrate-administrative-commands).
- [File grammar](../../sophia-admin-files.md).
