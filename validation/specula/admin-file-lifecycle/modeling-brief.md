# Administrative file lifecycle

Source: Sophia `5cea0350b`. Category B: one transport worker and the Session
command owner, joined through atomic ticket state. The file adapter is proposed;
its actions are design rules, not claims about an existing export.

## 1. Scope

Model request admission, the claim/recheck handshake, queued cancellation,
timeout after dispatch, separate outcome publication and settlement, result
retention, and a successor connection. Preserve `ControlTicket` ownership.
The bounded model has two connection identities, two request IDs per connection,
two catalog generations and one execution slot, making retained old work compete
with new work. Normal reactive steps are unbounded. Identity domains are finite
test parameters, not claims about production's u64 ranges or sixteen slots.

## 2. Scenarios

1. A queued request is cancelled while admission is being rechecked. A failed
   compare-exchange must not revive it or permit dispatch.
2. A dispatched request times out. Publishing Indeterminate, reading that result,
   admitting a newer request, or losing the connection must not free unsettled
   execution capacity.
3. Duplicate writes, read/flush/clunk and a successor connection must not reset
   the request watermark, consume an outcome, or let an old completion replace
   the successor's status.
4. Catalog replacement or failed admission prevents a queued request from
   dispatching. Admission is proved at the recheck handoff, not continuously
   against arbitrary external process changes after that check.

## 3. Boundaries and granularity

`control.rs:41–104` owns claim, cancellation, first-outcome CAS and settlement.
`control/transport.rs:71–104` rechecks HostDomain and publishes outcomes;
`:343` releases capacity only after settlement. `live_session/control.rs:268–337`
checks generation and dispatches Session operations after claim.
Keep the claim request and worker approval separate. Keep result publication,
settlement and capacity reclamation separate. The model represents one successful
claim as one effect opportunity; it does not simulate the command's actual effect.

File admission, snapshot reads and acknowledgement follow the proposal in
`docs/sophia-admin-files.md:100–139`. Whole malformed writes and unavailable
capacity are no-ops in this model. Parsing and errno behavior need Rust tests.
The model does not claim to reproduce every atomic instruction or weak-memory
ordering: the ticket handshake is a protocol abstraction, not a Rust refinement.

## 4. Exclusions

No payload bytes, real HostDomain inspection, wakeup liveness, fid accounting,
wall-clock deadlines, process crash/restart, or runtime trace conformance.
A successor connection is not a service restart. The new service's identity
and old service teardown still require separate tests and a later model extension.
No liveness claim assumes commands always finish.

## Restart extension (source `bfd65d84f`)

Scenario 5 asks whether service teardown cancels old queued tickets, closes old
sockets and isolates late results from a newly bound service, even when the new
connection and request counters repeat. `Restart.tla` is a separate boundary
model: two service instances, one connection and request per instance, and the
existing claim/cancel/result/settlement steps. It does not replace the richer
per-service lifecycle model above. Graceful stop, worker close, outcome expiry,
join and new bind remain separate actions. Owner finish may interleave with them.

`ControlService::drop` (`control.rs:269–278`) stops and joins the worker before
removing its socket. The worker closes connections before expiring active
tickets (`control/files.rs:166–170`). Closing cancels queued work, while a
dispatched ticket remains an owner-held Arc. A new bind creates independent
ticket/channel state and a random session identity (`control.rs:127–232`).
`LiveControlState::start` creates the service once for Session's lifetime;
hot replacement beneath surviving command owners is not a supported recovery
path. A full process crash loses volatile outcomes. Neither recovery path
promises durable exactly-once execution across service instances, and clients
must not reconnect and repeat an uncertain command.

The extension checks fresh service identity, closed old transports, cancellation
of undispatched old tickets after join, and result attribution to the current
instance. Negative controls deliberately skip cancellation or cross the instance
boundary when projecting a result. Random identity collisions, OS cleanup after
SIGKILL, automatic restart supervision and whole-desktop recovery remain outside
this model; socket tests check service-instance teardown and late completion.

## 5. Safety properties

TypeOK; AtMostOnce (each identity is accepted and dispatched at most once);
CancellationWins (a cancelled queued ticket never dispatches);
ExecutionCustody (every unsettled dispatched ticket retains capacity);
BoundedCustody; ResultRetention (status tracks the latest accepted identity);
CurrentDispatch (dispatch uses the checked admission handoff and current catalog).

## 6. Questions and controls

### 6.1 Model-checkable

Does an early outcome release execution capacity? Can a replay replace a result?
Can a status read consume the result? Deliberate negative controls remove each
rule independently. They demonstrate sensitivity, not discovered production bugs.
One focused hunt merges all four scenarios because the small state space retains
their enabling interleavings and enables every scenario property.

### 6.2 Implementation checks

Before promotion, exercise the actual 9P core with an independent peer, real
HostDomain admission, partial reads, flush/clunk, malformed writes, backpressure
and queued/dispatched connection loss. Record explicit action-to-code mapping
and runtime traces when the export exists. A design trace cannot replace these.
