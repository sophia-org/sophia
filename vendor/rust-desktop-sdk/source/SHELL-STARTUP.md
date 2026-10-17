# Caller-driven shell startup

Rust `sophia_shell_client::Connecting` and C `sophia_ss_open_fd` /
`sophia_ss_open_attached` let an external event loop drive shell startup.
They use the pinned `spec/sophia-shell-files.md` contract. No new wire record
or server authority is introduced.

| Operation | C | Rust |
| --- | --- | --- |
| Connected socket | `sophia_ss_open_fd[_staging]` | `Connecting::direct` |
| Admitted sole root | `sophia_ss_open_attached[_staging]` | `Connecting::attached` |
| Bounded service | `sophia_ss_dispatch` | `Connecting::dispatch` |
| Descriptor/readiness | `sophia_ss_poll_fd/poll_events` | `fd/io_interest` |
| Retry timer | `sophia_ss_timeout` | `wake_deadline` |
| Successful handoff | same session reaches READY | `ready`, then one `take_connection` |
| Refusal | terminal state and `sophia_ss_refusal` | retained `error`, including `AdmissionRefused` |
| Local cancellation | `sophia_ss_close`, then caller closes fd | `close` or Drop closes owned transport |

Both require a connected socket or an exclusively held, quiescent admitted
root. Neither constructor authenticates the caller. Keep any authentication
relay alive for the role lifetime. C borrows the descriptor, wire and caller
storage; Rust takes ownership. Construction does no socket I/O.

Pass monotonic milliseconds and a byte budget to each dispatch. Rust shares
one budget across version negotiation and the file handshake, with at most
32 service rounds; C bounds its pass as documented in the header. Zero budget
does no socket I/O. Regressed time is rejected without changing state.
Rust has an explicit absolute startup deadline, checked before further I/O.
C callers enforce that deadline and close on expiry; the SDK reads no clock.
Rust's old blocking helpers remain available, with their existing timeout.

An EAGAIN submit retries the same epoch, submission ID and candidate after
backoff (4 ms, doubling to 256 ms). Rust acknowledges startup receipts
automatically; C callers drive `sophia_ss_ack` as well as dispatch. Limits
are exposed only after validation. Rust transfers its ready connection once;
later progress uses that connection's event-loop API. Neither API reconnects
or replays an uncertain submission.

## Evidence and limits

Rust's independent scripted peer checks all four profiles through direct and
supplied-root startup with one-byte dispatch budgets, fragmented reads, short
candidate writes and an immutable EAGAIN retry. Both Rust startup APIs reject
malformed custody order, record bounds and stale event/limits epochs; they
acknowledge and report explicit admission refusal. Separate controls check
constructor silence, zero budget, clock regression and deadline expiry.

The independent C and Rust `shell_startup_peer` executables each pass Sophia's
`shell_sdk_startup` test in metadata and content modes. This uses the real
export with supplied admission evidence and one-byte dispatch budgets.
It does not prove protected launch, physical presentation or performance.
Full t333 release acceptance and the other shell control differences remain
in [API parity](API-PARITY.md).
