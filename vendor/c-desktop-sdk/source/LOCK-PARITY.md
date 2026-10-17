# Experimental lock SDK parity

Revision 8, 2026-10-16. The pinned lock file contract remains draft.
C reference: `86b3e89` plus the progress-query/peer change in this series;
`src/sophia_lock_client.h` and `src/lock_files/*.c`.
Rust `sophia-lock-client` integrates the experimental session and now has paired
production-export and idle protected-revocation evidence. Release qualification
and the remaining negative-case audit are open.

| Capability | Rust status / remaining work |
| --- | --- |
| Candidate preparation | All seven candidate kinds; epoch/submission envelope, typed body validation, fixed 128-byte owned storage. Three tests cover literal bytes, caller mutation, full-size presentation and invalid shapes/offers. |
| Direct/attached startup | Nonblocking Connecting from a socket or sole quiescent admitted root, with exact API epoch, matching Limits and four fixed streams. Transfers the ready Session once. |
| Negotiation | Local offer checks match C; exact grants, explicit EAGAIN retry and refusal ACK before terminal state. Server decides unsupported revision and semantic chord refusal. |
| Submission and custody | One immutable submission, explicit retry, cumulative Submitted ACK barrier and latest-result inspection. Clunk/walk after settlement; reopen after ACK. |
| Journal | Fixed 1024-byte buffer, retained ordered heads, internal controls, character-free Entry and granted Chord validation. Decode stops at a held head, including controls behind it. |
| Lock object | Exact Qid/epoch fetch through EOF and clunk, bound to the announced generation. Newer Qid supersedes the announcement, consumed internally without reading. Verified object transfers without cloning. |
| Upload | Begin/admission, slot open, owned chunks, end/cancel, status/refusal and slot-error recovery. Retire remains an ordinary submission. |
| Upload window | Both support 1–8 writes with five control slots reserved and drain before cancellation. Pipelined short writes cancel; single-write mode can continue. Socket tests verify control progress. |
| Event loop | Rust uses caller-monotonic time and one byte/syscall budget across at most 32 rounds. FD, readiness, obligations and startup deadline are exposed. C leaves all clocks to its caller; both require explicit retry. |
| Protected/production gates | Both candidate SDKs pass the same supplied-admission upload/pacing/presentation exchange and protected idle-revocation fixture. Raw reauthentication remains a separate control, not SDK session evidence. |

Prepared Rust candidates own no heap storage and borrow no caller memory. Body
encoding currently uses one temporary Vec; writing the fixed envelope directly
avoids a second allocation. Preparation is not allocation-free, and no native
CPU improvement is claimed. C already encodes into fixed caller-owned storage.
Resource and allocation validity remain server decisions; encoding grants no
custody, presentation, credential access or unlock authority.

The C client accepts Candidate, FrameDemand and ResourceRetire publicly; its
upload helper owns Begin/End/Cancel, and startup owns Negotiate. The Rust
session enforces those lifecycle boundaries rather than exposing
unrestricted raw admission through the preparation enum. C’s independent codec
and client suites pass; Rust’s three preparation tests and strict Clippy pass.

Seven Rust socket startup tests cover fragmented reads, exact decimal API epoch
(no zero, leading zero, overflow or suffix), Limits epoch matching, EOF/clunk,
control iounits, quiescent admitted roots, clock regression and unsent expiry.
Constructors queue without I/O. Startup keeps the transaction fid open read/write
and retains its iounit; submit and ACK are write-only. Rust currently requires
six requests, sixteen fids, msize 4096–65536 and two-frame input capacity.
Six requests support the default single-write mode in both SDKs. Larger upload
windows require their width plus five request slots.
Rust adds a caller-supplied startup deadline; C leaves that timer to its caller.
Four Rust submission tests cover both custody/reply orders, identical explicit
retries, EALREADY after confirmed custody, refusal, disconnect, invalid controls,
short writes and the ACK barrier before reopening the transaction. A contradictory
EAGAIN after Submitted now fails closed in both implementations, retaining custody;
C has an independent socket regression. Never replay a confirmed submission.
The Rust lane borrows bounded write slices from fixed storage and adds no clocks
or polling. Its body preparation still allocates temporarily. No measured runtime
gain is claimed. Fourteen Rust lock-client tests and strict Clippy pass.
The integrated Session now owns these components and their terminal handling.

Six journal tests cover fragmented records, retained heads, all event kinds,
grants/refusal, malformed prefixes, publication barriers and bounded compaction.
Four fetch tests cover fragmented reads, EOF/clunk before exposure, newer Qids
at walk or open, older Qids, bad epoch, premature EOF, trailing bytes, oversized
replies and remote errors. C independently covers ordered Entry/Chord delivery,
supersession, refusal ACKs and invalid grants; its fetch regressions now also
reject bad epoch, trailing bytes, premature EOF and an older Qid.

Rust journal assembly and object byte storage use fixed 1024-byte arrays, matching
C's bounds. The journal advances a cursor and compacts only when an append needs
tail space; C moves unread bytes when consuming a head. This is an internal
implementation difference, not a measured performance result. Object decoding
still allocates its bounded allocation list; the verified object transfers by
ownership without cloning. Neither component adds timers, retries or polling.
The 24 preparation/startup/submission/journal/fetch tests and strict Clippy pass. These component tests do
not establish integrated session or production-export parity.

Seven Rust upload tests cover reversed replies with refill while the first reply
is held, explicit cancellation, pipelined short writes and remote errors, terminal
resource status, disconnect, single-write continuation, multiple chunks, reserved
control capacity, failed admission and malformed replies. C independently covers
these behaviors in its socket-backed upload pipeline suite, including control
traffic and immediate poll interest after cancellation drains. A Rust socket test
uses one-byte budgets to prove POLLOUT exposes Cancel as soon as the last held
reply drains. C also independently tests negotiation retry with six requests.

Six Rust session tests cover both startup paths, explicit negotiation and later
submission retries, retained objects, event/ACK progress with eight held writes,
normal End, cancellation, short writes, terminal resource status, short/error/stale
ACKs, refusal ACK, uncertain disconnect and confirmed custody on close. Clock
regression, zero budgets and startup/upload-owned candidate restrictions are
checked. All 37 Rust lock-client tests, strict Clippy and C's lock suite pass.
These are scripted-peer gates; the paired production results below have separate
scope and do not turn every scripted fault into production evidence.

Rust accepts ownership of a Vec without copying its payload; failed chunk admission
returns that same Vec. After all issued writes settle, take_chunk returns the
buffer for reuse. A new chunk may replace completed storage the caller did not
recover. C borrows caller storage instead. Both retain bytes while writes are
outstanding and bound chunk length by the resource's remaining declared bytes.
Rust's wire transport may still copy write payloads; this is not an end-to-end
zero-copy claim. Slot errors end or cancel the upload, not the connection;
malformed replies terminate the session. New uploads remain possible after
refusal or slot cleanup. No runtime performance gain has been measured.

Sophia's `lock_client_c_sdk` test now runs pinned C, candidate C and external
Rust peers through the same production export. All three pass: one requested
chord, exact lock object, a two-chunk 2x2 upload, demand/permit, candidate outcome,
Entry/Chord events and republished Unlocking state. The harness supplies admission,
lock state, permits and presentation outcome; it does not prove Engine rendering
or physical locking. Candidate C is compiled from an explicit source path without
changing Sophia's pins; Rust is an independently built executable.

Both candidate SDKs also pass `external_sdk_lock_authenticates_and_observes_revocation`
under Sophia's native-session launch-authentication suite. Real Factotum,
bubblewrap and the production lock worker authenticate one admitted root; the
client fetches/ACKs the object, then observes grant revocation and retains its
confirmed negotiation custody. One connection/disconnection and zero remaining
namespace admissions are required. Lock state and the private channel class are
supplied. This covers idle revocation, not SDK reauthentication or an upload in
flight. The existing raw authenticate/reauthenticate/revoke control also passes.

C now exposes `sophia_lc_obligations`, matching Rust's passive consumed/acked,
event/object/upload-pending, outstanding upload writes and ACK/retry waits.
Terminal queries retain watermarks and clear pending flags. Tests cover invalid
arguments, explicit retry, held/drained writes, disconnect and production ACKs.
C leaves deadline tracking to its caller. For EOF, C reports FAILED with the
service error; Rust distinguishes Closed. Both terminate without reconnecting
and preserve the latest custody result.
Both peers now also pass protected unfinished-upload revocation. A four-byte
prefix of a 16-byte resource is written and acknowledged. The SDK queues its
remaining twelve bytes with a zero I/O budget and reports one pending write;
the parent revokes launch authority before allowing the peer to resume. No
resource or candidate may reach Session. Both clients terminate, preserve
ResourceBegin custody, clear pending work and refuse End/Cancel. Rust returns
the same owned tail buffer after termination. This is a queued-tail schedule,
not a claim that revocation interrupted a partially transmitted wire payload.
The existing raw in-place-fragment test owns that separate boundary. Fresh
authorized replacement/recovery and SDK reauthentication still need audit.
