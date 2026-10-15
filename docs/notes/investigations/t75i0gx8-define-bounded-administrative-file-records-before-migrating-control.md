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

The administrative export, bounded `AdminClient`, ordinary CLI and Session
transport selection now connect to the existing command owners. Qualification
uses `--control-transport=9p2000.L` for Session and
`sophia msg --transport 9p2000.L` for the CLI. Both still default to legacy
control; no installed session has changed. The [file contract](../../sophia-admin-files.md)
records admission, snapshot, deadline and no-replay rules. t254 remains open.

The [bounded lifecycle model](../../../validation/specula/admin-file-lifecycle/README.md)
and its new service-instance boundary extension pass. Their faulty variants
and false-settlement traces fail as expected. Runtime socket tests check
teardown, queued cancellation, fresh identity and late-result isolation; the
client preserves uncertainty after a lost invocation and never reconnects or
replays it. These are not durable exactly-once or whole-process crash proofs.

The generic supervised owner test passes over both transports, including policy
commit, WM replacement, stale-action refusal, reload settlement and logout.
Focused client/CLI and malformed-server controls pass. The full repository gate
passes with devices and display sockets hidden; hardware-only proofs remain
unproved in that environment.

The independent Go client now checks discovery, modes, malformed invocation,
commit and retained snapshots, replay, stale/unadvertised commands, revocation
and denied reattach. It uses the pinned third-party 9P client and no Sophia
record codec. A deliberately wrong owner settlement fails its outcome check.
The full repository gate passes for this addition with devices/display sockets
hidden; hardware-only proofs remain unproved here.

The paired diagnostic harness is being validated. No administrative numeric
performance budget has been declared, so its results cannot establish performance
acceptance. The workload below is fixed before measurement.

Next collect paired performance evidence, then prove live recovery/rollback and select the default through t254's
rollout gate. Include whole-process loss and the existing safe command/restart
smoke. Service hot replacement is not supported, and uncertain commands must
never be automatically repeated. The deferred output invocation remains excluded.

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

The [file lifecycle](../../sophia-admin-files.md#file-lifecycle)
keeps status reads non-consuming and makes a newer accepted invocation
acknowledge the previous outcome. Complete control lines avoid a second partial
write accumulator. The bounded temporal model now passes; runtime
tests below add export evidence. Service restart and owner integration remain.
No lifecycle claim follows from codec tests alone.

## Shared-core runtime export (October 14)

Built on `4e5b0c34b`. The new opt-in listener uses the same ticket queue, claim
handshake and settlement path as legacy control. It does not add an executor.
HostDomain admission remains separate from inspection and protected-role
authentication. Attach text cannot choose the connection identity; retained
fids and snapshot delivery recheck authority.

Catalog opens share one immutable snapshot per connection until its last fid
closes. Status opens retain their own snapshot; reading one does not consume a
result. A newer accepted invocation replaces the visible result, while malformed,
replayed and capacity-refused writes preserve it. A wire reset releases fids
without resetting the administrative watermark or cancelling accepted work.

The real ten-second deadline tests fill all sixteen execution slots. After
each result becomes indeterminate, further writes still fail for capacity and
the original results remain readable. Settling an owner ticket releases a slot;
a late completion cannot replace a newer request's status. Queued expiration
instead cancels the ticket and prevents a later claim. Connection loss and
revocation exercise the same existing cancellation path.

The core now exposes whether input or replies remain buffered so a role owner
can enforce a wire deadline without mistaking command execution for blocked
I/O. Administrative connections have a two-second initial-attach/wire limit,
a sixty-second idle limit and bounded fids, requests and reply storage. The
limits are in the file contract. The core tests cover partial input and blocked
output through its public Unix driver.

## Validation and remaining work

### Paired diagnostic workload (declared October 14)

Use the public legacy and file clients against their real services and one
synthetic policy owner. Both receive the same catalogs and commands. Test a
one-command catalog and a maximum-size 259-command catalog with 128-byte names.
For each catalog run legacy then files, followed by files then legacy. Each block
contains 100 fresh connection/discovery samples, twenty unmeasured warmups and
200 measured invocations at each of zero and two milliseconds of requested
owner delay, then a two-second idle interval. The owner sleeps on notifications
with a one-second safety wake; its injected two-millisecond sleep is subject to
scheduler delay and is not subtracted from latency.

Record nearest-rank p50/p95/p99/max and every completed raw sample, flushing
outside the timed region. Discovery includes connect and a complete catalog;
it excludes disconnect and a two-millisecond pause before the next connection.
Commands reuse a connection and cached catalog. Both clients must observe
Committed with the correct generation. Exactly 440 owner deliveries per block
and unchanged thread count after shutdown are required. Idle accounting includes
all harness threads, the client, service worker and synthetic owner. Report CPU
and context switches without borrowing the output role's acceptance threshold.

These are diagnostic workloads, not administrative acceptance budgets. They do
not measure native effects, a loaded desktop, whole-process recovery, aggregate
throughput or independent-client latency. The host quiet window is not externally
certified. Preserve failed runs and outliers; a timeout or missing final record
cannot complete the diagnostic. Ordinary workspace tests skip timing.

The Rust runner separates preparation from measurement:

```sh
cargo-slot sophia cargo xtask check admin-file-performance prepare --output=/absolute/new/private-directory
cargo-slot sophia cargo xtask check admin-file-performance measure --output=/absolute/new/private-directory
```

Preparation requires signed, clean source and uses the existing cargo-slot pool.
It records the release test binary's path and hash without copying it into
evidence. Measurement checks source/hash before and after, runs once per prepared
directory, hides devices/network/session sockets and records machine identity.
Finish all builds and other checks before measurement. No affinity or priority
change is made. Workspace tests passed. Clippy rejected a constant assertion in
the release-only guard; the established conditional-panic form fixes it and
focused strict Clippy passes. The final full gate and measurement remain pending.

### Independent administrative client (October 14)

Built on `e533fcddd`. `tools/9p-oracle/cmd/admin-oracle` uses the pinned
`github.com/hugelgupf/p9 v0.4.1` client and literal records from the published
contract. Rust starts the production file service and supplies catalog changes,
ticket completion and PID exclusion through a bounded side channel. That channel
carries phase names, not file records or expected wire values.

All eight named checks pass. Seven-byte reads split fields and counted names;
the client checks complete discovery and status records, forbidden file modes,
malformed and noncanonical invocations, one-pending-request capacity, immutable
pending snapshots, retained outcomes, replay refusal, stale generation and
unadvertised command outcomes. Revocation cancels an undispatched ticket,
invalidates an open catalog fid and prevents another attach from the excluded
peer. Rust verifies that refused commands never reach the owner.

A second run supplies Rejected where the fixture promised Committed. The client
fails specifically at its settlement check; a successful subprocess alone cannot
pass, because the harness also requires the exact complete named transcript.
The initial Go build needed two offset types corrected to the third-party API's
signed 64-bit arguments. The focused test then passed both positive and negative
runs. Go vet and formatting pass. The complete device-hidden `cargo xtask check`
passes, including this oracle, workspace tests, strict Clippy, layout and the
existing shell oracle that shares its offline build helper. Evidence and source
hashes for the final repository gate are under
`~/.local/state/sophia/development-evidence/t254-admin-oracle-20261015/`.

This is independent protocol interoperability against controlled owners. It does
not establish live Session recovery, real command effects, performance, foreign
namespace admission or durable results after a process crash. Existing owner and
admission tests remain separate evidence. No production code or default changed.

### Client and owner integration (October 14)

Built on `bfd65d84f`. `AdminClient` uses the pinned independent Rust SDK's
write-capable 9P pipeline, with bounded catalog/status reads, an absolute command
deadline and exact outcome attribution. It never probes another protocol after
a failure. The CLI retains command names and outcome/exit semantics; its
explicit transport selector reaches the same Session ticket owners. Session's
new startup selector changes transport only, never its HostDomain grant.

The restart extension passes its base and safety hunt with 352 distinct states,
702 generated and depth 23. Its eighteen-event design trace passes; two broken
variants and a false-settlement trace fail their intended checks. The runtime
tests retain an old owner ticket across service teardown and a fresh bind, reuse
connection/request numbers, and show that late completion cannot settle the
successor's request. This is service-instance isolation, not hot-reload support
or recovery of volatile results after a process crash.

Five recovery/client tests and three malformed-server tests pass. They include
wrong connection/request/generation, invalid success, oversized discovery,
server-controlled deadline expansion, short reads and short write replies.
The ordinary CLI tests pass over both transports. The generic owner test also
passes over both, including two WM replacements and refusal of a stale action.
The fixture initially inherited the operator's core configuration; it now uses
the established isolated configuration helper. A focused CLI build also exposed
the existing no-feature renderer dependency gap; CLI validation and the control
check script now enable its normal full feature set.

Strict CLI Clippy caught an oversized transport enum; the file-client variant
now stores a box. The first full gate passed workspace tests and Clippy, then
rejected the argument parser's new 1,001-line size. Transport parsing now lives
beside its selector type. The final complete `cargo xtask check` passes, including
workspace tests, strict Clippy, layout and repository checks. The final source
hashes are recorded in `source.sha256`; the passing log is `check-final.log`.

Evidence is retained under
`~/.local/state/sophia/development-evidence/t254-admin-client-20261015/`.
The owner test runs with devices/display sockets hidden and a private runtime
directory. Its generic C policy peer exercises the production owner, but it is
not an independent administrative client. No native desktop or monitor was used.

### Earlier foundation checks

The lifecycle model built on `5cea0350b` passed both final searches: 970,253
distinct states at depth 44, with no queued states remaining. It covers two
connections, two request identities per connection, two catalog generations
and one execution slot. The valid eleven-event design trace passed; the false
settlement trace could not complete. Replay, premature reclaim and consuming
status reads each violated their intended invariant. Results, exact commands,
limits and the source-to-action map are retained in the model directory.
No production Rust changed in this model slice, so no rebuild was needed.

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
The opt-in export adds eleven real-socket tests across `admin_file_service`
and `admin_file_deadlines`; all pass. The fourteen legacy service tests and
the shared-core suite also pass. One new negative test initially expected
`Rversion unknown` for a too-small message size; the established core contract
returns `EINVAL`, and the test now checks that refusal and the inability to
attach afterward.

The export slice's device-hidden `cargo xtask check` passed (exit 0), including
workspace tests, strict Clippy and tooling checks. The run hid display sockets
and devices and supplied a private runtime directory; hardware-dependent
proofs were skipped. Its log and matching source hashes are retained at
`~/.local/state/sophia/development-evidence/t254-admin-export-20261015/`.
Whitespace checks passed and the notebook was refreshed. Its broken-link report
flags these notes' model README because `validation/` is excluded from the
index; the linked file exists. All changed documentation links resolve.

Independent administrative-client, performance, live recovery/rollback and
default-selection gates remain; no live command service migration is claimed.

## Connections

- [t254 scope](../plans/jlftaw00-migrate-desktop-roles-to-a-daily-driver-9p-control-bus.md#t254--migrate-administrative-commands).
- [File grammar](../../sophia-admin-files.md).
