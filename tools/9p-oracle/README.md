# Independent 9P oracles

The existing command at this module's root tests Sophia's 9P core through the
pinned `github.com/hugelgupf/p9 v0.4.1` client and raw protocol scenarios.
Its scenarios and `sophia_9p_oracle schema=1` verdict remain unchanged.

## Shell file oracle (t252 B8)

`cmd/shell-oracle` judges the production shell file export with 96 named checks.
It uses only the Go standard library. The director owns the t252 task and
plan-note updates.

The approved extension plan is in [ROLE-COVERAGE.md](ROLE-COVERAGE.md). Phase 1
adds independent codecs for the 17 Catalog, Indicators, native launcher and
persistent catalog kinds, from KDL commits `ee5e7f809` and `ae60576f8`.
The unit tests use separate KDL-derived literals, truncations, text and revision
controls, maximum counts, and separate candidate byte/value validation. The live
verdict includes 54 base checks and 42 role checks; every named check and all of
its negative controls must execute successfully. See [ROLE-FIXTURES.md](ROLE-FIXTURES.md)
for the request/event sequences and scheduling barriers.

### Files

- `cmd/shell-oracle/main.go`: startup barrier and command entry point.
- `internal/shelloracle/wire.go`: independently written, bounded 9P2000.L
  request/reply handling, including pending reads and flush. Use only the Go
  standard library; leave the existing module pins unchanged.
- `internal/shelloracle/records*.go`: manually written envelope, object,
  negotiation, allocation, resource, candidate and action encoders/validators.
  Field offsets, bounds and conditional rules come from the KDL alone.
- `internal/shelloracle/session.go`, `runner.go` and `scenarios_*.go`: session
  state, named checks, main content flow, custody, malformed and flush controls.
- `internal/shelloracle/*_test.go`: independently authored literal vectors,
  malformed controls, bounded parsing, correlation and verdict validation.
- `crates/sophia-runtime/tests/shell_files_oracle.rs` and new
  `tests/support/shell_files_oracle/` files: offline build, process supervision,
  production export fixtures and scripted Session-side owner actions.
- `tools/check_shell_files_oracle.sh`: short nice-19 launcher with jobs 2,
  an isolated Cargo target and bounded Go build parallelism.
- This README: reference pins, fixture definitions, execution instructions,
  evidence limits and the later role-family scenarios.

No Sophia Rust/C codec is used to write or judge oracle bytes. No code is
generated from Sophia sources. Rust is confined to the server fixture and
test orchestration. No existing C1 command, shared xtask file or production
source changes belong to this slice.

### Reference inputs

- Base kinds in `protocol/sophia-shell-files-v1.kdl` at `d64bb5dd1`, including
  normative changes from `f64d670e0`, `bae4ec4a9` and `6bb0c8f2e`. Role layouts
  and value rules are pinned at `ee5e7f809` and `ae60576f8`.
- `docs/sophia-9p-profile.md`, blob
  `de101e3daed32fd7dad45d958d6f76d2e179af43`.
- `docs/references/diod-9p2000L-protocol.md`, blob
  `48d63c804aa9a8686094e0bfccf317f1a658b828`, upstream `de51d1ee1bd5`.
- `docs/sophia-shell-files.md` at `43e4530b3`, including `5f79a2784` normative
  negotiation/pacing outcomes, for node, custody, retention, pin, upload and
  revocation lifecycle. Record layouts and body rules still come only from KDL.
  Role lifecycle and negative outcomes use `ae60576f8`.

### Named checks: expected total 96

Every check must execute exactly once. The harness requires the exact name set,
96 checks, zero failures, successful process exit and this final line:

```text
sophia_shell_files_oracle schema=1 status=pass checks=96 failed=0
```

Missing, duplicate or unexpected check names fail the harness. Setup failure
must produce a failure verdict, never a smaller successful run. Every received
event and object passes the independent KDL validator before a scenario uses it;
body validation is also exercised by Go unit tests beyond these 96 live checks.

| IDs | Group | Checks, in order |
| --- | --- | --- |
| 1–5 | Negotiation | strict api discovery and accepted selection/epoch/capabilities; policy refusal with exact reason/bits plus unservable offer closure without Refused; second Negotiate refused; allocation before negotiation EACCES; Candidate before negotiation EACCES |
| 6–13 | Objects | valid Limits; valid Outputs; publication generation announcement; opened qid/getattr agreement; second pin EBUSY; old pin immutable through republish; fresh qid; fresh generation after reopen |
| 14–16 | Allocation | granted request correlation; rejected request correlation; granted geometry agrees with the fixture's declared output/allocation |
| 17–26 | Upload | admitted identity/length; split append cursor; msize-bounded writes; short write at canonical boundary; End accepted; explicit Cancel; writer-clunk cancellation; ended fid ESTALE; cancelled fid ESTALE; old fid clunk cannot cancel its successor |
| 27–33 | Candidates | demand/permit correlation; whole Candidate custody; Prepared outcome; Presented after Prepared; missing permit revokes; DemandCancel outcome; cancelled permit revokes |
| 34–35 | Actions | exact presented-target Action identities; ActionAck exact echo and its Submitted custody |
| 36–46 | Custody/journal | Submitted precedes semantic outcome; identical retry before ack is idempotent; replay below watermark EALREADY; EAGAIN journals nothing; same-ID retry after EAGAIN executes once; event reads continue by byte offset; retained reread is identical; ack advances retention and old offset is ESTALE; past-tail EINVAL; tail read remains pending; submission IDs correlate independently from domain transaction IDs |
| 47–50 | Malformed submit | bad length; nonzero reserved bytes; excessive row counts; unknown kind—each EINVAL and no journal entry |
| 51–54 | Stream/flush | fragmented 9P requests; partial transaction staging; Rflush settles pending events read; subsequent publication never answers the flushed tag |
| 55–64 | r6 indicators | exact profile; object pin/republish/qid, including changed bytes at the same generation; activation custody and exact echo; stale generation and duplicate-event outcomes |
| 65–84 | r7 launcher | catalog/opening/allocation; permit/candidate custody and negative controls; Prepared before Presented/focus; Text and ack; query disarm; repaint; Accept/activation; stale ack; revocation and close |
| 85–96 | r8 dock | exact profile; identities and immutable pins; 4096-row catalog; allocation/permit/candidate and negative controls; presentation; activation echo and stale generation/slot |

The malformed cases run in a negotiated session. Absence assertions use
flush/barrier ordering rather than a quiet sleep.
Short writes distinguish the msize ceiling from the server's canonical upload
chunk boundary. Custody checks never treat Submitted as resource acceptance or
presentation.

### Served-export harness

One Go child runs the complete check set against thirteen isolated production
exports: main content flow, policy refusal, unservable negotiation, custody
pressure, malformed records, stream/flush, missing permit and cancelled permit.
Five additional exports serve bar, launcher, dock and separate launcher/dock
fatal-permit controls. Each uses a fresh registry and epoch. The oracle discovers the
epoch by reading `api` to EOF; no argument or control message supplies it. The harness
authorizes that child's PID before releasing its startup barrier. Admission is
supplied evidence, not a supervisor-authentication result.

The main fixture publishes output facts and drives real allocation, resource,
demand, candidate and action owners. It explicitly calls the preparation and
presentation joins, retains the render lease through those joins, then releases
it and checks resource/accounting cleanup. It also checks the owner-side result
of the exact ActionAck; the Go check reports only its wire-visible custody.

The role fixtures use the real native focus/input and candidate/allocation/resource
owners. **Catalog and indicator admission decisions are scripted from the
normative rules.** The oracle checks their wire echoes; it makes no claim about
Session launch/admission policy. Native activation eligibility uses the runtime
owner, with queue admission supplied by the fixture. No fixture starts an app.
Session eligibility remains covered by Session's own tests.
The phase-A file wire's cross-family arrival ordering limitation is not an
ordering guarantee of this verdict. These fixtures use explicit owner barriers
where one family's completion is a prerequisite for the next request.

A bounded test-only control channel carries scheduling phases, such as
republish-after-pin and release-journal-pressure. It carries no encoded shell
records or expected results. Fixture identities and intended state changes are
declared below; the Go client independently computes expected bytes and
validates responses. Production file requests and replies always use the real
export socket.

Fixture facts: bar revision 6, capability bits 0/7/8, content grant epoch 1;
output (2,1), facts generations 3 then 4, scale 1/1 generation 5; allocation
(1,1), 64 by 32; resource (5,1), two transparent BGRA pixels; interaction
generation 4; Prepared generations 7/8 and Presented epoch 9; Action event 11
on target (1,1). Allocation on output 99 is rejected. The owner clock is fixed
at 10 ms so scheduling cannot expire a permit; transport deadlines remain real.

The pressure fixture retains 192 real output announcements without ack, then
observes EAGAIN with no custody. Acknowledgement releases space and the same
submission succeeds once. The fatal-permit fixtures pause candidate service
until Submitted is read and acknowledged, queue an events read behind a
getattr barrier, then release the owner. Its stale error revokes the epoch;
the pending read must receive ESTALE before clean EOF. This check exposed the
premature-close bug fixed by `ef0b94537`.

The extended 96-check verdict passes against runtime `3856d1014`. Its fixes
discard the remaining parts of a whole candidate after a rejecting outcome and
publish large file objects under their object cap, preserving prior event order.
The strict role controls cover both fixes, including the 4096-row catalog.

Build offline with `GOFLAGS=-mod=readonly`, `GOPROXY=off`,
`GOTOOLCHAIN=local`, `GOWORK=off`; validate the existing module and sum pins.
Use a private Go build cache; Go and Cargo parallelism follow the caller. Bound child
execution, captured output, wire buffers, pending requests and control messages.
On failure, capture the transcript, terminate/reap the owned child, disconnect
exports and remove only the fixture's private temporary directory.

The verdict parser gets negative tests for omitted checks, duplicate names,
failure rows paired with a pass footer, truncated output and nonzero exit.
Independent body-validator tests mutate valid literals, including every
conditional rule clarified in the KDL.

### Gates and scope limits

From the repository root:

```sh
sh tools/check_shell_files_oracle.sh
cd tools/9p-oracle
export GOFLAGS=-mod=readonly GOPROXY=off GOTOOLCHAIN=local GOWORK=off
export GOCACHE="$(pwd)/../../target/shell-oracle/go-cache"
go test ./...
go vet ./...
```

Run offline Go tests/vet, the new runtime integration test, Rust formatting,
Clippy for the new test and `xtask check layout`. Recheck the existing C1 Go
build and tests. All gates run at the caller's priority and parallelism with
private output directories; respect operator pauses for latency measurements.
Signed commits go to the director for review and merge.

Shared xtask integration and any future Session-policy harness are separate
follow-ups. This fixture proves wire/owner interoperability within the boundary
above; it does not prove native rendering, protected launch, latency budgets or
attended desktop acceptance.
