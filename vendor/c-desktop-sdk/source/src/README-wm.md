# WM file session

`sophia_wm_session.h` uses the generic 9P2000.L client and the pinned WM file
API-1 codecs. Link `libsophia-desktop.a` and `libsophia-9p.a`; the optional IPC
archive is not used. There is no discovery, fallback or automatic reconnect.

## Ownership and progress

The application obtains its admitted fd and allocates the opaque state with
`sophia_ws_state_bytes()`, using malloc-compatible alignment. A disjoint region
of `sophia_ws_storage_bytes(msize)` backs the session. Both must remain valid
until close. The session borrows the fd and never changes its flags or closes
it. End the session, close the fd, then release the two regions.

The session allocates nothing. Storage is two 1 MiB record buffers, a 64-record
event buffer (272 bytes per maximum event), and the generic pipeline's 17 msize
buffers. It allows eight ordinary 9P requests, sixteen fids and sixty-four
remembered ticket outcomes. The separate state size is reported by the API.
The supported msize range is 4096 through 65536 bytes.

Drive `sophia_ws_dispatch` from a single thread with monotonic milliseconds and
a byte budget. Poll using `poll_fd`, `poll_events` and `timeout`; a timeout of
-1 means no local timer. A negative dispatch result is terminal and latches.
The caller owns clock reads and the event loop. Idle event reads have no timer.

Bootstrap reads the complete `api` and `limits` files, negotiates capabilities,
and acknowledges transport events. READY means capability negotiation only.
Profile preparation/activation, exact policy correlations, checkpointing and
semantic settlement remain application and Session decisions.

## Events and acknowledgements

`event` returns one borrowed, stable head event. Apply its meaning before
`consume`. Events stay in order. The library validates buffered events behind
that head to observe transport custody, but does not apply their policy effects.
The first malformed envelope or typed body terminates validation; bytes after
that fault cannot establish custody.

Negotiated and Submitted are handled internally when they reach the queue head.
Cumulative acknowledgements follow consumed records automatically during
dispatch. An application-held event therefore holds back an ACK and the next
candidate transaction. Consume and keep dispatching promptly. An ACK releases
transport retention only; it does not commit a policy or settle presentation.

## Candidates and custody

`submit` copies one complete candidate into the session's immutable buffer.
BUSY leaves both the queue and the output ticket unchanged. The caller may retry
after dispatch clears the previous candidate's local cleanup. Bootstrap uses
the first submission ID; application tickets and domain transactions are
separate namespaces.

Each candidate has an absolute caller-supplied response deadline. The first
queued transaction write also starts the fixed Limits assembly bound. Neither
later writes nor retries extend these deadlines. A new candidate's transaction
walk waits for the reply to the ACK covering the preceding Submitted.

Ticket meanings:

| Outcome | Evidence |
| --- | --- |
| ADMITTED_LOCAL | Copied locally; no submit is currently uncertain |
| ISSUED | Submit queued; custody is not yet known |
| SUBMITTED | Valid, correlated Submitted event observed |
| REFUSED | Definitive submit refusal before observed custody |
| DROPPED_UNSENT | Terminal connection while still held locally |
| UNKNOWN_DISCONNECTED | Terminal connection after an uncertain submit |
| UNAVAILABLE | Ticket absent or older than the remembered window |

Submitted and other settled outcomes survive later errors and close. A normal
submit Rwrite is a byte count, not custody. A batch's event replies are decoded
before judging its submit reply, in either arrival order. EALREADY without
observed Submitted fails closed with unknown custody. A contradictory refusal
after Submitted fails closed while preserving Submitted.

EAGAIN transfers nothing. Retry retains the exact bytes and ID, waits for a
later dispatch pass, and then requires consumed/ACK progress or exponential
backoff from 4 to 256 ms. The original deadline still applies. No retry crosses
an attach epoch. Configuration, Projection and SessionOperation share the
strictly increasing domain watermark; only Submitted advances it. Profile
completion IDs are echoed server identities outside that namespace.

## Snapshots

Start `snapshot` while the corresponding Cycle is the current head. The session
copies its expected identity, so the application may then consume the Cycle.
It reads into a separate bounded buffer, requires complete length and EOF, and
checks kind, attach epoch, snapshot transaction and scene generation. It clunks
the pin before exposing the result. An EAGAIN open uses bounded backoff within
the supplied deadline. No partial snapshot is returned.

`snapshot_result` and its row bytes remain borrowed until `snapshot_release`
or close. There is one fetch/result at a time. The transport does not establish
scene truth or validate the application's policy against that scene.

## Evidence and remaining gates

`wm_session_test` uses a scripted socket peer with the real C session, generic
9P pipeline and WM codec. Its seventeen groups cover bootstrap, capability and
epoch rejection, partial transaction writes, sticky custody, both reply orders,
refusal continuation, paced retries, held events and ACK replies, malformed
event streams, short fixed writes, revocation, final Rread before EOF, snapshot
identity/EOF/pin release, the full retained journal and local deadlines.

The peer supplies outcomes and uses the same WM encoder; this is not an
independent encoder or a production-export proof. Literal codec vectors and the
pinned row corpus are separate tests. The production-export proof is Sophia's
generic C peer against the real WM file export (Sophia `c4e17899e`, bound to
this session at `b5be29db0`). Hagia's thin bindings and 449 product tests use it
(Hagia `b3d8496`). Together those gates support advertising `wm_files`.
No physical rendering, profile activation or live desktop claim follows from
these unit tests.
