# Metadata broker SDK parity

Revision 1, 2026-10-17. This maps file API 1 and broker semantic revision 2
against Rust `5683ae0` and C `6e75154`. Record authority is the pinned
`spec/sophia-broker-files.md` and KDL; their original implementation-status
paragraphs are historical. Sophia owns admission, publication and atomic effects.

## Session operations

C names use the `sophia_br_` prefix unless otherwise shown. Rust names belong
to `session::Session`. The public C structures expose stored observations;
applications must not modify their internal state.

| Capability | C | Rust | Ownership and evidence |
| --- | --- | --- | --- |
| Discovery | `sophia_bo_init`, `dispatch`, `ready` | `startup::Opening::from_attached`, `dispatch`, `ready`, `into_opened` | One sole quiescent admitted root. API and Limits reach EOF and clunk before handoff. Startup tests cover fragmented reads, invalid metadata, deadline and clunk refusal. |
| Negotiation | `init` | `new` | Supported range is submitted on the same connection; exact discovered limits must match. Session tests cover success, mismatched limits and refusal draining. |
| Readiness | `ready`, `transaction_ready` | Same names | Negotiated readiness differs from prior transaction cleanup. Neither means a descriptor was applied. |
| Service | `dispatch(now, budget)` | Same name | Caller-monotonic milliseconds and bounded work. C services its borrowed wire; Rust owns its pipeline. Neither sleeps internally. |
| Polling and timers | Borrowed `wire->fd`, `wants_write`, `deadline`; opening's stored deadline | `fd`, `io_interest`, `deadline` on opening/session | Recompute after progress. C callers must drain buffered replies before waiting; Rust exposes immediate readiness. The generic pipeline remains the sole I/O owner. |
| Retained journal | `event`, `consume`, `acknowledge` | Same names | Event labels borrow journal storage. Consumption is local; a successful ACK releases retention. Partial records and held heads are tested independently. |
| Pending request | `request` | `request` | A separate fixed copy preserves the label after journal consumption, until response receipt or failure. Control characters and UTF-8 survive the tested handoff. |
| Response | `respond` | `respond` | Encodes borrowed rows once into a fixed 448-byte candidate. One response contains one or two rows; bad second rows are refused before admission. |
| Custody | `transaction.submission.custody` | `custody` | Queued, Issued, WriteAccepted, Submitted, Refused, DroppedUnsent and Unknown remain distinct. A write reply alone is not Submitted. |
| Retry | `retry_ready`, `retry` | Same names | Readiness means received EAGAIN, not merely queued work. Explicit retry preserves candidate/control bytes, ID and deadline. No automatic retry or cross-epoch replay. |
| Errors | Operation return and stored `error`/`wire_error`; component errno | `error` and returned `session::Error` | Distinguish state, remote and transport failures. Socket tests cover stale epochs, wrong receipt IDs, short ACKs and loss after submit. |
| Close | `close`, then caller closes fd | `close` or owned-pipeline drop | Stop use of the old connection. Known custody remains queryable; unsent work is dropped and uncertain submission stays Unknown. Neither reconnects. |

Discovery itself neither authenticates nor attaches. The embedding process must
perform version/attach using the generic transport on its authorized endpoint,
then transfer exclusive use to the broker client. Normal broker admission is
the protected child plus exact kernel UID/PID, not a shell/WM launch claim.
C state is caller-owned and immovable while borrowed; Rust moves owned state.
The session requires five fids and three outstanding slots, with msize between
512 and 65536. Retry-ready queries perform no I/O or allocation.

## Records and component APIs

Both codecs cover all five requests: SurfaceAdmitted, CandidateReduced,
AttentionChanged, SurfaceRemoved and SetDisclosure. Response rows are
PublishRule, EmitDescriptor, RetireSurface, Rejected and NoChange. Negotiated,
Refused, Submitted and Request are the journal events. No codec result grants
authority or authorizes an effect; the Session owner validates the full set.

C `sophia_bf_decode_api/limits/event` and `encode_negotiate/response/submit/ack`
map to the same Rust protocol functions. Labels borrow input during decoding
or encoding. C preserves output on invalid input. Both preserve absent versus
present-empty labels and permit unsupported negotiation ranges to reach the
server's refusal path.

C `sophia_bj_*`, `sophia_bs_*`, `sophia_bt_*` and `sophia_bp_*` correspond to
Rust `Journal`, `Submission`, `transaction::Transaction` and `phase::Ordering`.
Their independent journal, submission, transaction and phase suites cover
fragmentation, fixed deadlines, receipt ordering, retry identity and cleanup.
They do not own socket I/O; callers supplying matched replies own tag routing.
The integrated sessions provide that routing and retain one pending request.
Standalone transactions clunk both handles. Integrated sessions retain submit
until connection close, but clunk each assembly only after its receipt ACK.

## Evidence and limits

Independent C/Rust socket peers cover negotiation plus two responses, a single
retained submit handle, immutable EAGAIN retry, malformed second rows, retained
labels, request timeout, terminal refusal and uncertain loss after submission.
Sources: C `src/tests/broker_*_test.c`; Rust broker client/protocol `tests/`.
The fourteen independent session schedules now include every request kind.
AttentionChanged has separate NoChange and EmitDescriptor cases, with literal
wire checks for the requested attention value. These are scripted semantic
cases, distinct from real-export qualification below.

Both peers passed Sophia's `external_sdk_broker_lifecycle` and
`external_sdk_protected_broker_lifecycle` with submit reuse. These exercise
negotiation, retirement and two-row disclosure through the real export;
the protected variant uses real supervisor evidence and Bubblewrap 0.13.0.
Sophia `28e5f3d59` additionally tests the production Rust-based broker across
all five requests, both lowering paths, and pre-custody EAGAIN followed by one
commit. That production-process test does not substitute for independent C
client coverage of each semantic case.

No dedicated broker foreign-language binding smoke has been run. The C SDK's
requester binding smoke establishes the shared native-library binding route,
not every broker declaration. Full desktop parity still requires the release
audit. Physical desktop acceptance, internal-copy accounting and broker
performance acceptance are separate; submit reuse alone does not close them.
