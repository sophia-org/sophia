# Desktop SDK qualification

The t333 capability-parity audit passed on 2026-10-17 against Rust
`10ab5bb0b84c4ea58f39a4107326a20cfb95fcaa`, C
`f7873a8409814442bb62a82ce955f3f0a1fff066` and Sophia `c3c388955`.
This document records that source state; adding it changes documentation only.
It supersedes earlier statements that the release-wide audit is pending.
It does not turn historical checkpoint results into current qualification.

Both SDKs provide the supported shell, WM, output, lock, administration,
metadata-broker and portal recipient/requester capabilities. The versioned
operation maps describe their APIs, ownership, errors, custody, retries,
cancellation, revocation and explicit recovery. Naming, allocation strategies
and convenience wrappers differ; applications need no private wire fallback.
C remains independently usable through foreign-language bindings without Rust.

| Evidence | Result and scope |
| --- | --- |
| Independent SDK suites | Full Rust workspace and C `make check` pass. Strict Rust protocol Clippy and the standalone shell allocation probe pass. |
| Native C binding | Nim requester smoke passes empty, one-byte and 64-KiB uploads using public headers and the native static libraries. This is a thin binding check, not a full Nim client for every role. |
| Shell | Both independent peers pass metadata/content startup, retained publications, explicit fetching, recovery and ACK control against the real export. Admission is supplied. |
| WM | Both peers pass profile/configuration/snapshot exchange and protected authentication with idle revocation. |
| Output and lock | Both pass real export exchange and protected revocation. Lock also passes unfinished-upload retirement and explicit replacement with a fresh epoch. |
| Administration | Five production scenarios pass per SDK, including service/process loss, replacement without replay, stale catalogs and foreign-namespace denial. |
| Broker | Supplied and protected admission gates pass per SDK; scripted peers cover all five request kinds and immutable retries. |
| Portal | All 22 protected recipient scenarios and thirteen requester lifecycle cases pass per SDK. |
| Sophia | Full `cargo xtask check` passes, including imported SDK checks, all-features tests, Clippy, independent conformance and existing archive/pixel checks. |

The authoritative audit and final adoption revisions are in Sophia's
`docs/notes/plans/9cd1ie0x-publish-independent-sophia-client-libraries.md`.
Logs and executable hashes are preserved on crunch under
`~/.local/state/sophia/t333-sdk-release-c3c388955/`. They include two corrected
test-invocation errors: an accidentally selected admin child helper and a WM
run without the cargo-slot environment. Neither is counted as a passing gate;
the corrected SDK runs pass.

The lock contract remains experimental. A portal requester whose End was
refused can retry explicitly or close, but cannot cancel that End while keeping
the connection alive. Disconnected EndIssued means unknown execution, never
Completed or Revoked. No SDK automatically reconnects or replays uncertain work.
These limits remain part of the public contract, not hidden fallback behavior.

Qualification is for Linux x86_64. Physical desktop acceptance, other platforms,
engine optimization and the remaining transport cost gates are separate.
Zero-allocation query tests do not prove a faster session or zero allocations
throughout the SDK. See the role maps, Rust `PLATFORMS.md` and C `COVERAGE.md` for narrower limits.
