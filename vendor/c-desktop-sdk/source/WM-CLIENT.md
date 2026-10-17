# WM client ownership and errors

This guide covers the developing API-1 clients. See [WM parity](WM-PARITY.md)
for operation mappings and the remaining qualification gates.

## Connection ownership

C borrows its nonblocking descriptor, opaque state allocation and separate
storage. Keep those allocations alive and unmoved while the session uses them.
After close, retain the state while inspecting custody; then free the storage
and state and close the descriptor. For attached startup, the session exclusively
borrows the admitted wire, its storage and sole root. Do not drive or reset that
wire concurrently.

Rust owns its Unix stream or admitted pipeline/root. `Connecting` owns startup;
`take_session` transfers the ready session once. All role I/O belongs to that
owner, including event reads and ACKs. A borrowed descriptor is for polling,
not for reading or writing. Closing or dropping the Rust owner closes its
descriptor.

Neither direct startup function performs authentication. Use the authenticated
launch client and attached startup for protected admission. A failed authenticated
attempt must never fall back to a direct attach.

## Driving and borrowing

Supply monotonic milliseconds from one clock for the connection's lifetime.
Startup has one absolute deadline across version, fixed files and negotiation.
Candidate and snapshot deadlines are absolute too; retries never extend them.
Dispatch bounds I/O by the caller's byte budget and the driver's syscall bound.
Recompute descriptor interests and the next timer wake after every state change.
Service immediate work before sleeping, and wake for retries without waiting
for socket activity. A retained application event must be handled and consumed;
repeated dispatch does not consume it for the caller.

The event head is borrowed until consumption or close. Rust's borrow checker
prevents mutation while the reference is live. C callers must enforce that
lifetime themselves. Negotiated and Submitted controls are handled internally.
Application events remain ordered; their delivery does not apply policy.

Request a snapshot while its Cycle is the retained head. The client copies the
expected epoch, snapshot transaction and scene generation before consumption.
Only a complete, identity-checked object with EOF and successful clunk becomes
readable. One fetch or retained result is allowed at a time. Release invalidates
the borrowed result; close also hides and invalidates it.

## Candidates and custody

Submission success copies or encodes an immutable candidate and returns a local
ticket. The caller may then change or release its source values. Busy admission
spends no ticket. One candidate is active at a time; a confirmed ticket can still
have cleanup pending, so the next admission can remain busy briefly.

Ticket identity belongs to this connection. The last 64 custody results are
retained; older or unissued tickets are unavailable (C's Unavailable, Rust's
None). Domain transaction IDs are separate from tickets. Profile completions
echo the server's transaction; they do not allocate a new policy transaction.
A refused domain transaction does not advance the Submitted watermark.

| Custody | Meaning |
| --- | --- |
| Admitted local | The SDK owns the candidate; no submit request has been issued. |
| Issued | Submit has been queued; the server's custody is not yet known. |
| Submitted | A validated journal record established server custody. This is not semantic commit or presentation. |
| Refused | The server refused the candidate; the result retains its wire errno. |
| Dropped unsent | The connection ended before submit was issued. |
| Unknown disconnected | Submit was issued but custody was not established before the connection ended. Do not replay automatically. |

EAGAIN retries reuse the exact bytes and identity after a later dispatch pass,
then ACK/consumption progress or bounded backoff. The next transaction waits
for the previous Submitted ACK reply. EALREADY is accepted only with a matching
validated Submitted record. Contradictory errors cannot erase established
custody; they can still end the connection.

## Errors and recovery

A local argument, busy or clock error does not by itself imply disconnection.
Inspect the connection state and retained error. Rust exposes typed
`SessionError`; C returns status codes. Both expose the last remote errno
separately. That diagnostic may describe a recovered EAGAIN or refused
candidate, so nonzero remote errno alone is not a terminal-state test.

Closed, Stale and Failed are terminal. ESTALE from an event read or ACK makes
the connection Stale. Short ACKs and other fatal protocol errors make it Failed.
Terminal sessions hide events and snapshots, cease I/O and retain custody.
Calling close again preserves the terminal state and custody. Close performs
no wire I/O and is not a server acknowledgement or a cancellation guarantee.

Neither client reconnects, replays, restores a profile or settles an Engine
transaction. Reconnect through fresh admission and reconcile uncertain work
using the public contract. Policy phase, semantic correlations, configuration
outcomes, profile recovery and presentation remain the application's decisions.
