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
(`sophia-factotum`) that is the only unlock authority, and a separately admitted
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

## Task details

### t291 Engine lock cover

Add an Engine-owned lock layer drawn last on every head: an opaque fill, with a
provider image above the fill only for an exact presented candidate of the
current lock epoch on that head. Include it in every path that builds a head
frame: `OutputComposition::display_list`, `display_list_without_policy` and
`recovery_display_list_for_output`, `compose_output_topology_head_frames`,
`resume_native_scanout`, preview recovery and the software-only cycle. Hide the
cursor while locked. Stamp lock frames so Session can require the same stamp on
every `presented_head_frames` entry before reporting locked. Expose the lock to
visibility consumers as above.

Exit: with `MirroredTarget`, several outputs and mirror heads, adding and
removing outputs and heads while locked, every presented head frame carries the
lock stamp, and reference sampling finds no application, shell, descriptor or WM
pixel on any head through topology frames, resume, recovery and preview
recovery. Direct scanout is never selected while locked.

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
target. Keep the lock across VT switches and seat release. Use a test-support
authenticator seam.

Exit: controls show that no physical or synthetic key reaches the X frontend,
WM router, shell, launcher or provider as text while locked; VT and emergency
chords still act; old-epoch frozen input returns `EpochRevoked`; no WM, shell,
X client or provider record can enter or leave locked state; a verdict for an
earlier lock epoch or a superseded attempt never unlocks; relocking during
authentication discards the in-flight verdict.

### t293 sophia-factotum

Add `crates/sophia-factotum` with the secret buffer and a helper binary. The
buffer is page-aligned, `mlock`ed, `MADV_DONTDUMP` and zeroed with volatile
writes on every clear, backspace and submit. The helper is executed, not forked
from the multithreaded Session, reads a length-prefixed request, runs PAM through
a minimal in-tree binding with the configured service (default `sophia-lock`),
closes inherited descriptors, clears dumpability and attempts `mlockall`. It does
not set `NO_NEW_PRIVS`, because `unix_chkpwd` is setuid. Ship an example
`sophia-lock` PAM file with `pam_faildelay` and `pam_unix` and no faillock;
installation belongs to the desktop installer. Never log the text, its length or
PAM prompts. lockme's `auth.nim` and `password.nim` are the reference design.
The first scope is unlock only; any wider agent role needs its own decision.

Exit: deterministic real-PAM controls with `pam_start_confdir` against a
private directory using `pam_permit` and `pam_deny`; helper crash and timeout
leave the session locked; secret pages are locked and zeroed after each use.

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

## Connections

[Lock ADR](../decisions/w0seozxx-session-owns-lock-state-and-authentication-lock-providers-only-render.md),
[synthetic input ADR](../decisions/htm85gg0-admission-ingress-and-provenance-for-synthetic-input.md),
[launcher presented input ADR](../decisions/f64wqfh2-independent-native-launcher-admission-and-presented-input-contract.md),
[output file role ADR](../decisions/vkkjmufd-use-native-records-for-the-separate-output-file-role.md),
[target-resolved input](../../target-resolved-input.md),
[native desktop capabilities](../../native-desktop-capabilities.md).
