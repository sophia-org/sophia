# Output file native proof

This opt-in proof qualifies the revision-1 output file role. It does not add a
wire message or implement confirmed display trials. Ordinary tests hide devices
and session sockets; physical qualification needs a separately authorized,
attended window and a prepared recovery route.

## Controls

`--output-proof-readback` records KMS state through the cards already owned by
Session. `--output-proof-peer-loss-after-apply` also holds the assigned peer's
first runtime Apply after the all-cards Applied transition and requests its
supervisor's termination. Both require native scanout, normal Session, an
explicit `--output-process`, a positive `--max-runtime-ms`, and
`SOPHIA_FRAME_FED_OUTPUT_ARM=1`. They exclude the other WM/output proof controls.
Startup and profile-reload transactions cannot consume the peer-loss hold.

Presentation continues while the hold prevents first-presentation settlement
from committing. The termination request does not disconnect the service or
cancel the candidate. Actual supervisor exit requests an admission pause; the
worker's Disconnected event then enters ordinary cancellation and rollback.
The verdict requires all of:

- the captured supervised peer exited by TERM or KILL;
- Disconnected named the armed connection epoch;
- the native owner observed RolledBack;
- restored KMS readback equals the saved pre-apply readback;
- restored native-owner state equals its pre-apply state;
- the applied mode timing differs from before.

Exit and disconnect may arrive in either order, including across restoration.
Worker failure, channel loss, reassignment, an unexpected exit, or a termination
request error poisons the proof. A proof error after apply requests rollback
through the existing native owner; it cannot release the commit hold.

The departure deadline is five seconds: the supervisor has a two-second
TERM-to-KILL grace, leaving time for reaping and worker event delivery. Expiry
marks failure and keeps requesting rollback until settlement. This is not a
physical-restoration deadline. `--max-runtime-ms` remains Session's outer bound;
if it interrupts an incomplete proof, qualification fails with
`reason=session_runtime_deadline restoration=unproven`. Ordinary shutdown may
then run without completing the in-session rollback. The external runner must
allow startup, peer work, departure and restoration inside that bound and treat
any missing verdict, session error, or hard timeout as failure. It must never
infer restoration from process exit or from an unchanged published topology.

## Evidence

The generic C peer uses the pinned public SDK. It requires a complete explicit
baseline layout A and its exact topology epoch, plus target B where applicable.
It consumes lower epochs while waiting, refuses a passed epoch, and checks A
before submitting. Transform and VRR intent come from arguments: revision 1
does not publish their current values. The native gate must establish A from
the supplied profile and physical evidence.

Stages are `validate`, `reject`, `commit-restore`, and
`apply-await-termination`. The last stage continues dispatching until killed;
it has no signal handler or voluntary disconnect. Its records are flushed
immediately so termination does not hide earlier evidence. The reject fixture
uses an unknown mode and exercises transport admission, not Session's semantic
owner refusal.

Session emits `sophia_output_kms_readback schema=1` rows at baseline, before,
applied, installed, presented, restored, and peer exit as those boundaries occur. Each set
ends with `complete=true heads=N`. Candidate sets carry connection epoch,
base topology epoch and transaction; baseline and peer-exit sets use zero for
the unattributed connection and transaction. `t` is monotonic nanoseconds.

Rows identify head, card index, connector, CRTC and plane. The mode tuple is
width, height, vertical refresh, clock, horizontal sync start/end/total,
vertical sync start/end/total, horizontal skew, vertical scan and mode flags.
Properties include connector routing, CRTC activity, plane source/destination
geometry, and available rotation/VRR properties. Missing required properties
fail the proof. Missing optional properties remain absent. Framebuffer and mode
blob IDs are excluded because restoration may allocate new resources.
Mode names and DRIVER/PREFERRED metadata are excluded: equality compares exactly
the printed timing tuple. The proof requires an actual mode-timing change in B
and unchanged head enablement and connector/CRTC/plane selections. It does not
qualify head disabling, routing changes, or leaks outside those selections.

These are kernel property reads, not pixel comparisons. Sophia performs
transforms and mappings in software; KMS rotation does not prove them.
Separate `sophia_output_owner_readback` rows record installed native-owner head
enablement, output, size, scale, refresh, transform, mapping and VRR policy,
plus logical output sizes. Restoration compares those values too. The Applied
rows precede owner installation; Installed rows record the adopted candidate.
Neither plane position nor logical size proves global desktop origins or the
primary output. Those remain in Session's candidate and restoration evidence. Readback does
not inventory unrelated outputs or other DRM clients, and owner-state equality
does not establish rendered pixel correctness.

The peer-loss hold suppresses first-presentation settlement, so it emits no
Presented row for B; Applied and Installed supply that stage's evidence. Peer
and Session timestamps use the same monotonic clock because the protection
domain does not create a time namespace. A future time namespace would require
a different clock join.

The peer-loss join uses the peer records, supervisor exit and pause records,
the exact-epoch Disconnected event, cancellation/rollback records, restored
readback, and `sophia_output_peer_loss_proof status=passed`. A peer pass line is
not expected in the termination stage. Rollback failure records
`restoration=unproven` and fails Session; it requires operator recovery.

## Qualification boundary

`cargo xtask check output-file-native-proof prepare --output=/PRIVATE/NEW/DIR`
requires a clean signed checkout and verifies its vendored SDK. It builds the
generic peer, runtime harness and Session harness in a private namespace with
devices, network and session sockets hidden. It requires the declared runtime
test set and runs the protected Session fixture, then writes artifact hashes
to `prepared.json`. Builds run at the caller's priority with the caller's
`CARGO_BUILD_JOBS`, or every available CPU; `prepared.json` records the jobs
used and the actual nice value. It keeps compiler and test logs, refuses an
existing output directory, and records native acceptance as false. A changed source or artifact identity prevents successful preparation.
The offline Cargo registry must already contain extracted locked dependencies.

Runtime fixtures exercise the real file service with supplied owner outcomes.
Session fixtures exercise supervision, cancellation and debt with supplied
physical observations. Neither proves KMS apply, presentation or restoration.
The attended gate must run the assembled signed candidate with a conforming
9P WM. Named desktop assembly and its acceptance runner belong in external
integration tooling, not Sophia. T272 retirement remains blocked on T253's
native acceptance, followed by qualification of the assembled strict-API
Sophia/SDK/WM replacement.
