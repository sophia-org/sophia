# Output API parity

Revision 10, 2026-10-17. API-1 output files, pinned in `spec/`.
Source audit: C `8894d64cfaa2d66bb6a4f9380dbe1076e4fc5707` and Rust
`902a26e574f1c55d2e31599d104a1251dd4e408f`.
Both copies of `spec/sophia-output-files.md` have SHA-256
`50b6342fcf6398c32025d8329e87d0818d661c5c11be77b36a70fe93cdeea3cf`.

Both SDKs implement startup, negotiation, immutable proposals, ordered events,
ACKs and publication-bound topology reads. Independent peers have passed the
supplied-admission output worker and protected launch/idle-revocation fixtures.
Those checks do not prove physical display application, revocation during an
in-flight proposal or the complete t333 release gate.

In this table, C names have the `sophia_os_` prefix; Rust methods belong to
`Session` unless `Connecting` is named. C tests are in
`src/tests/output_session_test.c`; Rust tests are under
`crates/sophia-output-client/tests/`.

| Operation | C entry point | Rust entry point | Contract and evidence |
| --- | --- | --- | --- |
| Allocate and start | `state_bytes`, `storage_bytes`, `open_fd`, `open_attached` | `Connecting::direct`, `Connecting::attached`, `ready`, `take_session`; lower-level `Session::from_opened` | C borrows stationary caller storage and fd; Rust owns its pipeline/socket. Attached startup requires the exact sole root on a quiescent wire. Paired socket proposal tests exercise both startup paths. |
| Inspect negotiation | `state`, `epoch`, `capabilities`, `refusal`, `limits` | Same method names on `Connecting` and `Session` | Exact API line, 72-byte Limits, Observe prerequisite and granted capability subset. Refused waits for its final ACK. Rust `refused_session_waits_for_ack_and_preserves_custody` and C refusal controls. |
| Drive external loop | `poll_fd`, `poll_events`, `timeout`, `dispatch` | `fd`, `io_interest`, `wake_deadline`, `dispatch` on both facades | Caller-monotonic milliseconds, absolute startup deadline and bounded I/O. C rejects zero budget; Rust permits local progress without socket I/O. Rust connecting/session deadline and zero-budget tests; C bootstrap/deadline controls. |
| Inspect pending work and errors | `obligations`, `remote_error`, `state` | `obligations`, `remote_error`, `state`, `error`; `acked`, `consumed` on `Session` | ACK watermark advances only on its write reply. Rust also reports `topology_ready`; C can query `topology`. Paired short ACK, EINVAL and ESTALE socket tests. |
| Admit proposal | `submit`, `next_transaction` | `submit`, `next_transaction` | Copies immutable candidate bytes, requires Configure, returns a local ticket; domain identity belongs to the caller. Rust socket tests mutate caller input, use domain IDs 50 then 3 and check allocator 51; C independently checks immutable retry bytes and out-of-order domains. |
| Inspect custody | `outcome` | `custody` | Last 64 tickets: admitted, issued, submitted, refused with errno, dropped unsent or unknown after disconnect; unavailable/`None` outside retained history. Paired disconnect tests and Rust submission history tests. |
| Retry and release assembly | Automatic in `dispatch` | Automatic in `dispatch` | EAGAIN retains identical bytes and envelope ID. Backoff or ACK progress permits retry; original and assembly deadlines still bind it. Next assembly waits for Submitted ACK and transaction clunk. Paired retry socket tests and Rust submission tests. EALREADY is refusal, not WM's success exception. |
| Observe and consume events | `event`, `consume` | `event`, `consume` | Retained head until consumed; Negotiated, Refused and Submitted are internal controls. Public events are ObjectPublished and Outcome (Validated, Committed, Stale, Rejected, RolledBack and Failed). Paired literal outcome, held-head and fragmentation tests; Rust `journal.rs`. |
| Read topology | `topology` | `topology` | Borrow decoded session storage. Publication is hidden until exact Qid, connection/topology epochs, bounds, EOF and clunk are verified. Rust `topology.rs` and integrated session fetch test; C bootstrap/topology and protocol-fault controls. |
| End session | `close` | `close` (or drop owned client) | No reconnect, replay or rollback. Close preserves confirmed custody and prior terminal state. C caller closes its fd; Rust drops the owned socket. Paired disconnect and ACK-fault tests. Closing cannot cancel a proposal already held by the server. |
| Use production export | Independent C output peer | `output_conformance_peer` | Exact topology, ValidateOnly outcome and final ACK on Sophia's real output worker. Commands and limits below. |
| Authenticate and observe revocation | `output_authenticated_peer` | `output_authenticated_peer` | Real protected launch, exact admitted root, topology fetch/ACK and idle revocation with negotiation custody retained. This fixture does not revoke an in-flight proposal. |

Output requires msize 4096–65536, eight request slots and sixteen fids. Rust
also requires input capacity for two negotiated frames plus scalar replies.
Both clients exclusively control their role wire. Codec success proves neither
custody nor topology application. See [ownership and recovery](OUTPUT-CLIENT.md).

## Earlier implementation evidence

The entries below record the sequence in which coverage was added. Their test
counts, pins and statements about work still pending apply to those checkpoints;
the current API inventory is above. Full release qualification remains separate.

Submission tests cover both Submitted/reply orders, the ACK/clunk barrier,
retry progress and backoff, immutable negotiation, grant/epoch rejection and
caller/assembly deadlines. Output permits out-of-order domain IDs and treats
EALREADY as refusal, without WM’s success exception. Independent C socket tests
exercise both rules through direct and attached startup; C implementation is
unchanged. Rust submission tests supply matched replies directly. They do not
yet prove socket dispatch, negotiation Refused handling or replay exhaustion.

Seven Rust journal tests use literal wire records: grant/refusal checks, all six
outcomes, held publication barriers, custody validation behind a held head,
malformed-record stop, fragmented input/deadlines and bounded retention. A
publication remains hidden until the object driver confirms its sequence, Qid
and topology epoch. These journal tests use a callback; they are not
proof of a full object read. C independently checks all six outcomes through
both constructors, preserving held heads and ACK order. Its existing fragmented
topology test remains the fetch counterpart. C’s full suite and Rust’s 23
output-client tests and strict Clippy pass. These are component tests; integrated evidence follows below.

The provisional topology fetcher has five matched-reply tests: fragmented
reads, exact identity, EOF/clunk visibility, withheld walk/open retries, fixed
deadline, length bounds and terminal clunk refusal. A withheld open releases
its fid before walking again. C’s existing `bootstrap_and_topology` and
`protocol_faults` socket tests independently cover fragmentation, withheld open
and wrong-publication Qid, through both startup paths. Rust has 28 passing
output-client tests and strict Clippy; C’s full suite passes. The fetcher still
needs the integrated socket driver and production-export qualification.

`Session::from_opened` now joins these components on one owned pipeline. Three
independent socket tests cover negotiation and topology delivery through EOF/
clunk, Refused after its final ACK, and zero-budget/expired startup without
sending queued negotiation. Caller time drives all retries; one shared I/O
budget bounds dispatch. The verified topology transfers ownership into Session
without a deep copy. This avoids copying its vectors and labels; native CPU
savings are unmeasured. C already stores its decoded topology in session-owned
fixed storage and needs no matching ownership change. Its existing bootstrap
and refusal tests are the independent counterparts. Rust’s 31 output-client
tests and strict Clippy pass. Terminal/recovery and production gates remain; proposal and connecting
coverage follows below.
Earlier component-only limits above describe those individual tests.

The nonblocking `Connecting` facade now covers direct and admitted-root startup,
with three tests for shared deadlines, errno retention and Observe validation.
The integrated proposal test runs both paths, with and without EAGAIN, and
checks unchanged candidate/control bytes, caller mutation, domain IDs 50 then 3,
allocator 51, and transaction clunk/Submitted ACK barriers. Retry cases use the
Connecting facade through one-time Session transfer. No extra clock or readiness
syscall was added. All 35 output-client tests and strict Clippy pass; C’s full
suite passes with its existing independent retry/domain/startup controls.
Terminal socket faults and paired production exports remain required.

Independent C and Rust socket tests now cover short ACK, ACK EINVAL/ESTALE,
and disconnect before/after proposal custody through both startup paths.
Terminal state and confirmed/uncertain custody survive explicit close. Rust’s
37 output-client tests and strict Clippy, and C’s full suite, pass.
[Output ownership and recovery](OUTPUT-CLIENT.md) documents caller obligations.
Protected grant revocation and paired production exports remain unqualified.

The independent Rust `output_conformance_peer` and existing C peer now pass
the same production output export/worker exchange in Sophia. The parent admits
the exact peer PID; topology and the Validated outcome are supplied by the
fixture. Both clients settle the final ACK. This is not Engine validation,
physical output application or protected launch authentication. The normal
output export suite passes 15 tests; the external Rust case is explicitly
ignored unless its executable is supplied, and passes when invoked below.

```sh
SOPHIA_OUTPUT_RUST_SDK_PEER=/absolute/path/to/output_conformance_peer \
  cargo-slot sophia cargo test -p sophia-runtime --test output_file_export \
  sdk_session_against_the_output_file_worker -- --include-ignored --nocapture
```

Build the Rust peer with `cargo-slot sophia-desktop-sdk-rs cargo build
-p sophia-output-client --example output_conformance_peer`. The test uses
Sophia’s pinned C SDK; C output implementation remains unchanged in this series.
Verified production pins are unchanged. Protected launch and release-wide
qualification remain open.

Both independent `output_authenticated_peer` programs pass Sophia’s protected
output fixture: real process protection and Factotum admission, exact admitted
root handoff, topology fetch/ACK, then idle grant revocation with confirmed
negotiation custody retained. The namespace admission count returns to zero;
the existing raw reauthentication/revocation control also passes. The SDK peers
do not reauthenticate or revoke an in-flight proposal. C’s full suite and Rust
all-target output Clippy pass. Build C with `make build/output_authenticated_peer`
and Rust with `cargo-slot sophia-desktop-sdk-rs cargo build
-p sophia-output-client --example output_authenticated_peer`. Run each binary:

```sh
SOPHIA_OUTPUT_AUTH_PEER=/absolute/path/to/output_authenticated_peer \
  cargo-slot sophia cargo test -p sophia-session --features native-session \
  --test launch_authentication external_sdk_output_authenticates_and_observes_revocation \
  -- --include-ignored --nocapture
```

`native-session` is required: without it, zero tests run and provide no evidence.
No production pin or installed desktop changed. Remaining release-wide parity
work includes the experimental lock client, administration and broker/portal
audit, foreign-language smoke and publication/adoption.
