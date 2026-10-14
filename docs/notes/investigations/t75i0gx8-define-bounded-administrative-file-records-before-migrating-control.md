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

The codec foundation now includes invocation/outcome, connection discovery,
catalog and ready/pending status records in `sophia-protocol::admin_files`, with
literal and malformed-input controls and the
[version 1 grammar](../../sophia-admin-files.md). Shared command, catalog and
outcome values live in `control_records`; the legacy codec re-exports the same
types. This is not a running administrative export, and
the legacy service remains active. No endpoint, default or installed session
has changed. t254 remains open.

Next model the proposed file lifecycle, then connect the shared-core export to
the existing ControlTicket claim/settlement owner. Preserve HostDomain checks
at attach and dispatch. The draft specifies snapshot reads, complete offset-zero
writes, bounded pending/outcome custody and no automatic replay. Model
cancellation/replay and terminal custody before changing those owners; a line
parser does not prove their temporal invariants.

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

## Discovery and shared records (October 14)

Built on `6fc80b1dc`. Moving the passive records preserves their numeric values,
validation and legacy imports; those imports resolve to the shared types. Revision-specific
welcome and envelope records remain in the old codec. No transport calls another
transport's encoder, and there is still only one command executor.

The new discovery codec supplies a session/connection identity and bounded
deadlines, a strictly sorted catalog, and distinct ready, pending and outcome
records. Names with spaces retain their counted byte length. The catalog shares
the current limit of 256 policy actions and three recognized Session operations;
it cannot advertise an arbitrary executable or a new Session command. Inputs
are bounded before allocation. Literal vectors, malformed records and a full
259-command catalog exercise these rules.

The [proposed lifecycle](../../sophia-admin-files.md#proposed-file-lifecycle)
keeps status reads non-consuming and makes a newer accepted invocation
acknowledge the previous outcome. Complete control lines avoid a second partial
write accumulator. This remains a proposal pending a temporal model and runtime
tests, especially flush versus dispatch, capacity refusal, retained snapshots,
indeterminate outcomes and restart. No lifecycle claim follows from codec tests.

## Validation and remaining work

The three focused protocol tests pass. They cover literal bytes, invalid
records, exact size boundaries and every outcome, including Indeterminate.
The device-hidden `cargo xtask check` passed (exit 0), including workspace tests
and strict Clippy. Hardware-dependent proofs were skipped. The log is
`~/.local/state/sophia/development-evidence/t252-owner-review-20261014/check-device-hidden-private-runtime.log`;
the [t252 investigation](lvlbjomq-qualify-the-shell-file-path-against-current-owners-and-independent-clients.md)
records the validation environment and earlier attempts. `git diff --check`
passed for the first slice. The discovery slice's focused run passes all 13
tests across `admin_discovery`, `admin_files` and the unchanged `control_wire`
golden-frame suite. Its full device-hidden `cargo xtask check` passed (exit 0),
including workspace tests, strict Clippy and script checks. Hardware-dependent
proofs were skipped. The log is
`~/.local/state/sophia/development-evidence/t254-admin-discovery-20261014/check.log`.
The adjacent `source.sha256` identifies the tested source files; all hashes
matched after the gate. Whitespace checks passed, and the notebook's broken-link
report named no changed note (it still reports 25 other notes).
Runtime export,
catalog, independent client, CLI migration, performance and rollback gates
remain; no command service migration is claimed.

## Connections

- [t254 scope](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t254--migrate-administrative-commands).
- [File grammar](../../sophia-admin-files.md).
