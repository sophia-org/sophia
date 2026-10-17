# Output API parity

Revision 9, 2026-10-16. Contract: API-1 output files in `spec/`.
C reference: `a88efe7`, `src/sophia_output_session.h`.
Rust has codecs, provisional fixed-file startup and an immutable submission
lane, ordered journal and topology fetcher joined by a developing session.

| C capability | Rust status | Evidence or remaining work |
| --- | --- | --- |
| File record codecs | `sophia-output-protocol` | Thirty extracted tests; schema and contract match C. |
| Direct startup | `Connecting::direct` | Nonblocking version, files and negotiation share one absolute deadline and caller budget. |
| Authenticated handoff | `Connecting::attached` | Exact sole root on a quiescent owned pipeline; no reset or attach. Full supplied socket proposal/retry tests; protected production gate remains. |
| API / Limits | Validated through EOF and clunk | Exact API line and 72-byte Limits object; retain epoch and resource bounds. |
| Role files | Opens events, submit and ACK | Refuses non-file Qids and control iounits smaller than 24/16 bytes. |
| Observe prerequisite | Required before opening | Unsupported revision requests survive startup for server negotiation/refusal. |
| Startup progress | Caller clock, deadline, interests and byte budget | Expiry sends nothing; clock regression is nonmutating. Constructors queue requests without socket I/O. |
| Negotiation and Refused state | Provisional journal | Checks grant subset and revision; Refused supplies an internal ACK target. Driver must settle its ACK before terminal handoff. |
| Proposal admission and custody | Provisional `Submission` | Configure/epoch checks, immutable bytes, 64-ticket history, same-ID retry, domain allocation and advertised assembly deadline. Nine driver tests plus socket proposals/retries through both startup paths. |
| Ordered events and ACKs | Provisional journal | Internal controls, retained heads, bounded fragments and advertised fragment deadline. Integrated cumulative ACK scheduling; terminal socket coverage remains. |
| Authoritative topology | Provisional `Fetch` | Checks published Qid on walk/open, connection and topology epochs, bounded reads, EOF and clunk. Socket test verifies delivery/ACK after EOF and clunk. |
| Close, revocation and recovery | Startup close and submission disconnect | Submission retains confirmed custody and distinguishes uncertain from unsent work. Session revocation/recovery tests remain. |
| Production export / protected launch | Paired supplied-admission exchange passes | Both SDKs negotiate, fetch exact topology, submit ValidateOnly and ACK the outcome against Sophia’s real output worker. Protected launch remains. |

C borrows its descriptor and caller storage; Rust owns the pipeline and lends
its descriptor for polling only. Both require exclusive control of the admitted
root. The developing Rust driver requires msize 4096–65536, eight request slots,
sixteen fids and input space for two negotiated frames plus scalar replies.

Output semantics are distinct from WM: explicit negotiation refusal, Observe
required, Configure granted by the server, publication-bound topology reads,
ACK progress and per-epoch domain replay capacity. Do not substitute a WM event
or snapshot policy for those rules. Codec success is not custody, topology
commit, presentation or API parity.

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
