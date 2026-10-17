# WM API parity

Revision 6, 2026-10-16. Contract: the pinned API-1 WM files, schema and golden
records in `spec/`. C reference: `fbfcdd8`, `src/sophia_wm_session.h`;
Rust base: `622d9c0`, with the obligations and startup-wake changes in this
series. This maps implemented capabilities, not production qualification.

| C entry points | Rust counterpart | Ownership and evidence |
| --- | --- | --- |
| `state_bytes`, `storage_bytes` | Rust-owned allocations and `PipelineLimits` | C caller storage and borrowed fd remain distinct from Rust-owned memory/socket. Neither SDK needs the other language. |
| `open_fd` | `Connecting::direct` | One deadline across nonblocking version, file startup and negotiation. Supplied admission only; no authentication fallback. `tests/connecting.rs`, `tests/session.rs`. |
| `open_attached` | `Connecting::attached` | Exact quiescent sole root; no reset/reattach. `tests/startup.rs` tests valid handoff and rejects wrong roots, unfinished work and inadequate capacity. |
| `state`, `epoch`, `capabilities`, `limits` | Same-named `Session` and `Connecting` getters | Ready means capability negotiation, not profile activation. Startup metadata is absent until validated; remote errors and terminal state survive explicit close. |
| `poll_fd`, `poll_events`, `timeout` | `fd`, `io_interest`, `wake_deadline` | Borrowed descriptor is monitoring-only. Recompute after API calls; immediate work precedes sleep. Connecting includes negotiation retry wakes. |
| `dispatch` | `dispatch(now, byte_budget)` | Caller monotonic milliseconds; shared per-call I/O budget. Rust allows zero-byte buffered work, whereas C refuses zero budget. Neither performs socket I/O at zero budget. |
| `obligations` | `Session::obligations`, `Connecting::obligations` | Consumed/acked/submitted watermarks, deadlines, retries, event/snapshot readiness and ACK barrier. Terminal sessions retain watermarks and clear pending flags. |
| `remote_error` | `remote_error` | Wire errno retained separately from local/protocol failures. Independent socket tests cover ACK short/error/ESTALE and event-read revocation before/after custody; explicit close preserves results. |
| `event`, `consume` | `event`, `consume` | Borrowed stable head; controls remain internal; cumulative ACK never skips the head. `tests/journal.rs` and socket session tests. |
| `submit` | `submit(&Candidate, deadline)` | Seven application candidate kinds; copied immutable bytes, one candidate, local ticket. `tests/candidate.rs`, `tests/submission.rs`. |
| `outcome` | `custody` | Last 64 results; Submitted is custody, not commit. Refusal, unsent and uncertain disconnect remain distinct. C Unavailable maps to Rust None. |
| `next_transaction` | `next_transaction` | Independent monotonic domain IDs; externally assigned IDs must exceed the Submitted watermark. Refusal does not advance it. |
| `snapshot`, `snapshot_result`, `snapshot_release` | `request_snapshot`, `snapshot_result`, `release_snapshot` | One Cycle-bound fetch/result, cap/length/EOF/identity validation and clunk before exposure. Socket tests use a pinned golden row and literal file framing. |
| `close` | `close` / Drop | No replay or reconnect. Rust closes its owned descriptor; C leaves its borrowed descriptor to the caller. Custody remains queryable after explicit close. |

Candidate kinds are ProfilePrepared, ProfileActive, ProfileRolledBack,
Configuration, Dirty, Projection and SessionOperation. Event kinds are
ProfilePrepare, ProfileActivate, ProfileRollback, ConfigurationOutcome, Cycle,
ProjectionOutcome, SessionOperationOutcome and PresentationReceipt. Codec
coverage includes each. Independent socket tests now deliver all three profile
commands and all five configuration/projection/session-operation outcomes and
all three presentation outcomes, using literal wire bodies. They verify ordered
retained heads, ACK progression and that semantic outcomes do not close the
transport. These tests do not execute a policy rollback or correlate every
Cycle cause with a submitted policy response.

## Remaining qualification

The production export counterpart is Sophia's
`crates/sophia-session/tests/support/policy_file_c_sdk.rs` and its independent
C peer. It covers profile handoff, configuration, a multi-read snapshot and
policy exchange through the production adapter/worker. The independent Rust
`wm_conformance_peer` example now passes the same scenario. Sophia's explicit
ignored test requires its absolute executable path in `SOPHIA_WM_RUST_SDK_PEER`;
ordinary tests do not silently use an ambient SDK checkout. Both peers pass.
The C run uses Sophia's pinned `ba09265`; candidate C WM sources remain unchanged.
This exchange fixture supplies admission and outcomes, so it proves neither
protected launch authentication nor Engine commit/presentation.

The separate `policy_file_protected_authentication.rs` fixture now runs native
C and Rust peers through the production protected supervisor, Factotum and WM
worker. Each authenticates, transfers the exact admitted root into its WM client,
negotiates configuration capability, settles its negotiation ACK and observes
terminal closure/staleness after the parent revokes registration. Confirmed
negotiation custody remains readable. The raw peer remains an independent
control. This tests admission and idle revocation, not application candidates
in flight, automatic recovery or physical presentation.

Build C with `make build/wm_authenticated_peer`; build Rust with
`cargo-slot sophia-desktop-sdk-rs cargo build -p sophia-wm-client --example wm_authenticated_peer`.
In Sophia, set `SOPHIA_WM_AUTH_PEER` to either absolute binary path and run
`cargo-slot sophia cargo test -p sophia-session --features native-session --lib external_sdk_wm_authenticates_then_negotiates_on_the_same_worker_connection -- --include-ignored --nocapture`.

The [client guide](WM-CLIENT.md) describes ownership, errors and recovery.
Scripted terminal checks are C's `ack_faults_and_revocation_keep_custody`
and Rust's `ack_faults_and_revocation_preserve_exact_custody`; these do not
replace the separate protected-admission fixture above.
All nine profile-completion kind/outcome combinations now pass independent
socket checks. The peers inspect literal echo identity and outcome bytes,
confirm Submitted/ACK settlement and verify that no policy transaction ID was
spent. Rust's journal checks lifecycle and chord causes with and without their
negotiated capabilities; C's socket tests cover the same acceptance boundary.
The SDK transports rollback commands/completions; application rollback policy
is outside its authority.

The operation map now covers the WM entry points above. The release audit still
needs the paired artifacts and exact revisions across all roles, and must keep
the distinction between scripted, supplied-admission and protected tests.
Publication, verified pins and foreign-language binding checks
remain t333 gates. These requirements cannot be waived by marking both SDKs
unsupported.
