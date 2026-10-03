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

Sophia stays independent of any particular locker. Kleis is the first consumer;
its behaviour is tested in its own repository. Sophia's controls use generic
contract peers.

## Coordination with t289

pF and the t289 owner reported the overlap with t289 (CPU reductions,
uncommitted on `performance/t289` in `~/dev/sophia-cpu-performance`, base
`b6ad18cf`) on 2026-10-03. Send candidate diffs to the t289 owner before any
merge; gates run in a shared slot. Agree the merge order before touching:

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

### t295 SDKs

Add a lock client to sophia-desktop-sdk-c and codecs to sophia-desktop-sdk-rs,
sign, re-pin and add both contract pairs to the xtask SDK checks. Publish the
SDKs before any Sophia consumer.

### t296 Models

Model lock epochs, stale verdicts, relock during authentication and provider
replacement in `validation/architecture`.

### t297 Attended acceptance

On an exact installed release with a lock provider: two outputs, hotplug while
locked, a VT round trip, a provider kill, a wrong and a right password. Physical
evidence stays separate from the deterministic checks.

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
