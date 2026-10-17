# Administrative client ownership and recovery

The developing C and Rust clients use standard 9P2000.L on a caller-selected,
already-connected HostDomain socket. The export decides admission. Neither SDK
discovers an ambient endpoint, impersonates a launch role, probes legacy IPC,
reconnects or replays commands automatically. Production qualification is still
open; see [the parity map](ADMIN-PARITY.md).

## Startup and event loop

Rust's `connecting::Connecting` owns the UnixStream and negotiates version,
attaches, reads info, opens ctl and fetches the catalog. Dispatch it with
monotonic milliseconds and a byte budget. Use its descriptor, I/O interest and
next wake; transfer the ready Session once with `take_session`.

C's `sophia_ad_connect_init` takes a freshly initialized 9P pipeline and
separate caller-allocated connecting/session storage. Dispatch with the same
clock and budget, inspect readiness and write interest, and take the session
pointer once. The caller owns the descriptor and pipeline storage throughout
and must close the descriptor after terminal failure. No descriptor close is
hidden in the C library.

One absolute startup deadline covers version, files and initial catalog.
Each dispatch spends at most the supplied bytes and the underlying pipeline's
syscall allowance per direction. Rust shares that allowance across up to 32
rounds; C services the pipeline once, then queues the next operation. A phase
transition does not replenish the budget. Zero bytes permits buffered processing
but no socket I/O. There are no internal sleeps.

After handoff, dispatch the session on descriptor readiness or its next timer
wake. Status snapshots are opened only for a pending command. A ready idle
session has no status timer; descriptor hangup/error still requires handling.
Use the same monotonic clock throughout. Clock regression is a caller error
that leaves state unchanged.

## Catalog and command lifetime

The initial catalog becomes visible only after complete validation, EOF and
clunk. Refresh is explicit and allowed only when no command or snapshot is
active. Commands require membership in the current catalog. Session results
and stale, denied or indeterminate outcomes invalidate it; refresh before
submitting another command.

Rust retains one buffer plus name offsets, allowing the session to move without
self-references. C retains borrowed names in its own buffer: do not move, copy
or reinitialize a live C session. Catalog views expire on refresh; result views
expire on the next admitted invocation. Keep C pipeline storage disjoint and
alive until the session is finished.

Each session admits one immutable command and assigns increasing nonzero request
IDs. IDs are not reused; exhaustion refuses admission. Encoding, catalog parsing
and command tracking use bounded storage. Rust reuses snapshot buffers; C uses
fixed caller storage. These choices do not establish allocation-free transport
or a measured CPU improvement.

## Custody and failure

- Queued means not yet admitted to the pipeline. Closing or expiring here drops
  the unsent command.
- Issued means the pipeline owns the write. Even if no reply arrives, execution
  is possible; loss, short replies and expiry leave the result uncertain.
- Accepted means the full write reply arrived. It does not establish success.
- Settled preserves the attributed outcome, including timed-out or indeterminate.
  Indeterminate is a reported outcome, not evidence that no effect occurred.
- Refused means a complete write error established nonacceptance.

The client checks connection, request and catalog identity and the owner's
legal success outcome. Pending status schedules another immutable snapshot one
millisecond later; progress cannot extend the original command deadline. Closing
or flushing does not undo dispatched effects. Confirmed results and refusals
survive close. Terminal failure prevents further mutation on that session.

Recovery requires an explicit new connection and fresh discovery. Never replay
an uncertain mutation merely because the connection changed. These APIs do not
provide durable exactly-once execution across service restarts.

## Evidence boundary

Independent socket peers cover startup, sequential commands, pending outcomes,
catalog reuse/refresh, refusal, connection loss and bad identity. Stalled and
unsupported version tests cover budgets and startup expiry. These checks do not
replace production owner/ticket, revocation, process-loss, foreign-language
binding and publication gates.
