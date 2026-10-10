---
id: 8jcykhdc
date: 2026-10-03
kind: plan
tags: [plan, milestone, security, lock]
---
# Secure session lock authority and lock provider role

## Scope and exit

Implement [t034](queue-13-authority-and-lifecycle-hardening.md#t034) under the
proposed [lock ADR](../decisions/w0seozxx-session-owns-lock-state-and-authentication-lock-providers-only-render.md):
an Engine-owned cover, Session lock state and input, a Session authenticator
(`sophia-factotum`, a Rust port of 9front's factotum) that is the only unlock
authority, and a separately admitted
lock provider role that only renders. niltempus promoted t034 to the parallel
lane on 2026-10-03 and reserved t291–t299 for its tasks.

t034 closes when t291–t296 are complete and its original exit holds: explicit
privileged admission and trusted unlock, coverage across topology changes,
revoked input and focus, and fail-closed provider loss and replacement, with
controls proving that ordinary content cannot claim locked state or receive
unlock input and that a stale completion cannot unlock a new epoch. t297's
attended acceptance is separate physical evidence.

t034 closed on 2026-10-07 with t291–t296 closed, and its original exit was
checked against their controls. Authority: only Session enters and leaves
locked state, and the t292 reducer controls show that no WM, shell, X client or
provider record can do either. The provider is admitted only through the lock
role's pidfd-checked endpoint, in its own protection domain, and cannot claim
or end the lock (t294); the C and Rust SDK clients speak that contract (t295).
Unlock is trusted to sophia-factotum behind PAM alone (t293), whose controls run
real PAM through a private configuration directory, kill a hung helper at its
deadline and refuse a forged acceptance. In the QEMU `session-lock` scenario,
physical keys reach it, a wrong password is refused and the right one unlocks
(t292). Input: at lock entry Session revokes input and focus and waits for the
X frontend to apply the new epoch; while locked, the VT and emergency
recognizers run first, synthetic input is refused and no key reaches the X
frontend, WM, shell, launcher or provider as text; unlock restores focus only
to a still-authorized target (t292). Cover: Engine draws the cover last on
every head through every path that builds a head frame, Session reports locked
only on every head's proof, a first Present waits for the unlock and an unlock
whose repaint cannot be queued keeps the cover (t291). These are CPU controls
on the mirrored target that add and remove outputs and heads while locked; they
are not topology continuity on the operator's hardware. Replacement: a provider
that crashes, stalls, is replaced or follows a render-device change leaves the
fill, and old feedback cannot settle a successor's image (t294). Stale
completion: a verdict for an earlier epoch or a superseded attempt never
unlocks, and relocking during authentication discards the one in flight; the
reducer controls cover this, and so does `NoStaleUnlock` in
`SessionLock.tla` (t296). Two things stay open. t297's physical acceptance on
an installed release has not passed. A monitor unplugged while locked ends the
session on the operator's hardware; that failure is t306's.

Sophia stays independent of any particular locker. Kleis is the first consumer;
its behaviour is tested in its own repository. Sophia's controls use generic
contract peers.

## Coordination with t289

pF and the t289 owner reported the overlap with t289 (CPU reductions,
uncommitted on `performance/t289` in `~/dev/sophia-cpu-performance`, base
`b6ad18cf`) on 2026-10-03. Send candidate diffs to the t289 owner before any
merge, and agree the merge order before touching the files below. Builds,
tests, Clippy and correctness gates need no shared slot: each lane runs them
concurrently at normal priority in its own reusable target with bounded jobs
(start at Cargo `-j8` and 8 test threads, adjusted to memory and I/O
headroom), coordinating only quiet measurement or reproduction windows and
exclusive hardware access (`docs/build-and-test-coordination.md`). The files:

- backend-live native scanout (`native_scanout.rs`, `persistent_native_scanout.rs`,
  its `construction.rs` and `topology/preparation.rs`): t289 adds per-head
  present clocks invalidated before topology preparation, and a new owner domain
  on resume. `production_visual_runtime/native.rs` changes retirement order;
  `resume_native_scanout` itself is unchanged.
- the Session owner loop (`owner_loop.rs`, `physical_input_loop.rs`, `run.rs`,
  `owner_wake.rs`, `owner_loop/authority.rs`, `authority_receive.rs`,
  `lifecycle/native_service.rs`): t289 adds Present-clock admission, wake and loss
  reconciliation. `routing/key.rs` and `PhysicalInputRoutingMode` are untouched.
- the X frontend (`runtime.rs`, `state.rs`, `dispatch.rs`,
  `x11_socket/connection/{dispatch,lifetime,private_service,server}.rs`,
  `routing/{registry,private_settlement}.rs`): t289 adds timed Present.
  `control_epoch.rs` and `control_transition.rs` are untouched; coordinate any
  epoch plumbing through the modified files.
- `Cargo.toml`, `Cargo.lock` (t293's new crate) and `todo.md`.

Free of t289 edits: `output_composition`, `display_list_without_policy`,
recovery and topology lowering, `role_endpoint`, the lock contract and
`sophia-factotum`.

Semantic interaction: background Present pacing and Present clock selection in
`background_present.rs` read the visibility inputs that the cover changes.
Lock suppression must feed actual sampling and clock selection for ordinary and
preview sources: while locked, application surfaces count as not visible, so
hidden clients are paced at the hidden rate and Present binds to the fallback
clock. Clock-debt reconciliation stays active while frame service is
quarantined. The planned W1 cached admission lease (not yet implemented) must
include the lock state. Locked routing must not leave timed Present or
private-input admission waiting on a focus change.

A read-only review of the t289 working tree on 2026-10-03 (against t291's
`a56f3131e`) found three t289 functions in `background_present.rs` that
re-derive visibility from the WM presentation instead of calling the runtime's
`surface_hidden`: `background_output_visibility`,
`background_visible_on_outputs` and `present_clock_outputs`. Whichever branch
merges second makes them lock-aware: a locked session has no visible surface,
so every Present binds to the fallback clock and is paced as hidden, and
unlocking produces the hidden-to-visible edge that releases parked Presents
promptly. Clock-debt reconciliation reads only native heads and stays active.
No t289 path waits on focus. Textual overlap is limited to
`production_visual_runtime.rs`, `authority.rs` and `todo.md`, in disjoint
hunks. The same review found that `service_first_visibility_presentations`
could release a parked first Present through a preview instance while locked,
re-parking it every pass; t291 now releases nothing while locked and leaves
the candidate to its expiry budget.

pF added a W1 requirement on 2026-10-03. t289's cached clock-selection proof
(`LivePresentClockSelectionSnapshot` with `present_clock_selection_matches` in
`background_present.rs`) compares the exact selection inputs: geometry, order,
routes, the WM tier, viewports and primary. The lock is a selection input too.
Whichever branch merges second adds the cover's epoch,
`session_lock: Option<SessionLockEpoch>` captured as
`self.session_lock.map(|cover| cover.epoch)`, to both the snapshot and the
match. Locking, relocking and unlocking then revoke every cached proof; without
this, a proof taken before a lock could still bind an application to a locked
head's clock. The snapshot's own requirement ("Lock integration must retain its
empty visibility in this same snapshot") is met by `background_output_visibility`
returning no output while locked, already on this branch.

## Task details

### t291 Engine lock cover

Add an Engine-owned lock layer drawn last on every head: an opaque fill, with a
provider image above the fill only for an exact presented candidate of the
current lock epoch on that head. Include it in every path that builds a head
frame: `OutputComposition::display_list`, `display_list_without_policy` and
`recovery_display_list_for_output`, `compose_output_topology_head_frames`,
`resume_native_scanout`, preview recovery and the software-only cycle. Stamp lock frames so Session can require the same stamp on
every `presented_head_frames` entry before reporting locked. Expose the lock to
visibility consumers as above.

Exit: with `MirroredTarget`, several outputs and mirror heads, adding and
removing outputs and heads while locked, every presented head frame carries the
lock stamp, and reference sampling finds no application, shell, descriptor or WM
pixel on any head through topology frames, resume, recovery and preview
recovery. Direct scanout is never selected while locked.

Checkpoint 2026-10-03 on `lock/t034` (rebased on t289's `5f55a71ca`): the cover,
the presented proof, lock-aware Present sampling and t289 pacing, and the
first-visibility guard are implemented. Controls: 10 Engine tests through the
production planner and 6 runtime tests on the mirrored target. Bounded mutants
on a separate source copy, each killed by a named assertion: no cover in
`OutputComposition` (four coverage tests), proof ignoring client surfaces and
proof ignoring non-rect draws (`a_client_drawn_beside_the_cover_voids_the_proof`,
added after the first run let both survive), t289 pacing ignoring the lock, and
Present sampling ignoring the lock. Device-hidden crate suites pass: Engine 552,
backend-live 818 (8 ignored), Session 311 (17 ignored). Open: a runtime control
for the first-visibility guard, which needs a queued-Present fixture, and the
unlock rollback, which needs an injected repaint failure.

t291 closed on 2026-10-06 with both open controls in place (534eda08b). They
are CPU runtime controls on the mirrored target. In
`a_first_present_parked_while_locked_waits_for_the_unlock`, a first Present
deferred before the lock is still parked 10 ms and 20 ms later while locked,
well inside its 2000 ms first-visibility expiry, and is released by the unlock.
In `an_unlock_whose_repaint_cannot_be_queued_keeps_the_cover`, the test target
refuses the batch before admission: a lock whose repaint cannot be queued keeps
the cover, an unlock whose repaint cannot be queued rolls back and leaves only
the cover in the head lists, and a retry unlocks. Three bounded mutants on a
separate source copy were each killed by the named assertion. The full gate
passed with 7174 tests, none failed and 100 ignored. Codex accepted the change
in a read-only review. The evidence is in
`development-evidence/t291-controls-01/`, with the review in
`REVIEW-CODEX-01.txt`. Attended acceptance stays with t297, and the hardware
hotplug failure while locked stays with t306.

### t292 Session lock state and input

Add `SessionLockState` with monotonically minted lock epochs; zero and
exhaustion refuse. Lock entry performs the full revocation set from one call
site and waits for the X frontend to apply the new epoch, moving lock and unlock
onto the requested/applied split of `ControlEpochCoordinator` instead of the
ungated counter. Add a `Locked` physical routing mode in which the VT and
emergency recognizers still run first and every other key goes to the lock
capture. Refuse synthetic input while locked. Add a reserved `session:lock`
action and a control request. Refuse screen-capture approval and revoke active
grants while locked; publish locked inspection without window state. Unlock on a
current verdict only: advance the epoch again, wait for it to apply, drop the
cover after the next presentation and restore focus only to a still-authorized
target. Hide the cursor while locked; its asset is Session's, so this is
presentation, not coverage. Keep the lock across VT switches and seat release. Use a test-support
authenticator seam.

Exit: controls show that no physical or synthetic key reaches the X frontend,
WM router, shell, launcher or provider as text while locked; VT and emergency
chords still act; old-epoch frozen input returns `EpochRevoked`; no WM, shell,
X client or provider record can enter or leave locked state; a verdict for an
earlier lock epoch or a superseded attempt never unlocks; relocking during
authentication discards the in-flight verdict.

Checkpoint 2026-10-03 on `lock/t034`: the `SessionLockState` reducer; the X
frontend publishing its applied input epoch (requested is not applied until
the broker clears grabs, frozen input and server grabs); Engine's
`SessionLockKeyboard` and Session's `SessionLockInput` with its bounded,
zeroing secret; the lock router in `route_physical_input` (VT and emergency
recognizers first, devices still arrive and leave, no pointer delivery); the
Session-direct `session:lock` shortcut, which the WM neither sees nor can
delay; the owner-loop phase that takes the seat, installs the cover, reports
locked only on every head's proof and an applied epoch, hands a submission to
the authenticator seam and unlocks only on the current attempt's verdict; and a
seat-wide synthetic-input switch that injectors and the broker both honour.
Controls: 11 reducer, 6 keyboard, 8 lock-input, the applied-epoch and
synthetic-admission broker tests, and the refused `session:lock` profile. Until
t293 supplies the authenticator a lock is refused and a `session:lock` binding
is an unavailable capability, so nothing here is live yet. Open: cursor
hiding, the control-bus lock request, egress gates (the screen-capture portal
has no live executor yet; WM inspection while locked), and an owner-loop
control driven by a fake authenticator once t293 defines it.

Later on 2026-10-03: the cursor is hidden on every head while the lock holds
the seat (`hide_hardware_cursor`) and redrawn at the pointer on unlock; it is
checked physically in t297, since no headless target drives the cursor
plane. The control-bus lock request is deferred to the separately authorized
administrative 9P export (todo row 15), together with idle locking (t110):
adding it to the socket control contract would extend the IPC the 9P
migration retires. The screen-capture gate goes in with the portal's live
executor, which does not exist yet; WM inspection carries metadata to the
operator only and is left as it is. The end-to-end owner-loop control needs
physical-origin keys, which the lock refuses from synthetic sources by
design, so it belongs in the QEMU session harness, whose in-guest uinput
injector produces them, or in t297.

Later still on 2026-10-03, on `lock/t034-egress`: the QEMU `session-lock`
scenario is that control. A proof-only `--inject-session-lock` starts the
lock once the GTK proof's physical input is armed; virtio-keyboard keys from QMP
then reach the real factotum agent and PAM in the guest, a wrong password is
rejected, the right one unlocks, and zenity's exact stdout shows no lock-time
key reached it ([validation](../../validation.md#session-lock-in-qemu)).

t292 closed on 2026-10-06, with each item it had left open now owned
elsewhere. Cursor hiding is checked physically in t297. The control-bus lock
request belongs to the administrative 9P export (t254) and idle locking to
t110. The screen-capture gate is part of the capture slice of t046, the portal
work that builds the executor it needs. WM inspection while locked is left as
it is, and the owner-loop control is the QEMU `session-lock` scenario above.

### t293 sophia-factotum core, pam and pass

Port 9front's factotum agent to `crates/sophia-factotum` under the
[factotum ADR](../decisions/hhbejm8k-port-9front-factotum-to-rust-as-sophia-factotum.md):
the key ring with hidden `!` attributes; the protocol-independent conversation
state machine with factotum's verbs, replies and phases; the `rpc`, `ctl`,
`proto`, `confirm`, `needkey` and `log` files on `sophia-9p`; the agent process
(supervised, not dumpable, memory locked, no cores) with socket, pidfd and same-UID
admission; and the protocol module interface shaped for dp9ik's multi-phase
conversations, authinfo secrets and auth-server dialing. Add the `pam` module:
`role=login` runs PAM in an executed helper over a length-prefixed pipe, with the
configured service (default `sophia-lock`), closed inherited descriptors, cleared
dumpability and best-effort `mlockall`, and without `NO_NEW_PRIVS` because
`unix_chkpwd` is setuid. Session's secret buffer is page-aligned, `mlock`ed,
`MADV_DONTDUMP` and zeroed with volatile writes on every clear, backspace and
submit. Ship an example `sophia-lock` PAM file with `pam_faildelay` and `pam_unix`
and no faillock. Never log secrets, their length or PAM prompts. 9front's
`factotum` sources and lockme's `auth.nim`/`password.nim` are the references;
ported files carry 9front's MIT notice. `pass` moves here from t298 because the
key ring needs a protocol that takes keys. The PAM binding is the separate
`sophia-factotum-pam` package, the one crate exempt from the unsafe-code lint;
`pam` is served only on Session's private channel. The decisions and defaults
are in the factotum ADR; the cited design is in development evidence.

Exit: conversation and key-ring controls ported from factotum's behaviour,
including hidden secrets on `ctl` reads and refused cross-UID peers;
deterministic real-PAM controls with `pam_start_confdir` against a private
directory using `pam_permit` and `pam_deny`; agent or helper crash and timeout
leave the session locked; secret pages are locked and zeroed after use.

Checkpoint 2026-10-03 on `lock/t034`: `crates/sophia-factotum` (attributes,
key ring, conversation, ctl, log, `pass`, `pam`, the 9P export, the job pool,
the helper launcher, the agent binary and Session's `UnlockClient`) and
`crates/sophia-factotum-pam` (the helper, `unsafe` only in `ffi`). Session
starts the agent from `--factotum-agent` and `--factotum-pam-helper`, makes
`session:lock` available with it, and installs it as the lock's
authenticator. `examples/pam.d/sophia-lock` is the example stack. Controls:
factotum and helper suites, including real PAM through a private confdir,
a hung helper killed at its deadline, a forged acceptance with a failing
exit refused, cancellation, and the agent's serve loop with Session's client
end to end. Open: a live-session run; the user endpoint waits for t275.

Review 2026-10-03 of `lock/t034` at 266ccc1fc (read-only, security and
correctness) found no unlock without a current verdict, no desktop pixel under
the cover and no lock-time key or XTEST press reaching a client. It found
availability and secret-handling defects, fixed on `lock/t034` before merge:

- The authenticator is supervised (`session_unlock_supervisor`): reported
  available only after the agent answers its handshake, replaced with a
  bounded backoff after a failed launch, a broken login or an exit while
  idle, and every attempt made while none is up answers `Unavailable`.
  Session refuses to lock while nothing is available.
- Locked keys are never counted, timed or recorded: no `keys_observed`, no
  physical-event metric, no keyboard coverage, and no per-edit log line.
- The secret's copies are zeroed or locked: Session's secret is one locked,
  undumped page; the client builds the request once and sends it through the
  SDK pipeline's `write_secret`, which zeroes the body and the output
  buffer's vacated bytes and reserves the buffer's whole bound before the
  secret enters, so no later request reallocates it (sophia-desktop-sdk-rs
  1cce77b, 9e59d78); the helper reads its request straight into its locked
  page.
- The helper's reply leaves on a private descriptor with stdout on
  `/dev/null`; the agent reads it and waits for the helper's exit under one
  deadline, killing and reaping it on overrun or cancel.
- The helper path is canonicalized and every ancestor must be root-owned and
  writable by no one else; the system's services verify only the caller's
  own account, whatever `LOGNAME` said.
- The lock input (keymap and secret page) is built before the lock is taken,
  so a failure refuses the lock instead of ending the session.
- An attempt may run while locking, so a head that never retires the cover
  cannot lock the user out; a lock request during an attempt starts a new
  lock that voids it; the cover stays through unlocking until the frontend
  applies the new epoch; the broker's synthetic executor refuses presses
  while admission is closed.

Accepted residuals: the agent's 9P server keeps consumed request bytes until
reused (the agent is undumpable and its memory locked); Session itself stays
dumpable, with its secret page excluded; the helper's stdout redirect has no
deterministic control, since only a module inside the helper can write there.

### t294 Lock provider role and contract

Specify `protocol/sophia-lock-files-v1.kdl` and `docs/sophia-lock-files.md` with
the common record identity shape. Add the role endpoint, protection-domain role,
supervised process kind and `SOPHIA_LOCK_9P_SOCKET`, with pidfd-checked peer
admission as in the output file transport. Add the operator selection
`session { lock-provider { executable; config; gpu } }`, GPU denied by default.
The provider is resident from session start and supervised with backoff; a
replacement gets a fresh connection epoch. Reuse the wire-neutral content store
with a separate budget derived from the topology. Deliver edit and status
events without characters, and opaque IDs for provider-registered chords that
include a modifier other than Shift; such chords never reach the secret.

Exit: codec bound to its KDL by test; an independent C peer exchanges records
with the export; the provider cannot claim or end locked state, see characters,
draw outside its allocation, present into a newer lock epoch from an older
connection or delay the cover; crash, stall and replacement leave the fill.

Checkpoint 2026-10-03 on `lock/t034-next`:
- `sophia_protocol::lock_files` is the record codec, bound to the KDL by
  `lock_file_schema.rs`. Rules the contract states in prose are covered in
  `lock_files.rs`.
- `sophia_runtime::lock_files` holds one provider connection epoch:
  - negotiation, which refuses chords that hold only Shift or that Session
    reserves;
  - uploads, which become images only once every byte has arrived;
  - frame demands and permits;
  - candidates, checked against the published lock object, the allocation,
    the image's size and the permit, with journal room held for every
    answer;
  - revocation of what a new lock object no longer grants.
- `LockFileExport` serves the files over the `sophia-9p` export: a fixed
  root, a single admitted attach, lock objects pinned per generation, one
  staged candidate, and upload writers fenced by binding.
- Controls: 13 custody, 7 export.
- Since then, on the same branch:
  - the endpoint (`PolicyRole::Lock`, pidfd admission, a fresh epoch per
    connection) and its worker thread;
  - the profile's `session { lock-provider }` selection, launched once a
    topology is published, in its own domain, restarted with backoff;
  - the lock object and the provider's limits derived from the topology;
  - entries for the lock's edits and verdicts;
  - content images given an owner-specific source, so a lock image has its
    own identity and texture handles apart from shell content;
  - the cover drawing each output's provider image over the fill, with the
    coverage proof extended to it;
  - candidates placed over their outputs, outcomes (presented, superseded,
    rejected) and permits paced by presentation.
  - granted chords reaching the lock keyboard, and a direct GPU grant;
  - an independent C peer against the production export.
- Open: following a render-device change after a direct grant, and a live
  run with a real provider (kleis through the t295 C SDK client).
- The live run with a real provider has happened: the operator installed
  `niltempus-99bb041fe3535b5d265d` with kleis on the granted render node,
  locked and unlocked on two outputs and found the keys normal afterwards
  ([lock performance plan](qrstyyjn-restore-lock-animation-and-input-responsiveness.md)).
  Following a render-device change after a direct grant remains open.

t294 closed on 2026-10-07 when the provider started following the render
device (f9191659b). A direct grant names one device. When the session's
admitted device changes, is lost or first appears, Session replaces only the
provider's process. The lock file service and its transport live as long as
the session, so every connection, and every image identity drawn from one,
takes the next epoch of one checked counter. From the change on, Session
ignores the old process's connection and submission events, ends its chords
and images at once so every locked head shows the fill, asks it to exit
without waiting for it, and asks the service to retire it. The service ends
the connection, admits nobody until the next authorization, and answers with a
`Retired` marker after every event the old process sent. The successor starts
only after the marker is seen and the old process reaped, in either order.
Its launch is prepared from the ungranted base for the latest device under the
next grant epoch. A preparation that fails is made again at the next start, so
no older launch is used and no grant epoch wraps; a prepared launch that fails
to spawn is retried as prepared, on the ordinary backoff. Changes that arrive
while a retirement is outstanding coalesce into the latest device. A direct
grant with no admitted device starts nothing until one is admitted. A service
that stops ends the provider for the rest of the session, and the cover keeps
its fill. Grants other than direct name no device and are never replaced this
way. The first launch now happens on the owner's first poll, so a first launch
that fails is retried on the backoff like any later one. The controls are in
`crates/sophia-session/tests/session_lock_succession.rs`, the provider's
in-crate tests (a process that ignores TERM does not hold the owner step, and
a reported service failure is terminal and discards its batch), and the
retirement tests in `crates/sophia-runtime/tests/lock_file_service.rs`.
Fifteen bounded mutants, one per rule, on a separate source copy built in its
own target, were each killed by a named assertion; the one that first survived
was killed after its control was strengthened. The full gate passed with 7192
tests, none failed and 100 ignored, and Codex accepted the change in a
read-only review. The owner-loop wiring that calls the provider and the
revocation helper has no control of its own, because no harness swaps the
admitted render device. Queue pressure is controlled in the succession rules,
not through a full service queue. An exhausted connection counter refuses each
successor without wrapping, and the service is not otherwise stopped. The
evidence is in `development-evidence/t294-regrant-02/`, with the review in
`REVIEW-CODEX-01.txt`; the held first design (bb3b9bf5f) and its reviews are in
`t294-regrant-01/`.

### t295 SDKs

Add a lock client to sophia-desktop-sdk-c and codecs to sophia-desktop-sdk-rs,
sign, re-pin and add both contract pairs to the xtask SDK checks. Publish the
SDKs before any Sophia consumer.

### t296 Models

Model lock epochs, stale verdicts, relock during authentication and provider
replacement in `validation/architecture`.

Checkpoint 2026-10-03 on `models/t296`: `validation/tla/SessionLock.tla`
models the reducer, the frontend's requested and applied input epochs, the
cover proof and a replaceable provider. The authenticator may answer any
attempt it was ever given, in any order. The model holds `NoStaleUnlock`,
`ProviderImageIsCurrent`, `InputReturnsOnlyAfterApplied` and
`LockedOnlyWhenCovered` over 395,063 distinct states (three epochs, four
attempts, two connections). Five negative controls, one per rule, each violate
their invariant; the stale-verdict control's trace is the relock case (attempt
1 of epoch 1, a relock to epoch 2, then attempt 1's verdict). It is temporal
and epoch evidence, so it lives with the TLA+ models rather than the Alloy
topology; role authority over the lock (no WM, shell, X client or provider
record can enter or leave it) remains covered by the code controls.

### t297 Attended acceptance

On an exact installed release with a lock provider: two outputs, hotplug while
locked, a VT round trip, a provider kill, a wrong and a right password. Physical
evidence stays separate from the deterministic checks.

On 2026-10-09 niltempus accepted locked cable and locked KVM return through
unlock on release 222 (Sophia `ada93fd4b`), as part of
[t306's operator recovery](../investigations/kdleagg3-kvm-output-and-usb-loss-returns-the-desktop-to-greetd.md#release-222-physical-acceptance-2026-10-09).
This is one admitted output; two-output coverage, VT round trip, provider kill
and wrong-password rejection are not supplied by those observations. t297
remains open. Signed `1162d2f81` adds the bounded lock-specific capture reducer
for status, source, verdict and numeric coverage fields, excluding free-form
errors. It is included in integration `96cb6d6d8`, whose full isolated gate
passes with 7,513 test passes, zero failures and 101 ignored. Evidence is
`t310-transport-publication-01`; the original unwired red and green controls are
in `t297-lock-capture-01` (manifest
`442b06f05e98b03de3278eed2e70671d9aa300ec32b9d4db27e4b5d977380dad`).
This reducer is a bounded capture filter, not a complete record validator:
consumers must still require their expected schema and proof fields.

Coverage dedup must include native owner identity
alongside lock and topology epochs, so a same-topology replacement records its
own cover. Add a regression for equal topology epoch with a different owner;
the current absence of that record is not itself proof of an uncovered frame.
Separate candidate `0b1d7ab60` implements that identity and was reviewed read-only;
the proof and owner are read from the same current native owner at the call
site. It is outside the frozen `96cb6d6d8` integration and its release candidate.

### t298 p9any and dp9ik

Add `p9any` and `dp9ik` (client and server roles) to `sophia-factotum`,
and port libauthsrv's ticket, authenticator and key formats, `passtokey`, `form1`
and authpak into `crates/sophia-libauthsrv`, shared with the separately hosted
auth-server port. Take the primitives from audited RustCrypto crates, the Ed448
field from `crypto-bigint`, and port only authpak's formulas (decaf, Elligator2,
SPAKE2-EE, the ladder), as niltempus decided on 2026-10-03.
Build an independent C oracle from 9front's libauthsrv and libsec sources,
including the C that `mpc` generates, under `tools/`. Wiring `Tauth` into
`sophia-9p` exports waits for t275's decision.

Exit: byte-identical vectors against the oracle for `passtokey`, authpak, `form1`
tickets and authenticators; live p9any/dp9ik exchanges between the Rust modules
and the oracle in both roles, with a stub auth server driven by the oracle;
malformed and replayed messages refused.

## Connections

[Factotum ADR](../decisions/hhbejm8k-port-9front-factotum-to-rust-as-sophia-factotum.md),
[Lock ADR](../decisions/w0seozxx-session-owns-lock-state-and-authentication-lock-providers-only-render.md),
[synthetic input ADR](../decisions/htm85gg0-admission-ingress-and-provenance-for-synthetic-input.md),
[launcher presented input ADR](../decisions/f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md),
[output file role ADR](../decisions/vkkjmufd-use-native-records-for-the-separate-output-file-role.md),
[target-resolved input](../../target-resolved-input.md),
[native desktop capabilities](../../native-desktop-capabilities.md).
