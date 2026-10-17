# Launch authentication parity

Operation map revision 1, 2026-10-17. Both SDKs implement the local launch
conversation in `spec/factotum-launch-authentication.md`, copied from Sophia
`c57cc30eedce72dfeacea6ced3d49df78b26da52`. Its SHA-256 is
`2c54fecf31ee7ec0634bb57caa49b508ea65642deaaabeae7ac0a9b474bb2719`.
This is authentication against an existing supervisor-provisioned capability;
claims never select a principal or grant permission.

| Operation or outcome | C | Rust | Ownership and evidence |
| --- | --- | --- | --- |
| Start without socket I/O | `sophia_la_init` | `Client::new` | C exclusively borrows two clients and their storage; Rust owns both streams. Both mark the relay CLOEXEC before I/O. |
| Version negotiation through temporary-fid cleanup | `sophia_la_service` | `Client::dispatch` | Caller monotonic milliseconds and byte budget cover the whole attempt. No root is published before cleanup. |
| Readiness and next timer | Client fds, `sophia_9p_wants_write`, `sophia_la_next_wake` | `role_fd`, `relay_fd`, `role_interest`, `relay_interest`, `next_wake` | Monitor both wires, including hangup/errors. Due local work must run before sleeping. |
| Completion and admitted root | `sophia_la_ready`, `sophia_la_root` | `ready`, `into_authenticated` | Only the role root remains; relay and afid temporaries have been clunked. C keeps its borrowed clients; Rust transfers them once. |
| Retryable Tauth EAGAIN | Internal ten-millisecond retry | Same | Same claims, original total deadline, no unauthenticated attach fallback. |
| Refusal, malformed reply, disconnect | Terminal result; `sophia_la_remote_error` | Latched `Error` | Role/verifier errors cannot publish a root. C's caller retires clients and closes descriptors; Rust shuts down owned transports. |
| Cancel or abandon | `sophia_la_cancel`, then caller cleanup | `cancel` or drop | No fallback or automatic reconnect. |
| Another approved endpoint | `sophia_la_next_endpoint` | `over_relay_at`; blocking `over_relay` | Exclusive relay reuse refuses pending traffic, live fids and partial input. Earlier role roots remain untouched. |
| Reauthenticate the same endpoint | Retire the old clients; reinitialize their storage on the same owned descriptors, then `sophia_la_init` | `Authenticated::reauthenticate_at`; blocking `reauthenticate` | Invalidate old role fids and prove the same claims anew; never reuse the prior verdict. Rust checks quiescence before reset. |
| Convenience loop | Application wrapper over service/readiness | `over`, `with_timeout`, `poll`, `wait_for_io` | Rust waits for negotiation in its convenience constructor. External-loop callers use `new` and `dispatch` throughout. |

## Independent checks

Rust's `crates/sophia-launch-client/tests/exchange.rs` and C's
`src/tests/launch_auth_test.c` exercise successful cleanup, malformed challenge
and status records, short writes, remote refusal, EAGAIN, cancellation and total
deadline expiry. Both now cover no constructor I/O, zero-byte dispatch, a full
exchange with one-byte budgets, the exact retry wake, backwards clocks, and
relay reuse refusal after a request has settled but left a fid or partial input.

Rust additionally drives reauthentication with caller time and one-byte
budgets; its two-endpoint test covers both convenience and external-loop entry.
The protected output peer uses the new Rust API and reauthenticates before
negotiating the output role. Both protected lock peers also reauthenticate
before negotiation; C explicitly retires its quiescent clients before resetting
their storage on the same descriptors. Paired lock replacement checks use a new
supervised process and capability, with a fresh connection epoch and no replay.
Production-export results and exact SDK pins belong
in Sophia's owning t333 note, separately from these independent scripted peers.

## Bounds and implementation differences

The total authentication deadline is at most five seconds, including version
negotiation. Rust accepts an explicit shorter deadline; C callers can cancel at
an earlier application deadline. Neither a retry nor handoff between internal
stages renews it. Do not mix Rust caller time with its convenience wall clock.

C's service budget applies separately to each wire: at most the supplied bytes
and 32 syscall attempts per direction per wire. Rust shares that allowance
across both wires. Both bound work; identical dispatch counts are not promised.
C negotiates versions sequentially; Rust may progress both in one pass.

C uses caller-owned storage and NUL-terminated claims. Rust uses bounded owned
buffers and borrowed byte-slice claims copied at admission. Allocation strategies
need not match. Neither SDK discovers an ambient credential or opens a role
endpoint on the caller's behalf. This map does not establish full t333 parity
or physical desktop acceptance.

