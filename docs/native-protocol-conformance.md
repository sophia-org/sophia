# Native protocol family conformance

From a Sophia checkout, with independent Hagia and Narthex checkouts beside it:

```sh
mkdir -p .artifacts
cargo xtask check native-protocol-family \
  --output=.artifacts/native-family-run \
  --target-dir=.artifacts/native-family-target
```

The output directory must be new. Optional `--hagia-root=/path` and
`--narthex-root=/path` select other independent checkouts. `--timeout=3600`
bounds the whole run; values 1–7200 seconds are accepted. The compatibility
launcher `tools/check_native_protocol_family.sh` forwards the same arguments.
Use a target directory owned by this worktree so the compiled xtask resolves
the intended workspace. No canonical main-tree checkout is needed for this gate.

Prerequisites are the offline Cargo dependencies, C compiler, Nim/Nimble and
their installed independent-client dependencies, Bubblewrap and GNU timeout.
Missing checkouts or isolation tools fail the gate. Source identity includes
each checkout's commit and tracked diff digest; stage new source files before
running. Changing a checkout during the run produces NORESULT. Immutable local
clones can keep a collaborating agent's in-progress checkout out of the run.

The runner mounts a fresh device directory, hides installed session sockets,
clears display and role-socket variables, and supplies private configuration,
runtime and temporary directories. It never grants the inherited real atomic
scanout opt-in. Protected shell hosts still establish their own process domains;
the outer isolation does not replace role admission. This is a deterministic
protocol/owner check, not an installed desktop or physical input/display test.

`report.json` retains the source identities and verdict for each phase, with
separate logs. An unavailable prerequisite, failed phase or deadline stops the
run with a failing exit status; it cannot become a partial PASS. Cargo test
phases must report at least one executed passing test; an empty or ignored-only
target is refused. Output's owner phase explicitly enables `native-session`.

| Phase | Evidence retained |
| --- | --- |
| WM independent clients | Schema regeneration check; valid/malformed frames and records; Rust, C99 and independent Nim Hagia lifecycle, denial, stale/invalid proposals, timeout, restart and last-layout preservation |
| Immutable WM r3 client | Archived SHA256SUMS verified before compilation; archived client runs all lifecycle and reconnect scenarios without regeneration |
| Shell independent clients | Every retained shell revision/capability corpus; independent C decoders and descriptor/launcher socket clients; Nim Narthex descriptor, live serve, reservation and launcher proofs; malformed negative controls |
| Protocol and runtime | All integration targets, including output schema equivalence, negotiated denial, foreign/stale grants, revocation, partial I/O, queue pressure, multi-component ownership, output replacement and exact backing release |
| Engine owners | All Engine integration targets, including coherent work areas, content capture/stack and topology transactions |
| Output owner/client | Live output authority reducer and output client's retained scenarios; experimental output has no independent full-lifecycle client yet |
| Control service | Separate host-administration envelope/codec, access, real service and independent Python client; control is not a supervised desktop role |

The C and Nim proofs remain independently implemented and do not acquire a
Sophia Rust or generated-binding dependency. WM revision 3 is the stable role;
its immutable client is mandatory on every run. Shell and output remain
experimental. Corpus readers prove byte agreement; protected socket clients
prove admitted lifecycles. Hosts supply presentation completions, topology and
activation facts where documented by their scenarios, so neither proves native
scanout, physical input, GPU execution permission or installed-session acceptance.
An optional externally supplied Lom content client retains its existing
`SOPHIA_LOM_CONTENT_CLIENT` path and is reported separately by the shell phase;
its absence does not erase the required C/Nim evidence.

The [native family contract](sophia-policy-ipc.md), role contracts and checked-in
schemas remain the specification. The runner is an evidence collector and
cannot add authority or declare a role stable.
