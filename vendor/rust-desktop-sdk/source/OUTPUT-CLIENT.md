# Output client ownership and recovery

The native C and Rust clients implement the same output file contract. The
server owns topology validation and application. A successful submit call means
local admission; Submitted means file custody. Only an Outcome reports the
server's result. A topology publication names a separate authoritative object.

| Concern | C | Rust |
| --- | --- | --- |
| Startup | `sophia_os_open_fd` or `sophia_os_open_attached` | `Connecting::direct` or `Connecting::attached`, then `take_session` once |
| Ownership | Borrows fd, wire and caller storage; keep them alive and stationary until close | Owns pipeline and socket; `fd` is borrowed for polling only |
| Protected launch | Continue on the exact authenticated root | Same; direct startup is never an authentication fallback |
| Progress | `sophia_os_dispatch`, poll events and timeout | `dispatch`, `io_interest` and `wake_deadline` |
| Time | Caller-monotonic milliseconds | Caller-monotonic milliseconds; dispatch does not read a clock |
| Events | Retained pointer until consume | Retained borrow until consume |
| Topology | Borrow session-owned decoded storage | Borrow session-owned snapshot; fetch transfers ownership without a deep copy |
| Custody | Query ticket with `sophia_os_outcome` | Query ticket with `custody` |
| Close | Ends the role driver; caller still owns fd/storage | Ends the role driver and drops its owned pipeline/socket |

One owner drives each wire. Never read, write or attach independently through a
borrowed polling fd. Recompute interests after state changes and service
immediate work before sleeping. Wake at the retry/deadline even without socket
activity. Rust shares one byte/syscall budget across bounded dispatch rounds;
zero bytes performs no socket I/O. C requires a nonzero dispatch budget.

Both clients retain immutable candidate bytes and the same ticket on EAGAIN.
Out-of-order domain transaction IDs are allowed; the server owns replay history.
Submitted ACK and transaction clunk must settle before opening the next
transaction. Output has no WM-style EALREADY success exception. Neither SDK
reuses a domain ID merely because an event was acknowledged.

An ObjectPublished event stays hidden until its exact Qid is read through EOF,
its connection/topology epochs are verified, and its handle is clunked. Consume
the retained event only after handling it. Consuming advances the local watermark;
the peer has acknowledged nothing until the ACK write succeeds. Borrowed topology
is an observation, not a continuing grant or guarantee that the physical state
has remained unchanged.

Refused negotiation is terminal only after its final ACK completes. Short ACKs
and other protocol contradictions fail closed. ESTALE remains distinguishable
from ordinary failure. Inspect state, remote errno and custody before releasing
the client; explicit close does not erase an earlier terminal result.

On disconnect, confirmed Submitted stays Submitted. An issued proposal without
confirmed custody becomes UnknownDisconnected; unsent work becomes DroppedUnsent.
Do not replay uncertain work automatically. Reauthenticate on a fresh connection,
observe current topology, and make a new policy decision with fresh identities.
Closing an SDK object is not cancellation of a proposal already held by the
server. Neither client reconnects or rolls back the desktop automatically.

Independent scripted C and Rust tests cover short ACK, EINVAL/ESTALE ACK refusal,
and disconnect before/after observed proposal custody through direct and attached
startup. These tests supply admission. Protected grant revocation and paired
production-export qualification remain separate gates under t333. No performance
improvement is claimed from these correctness tests.
