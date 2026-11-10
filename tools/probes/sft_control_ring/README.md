# Commit-only carrier: real-export correctness checkpoint

Selected scope: z8cr4n2p, commit-only, pread-first. This standalone Linux probe
has no production carrier API, host feature, native entry point or timing command.
The real-export seam is reachable only from existing runtime `cfg(test)` support.
Root Cargo changes are a dev-dependency, its lock entry and the standalone
workspace exclusion; no non-test `crates/*/src` item changes.

## Current shape

```
peer                               owner
own writable SQ mapping            never maps SQ
  write header + FrameCommit
  publish 8-byte sequence hint ----> read one hint, validate sequence
                                    pread privately; validate header == hint
                                    reserve one response before effects
                                    real writer check + FrameSubmit export
             <-------------------- write one exact 80-byte receipt to pipe
  read receipt; validate identity
  only now reuse SQ

journal events / offsets / holds / standalone ACK: still 9P, unchanged
```

One outstanding attempt, no deliberate frame buffering. The producer alone owns
`sophia_linux_peer::ProducerRing` and its existing unsafe mapping implementation;
all host-side input handling is private pread. The probe crate forbids unsafe.
The host creates both directional pipes, retaining the SQ read end and receipt
write end; only the opposite ends go to the peer. Thus the peer cannot change
the NONBLOCK flags on the host's open file descriptions or block the host read
by draining a shared eventfd. No helper thread or futex waiter is introduced.

The new receipt is a pipe record, not shared CQ memory. The earlier mapped CQ
at 350ce8c3d is frozen history. A pwrite-only sealed CQ is unavailable:
F_SEAL_FUTURE_WRITE also rejects host pwrite; without it a peer can reopen its
read-only memfd through /proc/self/fd and write. The receipt pipe removes that
host mapping, a separate bell, and the torn-CQ read problem. A peer can reopen
its pipe reader for writing and inject a lie to itself; that cannot change host
custody, exactly as a client can lie to itself about an Rwrite. The host never
uses receipt bytes supplied by the peer as authority.

## Publication and bounded servicing

The **eight-byte hint write is publication**. The SQ cell is immutable from that
write until its exact receipt is observed. The receiver consumes one hint per
wake before touching shared storage:

- Expected next sequence: private pread is allowed, but the header must still
  equal both that hint and host-local expected sequence.
- Last consumed sequence: idle, with no pread of a possibly reused cell. One such
  duplicate is tolerated per sequence; a second closes rather than allowing an
  unlimited hostile idle stream.
- Gap, out-of-order, zero/wrap, short read or error: close the carrier. Full/short/
  error hint or receipt write also closes, never spins, retries indefinitely or
  claims delivery. A conforming one-outstanding peer cannot fill either pipe.

A sequence hint is transport identity, never admission, ACK or release authority.
Pipe framing and the wait are bounded. The persistent fixture uses an absolute
10 s deadline, a child pidfd obtained from the launch result, and process-exit
readiness in the same poll as requests. Exit wins over work. HUP is checked
before IN: closing the hint writer with a hint buffered loses it, so a conforming
peer keeps its writer open until the receipt. Error returns must
end the owning control endpoint; an unanswered published attempt is unknown,
not replayable. The fixture retains export custody when transport closes.

The source has exact extent `64 + SHELL_FILE_MAX_TRANSACTION_BYTES` (8256) and
GROW|SHRINK seals. A capture buffer is allocated once and reused. Both read shapes
exist for later comparison: header then payload (two preads), or the full extent
(one pread followed by private compaction). One local EINTR retry per read is
bounded; short successful reads are never stitched. Any remaining capture error
closes the slot. Untrusted bytes may change during capture; no atomic snapshot
claim is made for a hostile producer. The metadata/payload identity and host
codec still validate the private result. Mutation after capture is inert.

## One admission implementation

The earlier supplied `Owner` implementation is deleted. `Slot` holds only
sequence, pending response and closed state. It reserves before invoking an
adapter, preserves exact errno and count, and gates publication separately.

`crates/sophia-runtime/tests/support/shell_frame_control_carrier.rs` is mounted
under the existing private frame-export tests. Captured bytes go through the
real `read_reusable_transaction` and `ShellFiles::write(FrameSubmit)` path and
therefore the host `sophia_protocol` codec, ACK/recycle, staging, expiry, watermark
and journal implementation. The explicit read_reusable_transaction check binds
the peer-claimed writer; the export checks its own live writer. A production
adapter must keep this binding check and the per-operation authenticated
Export::check gate. The older direct-export tests omit that wrapper; the
authenticated adapter tests below exercise it. No visibility is widened.
Tests compare direct
FrameSubmit results and journal/state to the carrier for retries, changed bytes,
EALREADY plus poisoned writer, expiry/clunk/reopen, ACK-before-backpressure,
publication holds and transport closure. There is no second admission owner.

The persistent child fixture publishes three immutable submissions, interleaves
two duplicate old hints, and observes three real-export receipts. The host makes
three captures, never touches the SQ for the duplicates, and retains the same
inbound records/journal as the direct export control. This is a mechanism proof,
**not yet the socket-framed 9P performance control**.

Success receipts bind sequence, epoch, writer, submission, byte count and the
exact applied ACK word. EAGAIN confirms no ACK even if it applied before admission
failed. Standalone ACK and submissions serialize in both directions. EALREADY
sets unknown and closes. Client begin prepares an attempt; only a successful
hint write through Client::publish marks it published. A failed hint write
closes as unpublished, while a published attempt with no receipt closes unknown.
Both states block overlapping ACKs; a receipt before publication is refused. Every receipt pipe record must answer the pending
attempt; duplicate or foreign messages close. The former benign duplicate CQ
cell rule is obsolete because the CQ is no longer shared storage.

## Authenticated adapter checkpoint

The cfg(test)-only `shell_frame_control_carrier_auth.rs` fixture adopts a real
UnixStream pair through ShellFileWire and negotiates auth/attach/walk/open with
the vendored Pipeline. Both sides use the same ShellFiles construction and
supplied test authentication owner. The control sends actual socket-framed
Twrite; the candidate dispatches a privately captured record through
AuthenticatedFiles. Common export state and journal bytes are compared.
This is adapter wiring evidence, not Factotum/LaunchCustody or timing evidence.

The binding retains the host's adopt connection and published attachment epoch,
not the captured connection_epoch. It names the FrameSubmit write-open and the
reusable writer opened during setup (the host-issued Lopen qid is its id).
That fixed write-open replaces `opened.access.writes()` for this fixture;
`read_reusable_transaction` still rechecks the bound writer on every dequeue.
The fixture keeps the FrameSubmit fid open for its lifetime. Production binding
installation, FrameSubmit clunk/replacement and descriptor negotiation are not
implemented by this test binding. A lying header cannot replace host identity.

After capture and before any write, `AuthenticatedFiles::check` runs with
Operation::Write. Failure closes without effects or a receipt. Slot reservation
still precedes the real write/ACK effects. Receipt delivery wraps the actual
pipe write in `AuthenticatedFiles::with_delivery`, also Operation::Write:
one authentication-owner check and two inner export checks (the wrapper's check
and inner with_delivery). The tests assert one owner check per dequeue and one
per delivery. No custody-dispatch scope is opened; these are harness call counts,
not production syscall counts. Oversized successful counts close with EIO, as
Connection::write refuses them; short success also closes without custody.

**Required asymmetry:** ShellFiles never enables guard_read_delivery. None of
its current shell replies, including Rread and Rwrite, has the carrier's fresh
delivery guard. The blocked-socket control demonstrates an owed lean Rwrite
flushing after ShellFileWire::revoke. The carrier must suppress its receipt if
authority is withdrawn after acceptance; transport close does not undo accepted
owner work. This stricter delivery rule is intentional, not errno/lifecycle
parity. The tests also cover authentication failure after capture, wrong host
connection/epoch, lying headers/writer ids and writer clunk after capture.

The first authenticated fixture is in-process. The follow-up child composition
uses the same Binding::process/deliver functions with a real child-owned SQ,
publication pipe and receipt pipe. The 9P attach, writer fid and FrameSubmit fid
belong to the harness Pipeline in the owner process; the child owns only the SQ,
hint writer and receipt reader, and receives the binding over the private test
bootstrap. This proves adapter composition, **not binding to the child's own
authenticated attach**, production descriptor negotiation or a per-role census.

The child publishes three sequential attempts and two duplicate old hints.
Normal delivery confirms each exact receipt before reuse. On withdrawal after
the third Process, the fresh gate refuses; closing the receipt writer produces
EOF, the child's Client disconnects unknown, and all three owner records remain.
For exit with a receipt pending, the owner first processes the third attempt,
then sends E over the control stream; the child disconnects unknown and exits.
Only after child termination does the owner poll the pidfd, observe Exited, and
close the pending transport without delivery. This pins accept -> exit -> poll,
not exit-before-capture or write-after-reader-close. Every case compares the
remaining export state and records with three actual socket-framed Twrites.

A separate test clunks the reusable writer through 9P between Process and
Deliver: accepted custody and its receipt survive; the next attempt refuses
ESTALE and adds no owner record. The fresh attach/epoch delivery check remains.
No production lifetime routing is implemented by these tests. FrameSubmit
clunk/replacement, grant replacement, full/held journal and idle ACK through both
complete carriers, and the child's own authenticated attach remain open before
measurement.

## Setup, authority and remaining comparison gate

The current setup channel is a private Unix SCM_RIGHTS **test bootstrap** using
the existing descriptor receiver: peer->host SQ memfd; host->peer SQ pipe writer
and receipt pipe reader. Its negotiated identities are supplied by the fixture.
This is not an existing image-channel message or a new public allocation. The
current image protocol does not negotiate these host-to-peer carrier descriptors;
production setup needs its own contract/ledger review before integration.

The older real-export tests supply negotiation. The authenticated fixture proves
the wrapper wiring described above, not caller-fresh pidfd/admission checks,
production fid-binding lifecycle, grant replacement or all loop failures. Writer clunk/expiry is exercised through the real export;
a production adapter still needs the full authenticated wrapper and withdrawal
routing. No process-exit or transport event may invent pixel release or undo
accepted owner work. Full/held event offsets, idle ACK, terminal reserve and
remaining lifecycle cases must be exercised through both complete transports
before timing. Existing independent journal tests are not a composed proof.

For a future owner integration, the proposed doorbell belongs to fd-class
readiness (`cause_fd`, bit 8). Existing component observed/blind counters must
continue to count their actual wire visits; a direct carrier commit must not be
renamed a 9P turn. Separate carrier capture/dispatch counters and completed-frame
denominators would be needed. No native counter or wait classification changes
are implemented here.

Static host I/O shape: one hint read, one or two preads, one receipt write,
plus polling and the shared authority/export work: 3–4 carrier syscalls before
those shared costs. Do not claim fewer syscalls than 9P. The possible saving is
framing/dispatch/reply bookkeeping, not removal of freshness checks. Next is an
actual matched socket-framed control and a counts-only per-role census, before
paced or throughput timing. Setup, allocation and tracing costs must stay visible.

## Model correspondence and limits

| Executable boundary | CommitOnly projection | Limit |
|---|---|---|
| write hint after immutable SQ | Publish | pipe framing and sizes absent in model |
| capture after expected hint | Capture | real read errors/bytes not modeled |
| reserve response, call real export | Process | export internals abstracted, not atomic Rust proof |
| delivery gate then successful receipt pipe write | Deliver | partial/error write closes; no refinement claim |
| client validates exact receipt | Receive | real errno/codec and unknown duplicate behavior are code tests |
| endpoint closes with attempt unanswered | Close | accepted owner work remains separately owned |

Each successful fixture attempt has that order. The finite model does not admit
all subsequent legitimate frames or writer reopens, and its wiring controls are
not behavioral proofs. This table is a source mapping, not validated execution
traces or a composition proof; do not read a future comparison as model refinement.

## Gates

Root xtask/fmt do not cover this excluded standalone workspace. Run its explicit
gates from the Sophia worktree root, always through cargo-slot:

```
cargo-slot sophia cargo fmt --manifest-path tools/probes/sft_control_ring/Cargo.toml -- --check
cargo-slot sophia cargo test --offline --locked --manifest-path tools/probes/sft_control_ring/Cargo.toml
cargo-slot sophia cargo clippy --offline --locked --manifest-path tools/probes/sft_control_ring/Cargo.toml --all-targets -- -D warnings
cargo-slot sophia cargo doc --offline --locked --manifest-path tools/probes/sft_control_ring/Cargo.toml --no-deps
cargo-slot sophia cargo test --offline --locked -p sophia-runtime --features experimental-native-frame-transaction control_carrier --lib
```

Also run the full root `cargo xtask check` for its new test-only seam. The recorded
isolation uses umask 022, bwrap hiding /dev/dri and /dev/input and clears session/
SOPHIA_* environment. Ignored child fixtures are explicitly invoked by their
non-ignored parent tests. Dependencies use held root-lock versions; no binaries
are copied into evidence. No native launch or CPU/saving claim follows.

Checkpoint 52ea995d4 was accepted as device-free correctness evidence by review
6f59474c (sophia-control-carrier-probe-review-claude-02). It classified all prepared
attempts conservatively as unknown on disconnect; the client publication state
follow-up separates them without changing host custody. Remaining transport and
authority gates above are unchanged.
