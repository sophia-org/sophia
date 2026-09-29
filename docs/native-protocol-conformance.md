# Native protocol family conformance

## Generic descriptor host coverage

Sophia's ordinary conformance tests exercise all three descriptor host modes
with `shell_descriptor_file_peer.c`, an independent C SDK peer over 9P2000.L.
It links the pinned C SDK's file libraries, without any Rust codec:

```sh
cargo test --offline --locked -p sophia-conformance --test shell_descriptor_modes
```

The `--proof` path covers descriptor presentation, exact activation and
withdrawal. The `--serve` path also covers tab supersession, host rejection of a stale
presentation epoch, the maximum 256-row shortcut catalog, committed-page
navigation and reference dismissal. The `--bar-proof` path checks that a
reservation changes the work area only at commit and that withdrawal restores
it. The host launches the peer through the protected shell supervisor; a missing
Bubblewrap or unavailable isolation fails the test. Each invocation has a
deadline and capped logs.

The host accepts optional client arguments after the mode and forwards them as
an argument vector. Negative fixture runs send validly encoded tab acks with a
wrong activation ID or transaction, or a refused disposition. Wrong identities
never reach the waiting owner and hit its response deadline; a refused
disposition reaches the owner and is rejected explicitly. Each test requires
that specific failure. The file owner refuses a stale presentation before
disclosing an activation, replacing the old socket fixture's stale-event
injection. This does not claim client-side stale-event rejection.

The same C peer exercises the launcher host: a 4096-row catalog, presentation,
activation, replay rejection and a new query. Unpresented and pending-query
grants are refused by the host; replay is refused by the peer. Persistent peers
exit cleanly on SIGTERM. Product-client checks retain their own evidence. None
of these scripted presentation outcomes proves physical rendering or input.

## Combined client gate during relocation

From a Sophia checkout, without sibling WM or shell checkouts:

```sh
mkdir -p .artifacts
cargo xtask check native-protocol-family \
  --output=.artifacts/native-family-run \
  --target-dir=.artifacts/native-family-target
```

The output directory must be new. `--timeout=3600`
bounds the whole run; values 1–7200 seconds are accepted. The compatibility
launcher `tools/check_native_protocol_family.sh` forwards the same arguments.
Use a target directory owned by this worktree so the compiled xtask resolves
the intended workspace. No canonical main-tree checkout is needed for this gate.

Prerequisites are the offline Cargo dependencies, C compiler, Bubblewrap and
GNU timeout. Missing isolation tools fail the gate. Source identity includes
Sophia's commit and tracked diff digest, including its pinned SDKs; stage new
source files before running. Changing the checkout during the run produces
NORESULT. Product repositories gate their own clients separately.

The runner mounts a fresh device directory, hides installed session sockets,
clears display and role-socket variables, and supplies private configuration,
runtime and temporary directories. It never grants the inherited real atomic
scanout opt-in. Protected shell hosts still establish their own process domains;
the outer isolation does not replace role admission. This is a deterministic
protocol/owner check, not an installed desktop or physical input/display test.

`report.json` schema 2 retains the source identity and verdict for each phase, with
separate logs. An unavailable prerequisite, failed phase or deadline stops the
run with a failing exit status; it cannot become a partial PASS. Cargo test
phases must report at least one executed passing test; an empty or ignored-only
target is refused. Output's owner phase explicitly enables `native-session`.

| Phase | Evidence retained |
| --- | --- |
| WM file export and independent client | Complete-record/capability controls, profile reducer, C SDK configuration/snapshot/projection/session-operation exchange, protected stale/timeout recovery and control-driven replacement |
| Shell independent clients | Every retained shell revision/capability corpus; independent C decoders and 9P descriptor, tabs, shortcuts, reservation, launcher and content peers; malformed negative controls |
| Protocol and runtime | All integration targets, including output schema equivalence, negotiated denial, foreign/stale grants, revocation, partial I/O, queue pressure, multi-component ownership, output replacement and exact backing release |
| Engine owners | All Engine integration targets, including coherent work areas, content capture/stack and topology transactions |
| Output owner/client | Live output authority reducer and output client's retained scenarios; experimental output has no independent full-lifecycle client yet |
| Control service | Separate host-administration envelope/codec, access, real service and independent Python client; control is not a supervised desktop role |

The C peers remain independently implemented and do not acquire a Sophia Rust
codec dependency. Product shell checks run in their own repositories; the
shell script no longer reads or builds a sibling Narthex checkout. The combined
runner records Sophia and its pinned SDKs. The WM role retains revision-3
semantics; its independent C file peer is mandatory on every run. Shell and
output remain experimental. Record readers prove byte agreement; protected clients
prove admitted lifecycles. Hosts supply presentation completions, topology and
activation facts where documented by their scenarios, so neither proves native
scanout, physical input, GPU execution permission or installed-session acceptance.
The shell phase's content host serves only 9P2000.L. It runs an independent C
peer linked against the pinned C SDK built without its IPC library, together
with red mutations and hand-encoded malformed-record controls against the
production file export. The protected popout lifecycle test uses the same file
contract and independent C SDK peer. An optional
externally supplied 9P content client is selected by the absolute
`SOPHIA_CONTENT_LIFECYCLE_CLIENT` path, run against the host and reported
separately; its absence does not erase the required independent C evidence.

The [native family contract](sophia-policy-ipc.md), role contracts and checked-in
schemas remain the specification. The runner is an evidence collector and
cannot add authority or declare a role stable.
