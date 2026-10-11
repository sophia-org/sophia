---
id: etyvymxi
date: 2026-10-11
kind: investigation
status: investigating
tags: [investigation, 9p, security]
---
# Authenticated attach transport state and Factotum handoff

## Scope

Begin [t317](../plans/jsschoen-converge-public-roles-on-one-9p-core.md#t317)
with the transport state prescribed by the approved
[t133 handoff](../plans/esnqxpqw-pidfd-and-namespace-admission-optimizations.md#local-authentication-design-and-t317-handoff-2026-10-10).
The initial candidate is uncommitted on `dev/t317-admission-state`, based on
`e855fd75acd8ac38b7b779cc5377acd5d2c0c223`. It changes no live session.

## State model

`sophia-9p::authentication` keeps transport identity outside wire-reset state.
It permits one attempt at a time and eight starts over the connection lifetime.
Attempts carry server, connection, session, audience, wire generation, attempt
generation and afid. A foreign or late result cannot consume a live attempt.
Cancellation burns its start; reset invalidates verdicts without unpinning
principal, namespace or launch. First attach and attempt deadlines cannot be
renewed by activity or reset. Transcript charging bounds aggregate bytes before
forwarding. A backwards owner clock closes the state rather than extending it.

Its `verified` input is a
trusted, supplied verdict, not an authentication proof. The owner must reserve
quota and root/reply capacity, check current launch custody and policy, then
publish the root without a fallible step after committing the identity pin.
No role has been migrated; existing peer checks remain unchanged.

## Executed evidence

The complete `sophia-9p` crate suite passes: 131 tests, zero failures or ignored,
across 13 summaries. Five new tests exercise reset/pinning, every correlation
field, deadlines/budget, cancellation/oversize/revocation and monotonic time.
All-target crate Clippy passes with warnings denied. Logs are retained under
`~/.local/state/sophia/development-evidence/t317-admission-state-01`.

The serial control adds `self.pinned = None` to reset. It exits 101 with one
failed test, at `reset must preserve the pinned identity`; the same transport
would otherwise accept a changed identity after reset. Exact original source
bytes were restored. The separate control package is
`~/.local/state/sophia/development-evidence/t317-admission-state-control-01`.
No binary was copied into evidence. This control proves the state oracle, not
the real Tversion/Factotum path, which is not joined yet.

## Remaining implementation

### Candidate wire join

The uncommitted SDK branch `dev/t317-afid-records`, based on `a111b55`, adds
Tauth/Rauth records and pipeline authentication/attach-with-afid operations.
The core dispatches these through opt-in export hooks. Authentication fids are
conversation handles, excluded from ordinary walk/open/getattr and in-place
staging. Defaults continue to refuse authentication. Wire reset releases afids
and notifies the owner; transport closure has a separate notification.

Four wire tests cover trusted-verdict-only attach, claimed-name refusal,
reset/stale-afid behavior, waiting-read flush and revocation, root capacity
before identity pinning, and the actual Rust SDK pipeline talking to the Unix
server. Verdicts and live custody are supplied by the test owner. This is not
a Factotum or protected-process authentication claim.

The current core suite passes 135 tests across 14 summaries; the candidate
SDK workspace passes 343 tests across 38 summaries, with no failures or ignores.
Both all-target Clippy checks pass with warnings denied. These development runs
use command-line Cargo path overrides for the two SDK crates; the immutable
vendor tree is untouched. The initial Clippy invocation placed `--config`
before the subcommand and compiled the old records, failing with missing Auth
variants. Moving `--config` after `clippy` checked the intended candidate.
This is not evidence for a reviewed/published SDK pin or the full repository.
The wire package is `~/.local/state/sophia/development-evidence/t317-afid-wire-01`.

### Factotum nonce and private control exchange

The candidate now includes Factotum's bounded launch authority and a separate
Session-only `sophia-launch-v1` attach view containing one exclusive `rpc`.
The ordinary 9front file tree and its bytes remain unchanged. The new view
cannot reach key administration or PAM. See the
[wire contract and ASCII owner diagram](../../factotum-launch-authentication.md).
The child must never receive this owner view.

Registration reserves an increasing launch generation; only an exact activation
can make it eligible. Factotum generates a 256-bit nonce with nonblocking kernel
randomness, matches Session's channel attestation to its registered audience,
and consumes the nonce once. All seven attempt fields correlate result delivery
and cancellation. Deadlines also cover undelivered results; backward owner time
closes the authority. The bounded table retains pending work and results, with
per-launch/principal limits applied only after registered identity is known.
Session still owes its separate admission and delivery reservations.

The complete core and Factotum crate suites pass 195 tests (135 core, 60
Factotum), zero failures/ignores, across 27 summaries. Eleven new Factotum tests
cover owner state, named quotas, whole-record refusal and the real 9P
server/pipeline exchange. Clippy with warnings denied, formatting and diff
checks pass. The full repository gate remains outstanding. This run still uses
the local SDK overrides; it is not evidence for a published/vendor pin.

The separate negative control removes the activation guard. Its private 9P
exchange fails first at `the private exchange must not verify a Reserved launch`,
exit 101 and exactly one failed test. The source is restored to SHA-256
`816baa2f4012a35f2ff312d9a0c5f1493b8ae24246fedbaf01aa7b81518d9a3e`;
all candidate file hashes in both worktrees are checked before/after. The
positive run and control use the same exclusive worktree lease, sequentially.
The restored exchange passes again. No binary is copied into evidence.

Packages: `~/.local/state/sophia/development-evidence/t317-factotum-launch-01`
and `t317-factotum-launch-control-01`; both manifests verify. The positive
manifest SHA-256 is
`1f676021e9c882ba22c148074b81d7df0f6b42178bca64e814be16f5e73f7f0f`.
Candidate manifests capture the complete modified/untracked file sets before
execution. This investigation update follows the run; source bytes are unchanged.

Channel class, registration and activation are supplied in these tests.
Factotum's nonce and verdict are real, but no protected spawn, child relay,
Session final custody check or role admission is qualified. Entropy failure
is refused in source but not fault-injected. The Factotum-side relative deadline
does not establish Session's absolute adoption/attempt deadlines.

### Protected launch channel primitive

`LaunchChannel::reserve` now provisions a fresh pair, with a non-cloneable child
handoff outside the reusable process spec. `ProcessSupervisor` consumes it on
success or failure, burns its generation, and records only its own successful
spawn's generation beside protection evidence. Another reservation cannot
relabel an existing child. Opting into this path prevents an ordinary restart
from silently omitting its fresh channel. Unprotected starts and a configured
descriptor-number claim are refused.

The new path clears the version probe and launch environments. Child-only
descriptor hooks mark everything above stdio CLOEXEC, then retain only the
designated channel for the final launch. The parent drops its child-end copy
immediately after spawn. Both pair endpoints are promoted above stdio if a
caller had closed a standard fd; this rare branch is source-reviewed, not
fault-injected. Existing non-authenticated starts retain their previous path.

An independent C descriptor probe runs through the production supervisor and
Bubblewrap 0.13.0. It inventories descriptors, uses the connected channel, checks
ordinary exec exclusion after setting CLOEXEC, and lets the parent verify EOF
after exit. A deliberately inheritable sentinel is absent in the sandbox while
its parent flags remain unchanged. Failed spawn and reused generations refuse;
a later fresh generation works. These are descriptor/custody observations, not
a C authentication exchange or SDK adoption proof.

The Linux-peer and runtime suites pass 497 tests, zero failures, with 18 ignored
and 64 summaries. All-target Clippy with warnings denied, formatting and diff
checks pass. The serial negative control omits the common non-stdio exclusion:
the C probe reports `unrelated inherited descriptor`, and the test fails first
at `handoff must expose only the designated channel`, exit 101 and one failure.
All captured source/fixture hashes in both worktrees match after restoration;
both handoff tests then pass again. No build artifact is copied into evidence.

The closed packages are
`~/.local/state/sophia/development-evidence/t317-launch-handoff-01` and
`t317-launch-handoff-control-01`. The positive manifest SHA-256 is
`dddb9e688c536f66c867a117b7e511c4ea8bdf9d023f5b757727953300c90e34`.
The Linux ABI source is restored to
`c74cd806220aac4d7360ac7cab4202ab0c499953e06b2adddc053ba00776c111`.
The candidate capture precedes execution; this tracking update follows it.

Bubblewrap's minimum supported 0.11.2 descriptor path was read from upstream
source, not executed. That version still needs execution before enabling the
role path. This supervisor primitive also retains the current synchronous
spawn/protection confirmation; it does not establish nonblocking Session
scheduling. No live desktop, installation or device was changed.

Next connect Session's reserved/active relay to this handoff and Factotum, then
each role's admission. Global quotas,
fair progress, first-attach transport closure, claimed-name matching and final
revocation checks belong to those joins; the local state alone does not prove
them. Full repository and external client gates remain outstanding. t317 stays
open and no SO_PEERPIDFD or expected-peer check is removed.

### Nonblocking private control client

The next integration boundary is the existing `UnlockClient`: it owns a
pipeline and waits through a PAM conversation on the unlock supervisor thread.
Giving another reader that same socket would break tag ownership; making
launch work wait behind PAM would break authentication progress. The new
`launch::client::LaunchClient` instead borrows a caller-owned pipeline to queue
one step, and consumes only its own tag's response. It performs no socket wait,
poll or thread creation. The existing Session authenticator is unchanged.

One command/result slot remains occupied until consumption. Pipeline pressure
preserves the next unsent read without resending the command. Setup, writes,
reads and buffered results share their original absolute deadline. Begin's
remaining duration is clamped before queueing. A fatal uncertain exchange
requires the connection owner to close the shared transport and retire its
delegations; that integration is not yet present. Exact attempt correlation
is checked before exposing a verified result. Session still owes the final
identity, live custody, recipe and attempt-deadline checks.

Five new tests run against the actual 9P core and Factotum export. They complete
the launch exchange while a PAM read on the same connection remains pending,
then deliver a supplied rejection to that PAM tag; retain an unread result and
expire it; refuse a late write reply; refuse another attempt's otherwise valid
verdict; and recover from pipeline request capacity without repeating Initialize.
The Session class, delegation and PAM outcome are supplied. No live PAM,
Session coordinator, protected child relay or final attach is exercised.

The core and Factotum suites pass 200 tests, zero failures and zero ignored
across 28 summaries. All-target Clippy with warnings denied, formatting and
diff checks pass. These remain development SDK path-override results, not an
integrated immutable SDK pin. An initial test compile used the private PAM
module path and the wrong Wake method; both fixture calls were corrected before
the retained run.

The negative control replaces full attempt equality with server-only equality.
It fails first at `a verdict must name the exact requested attempt`, exit 101,
one failure. The source was restored to SHA-256
`eefccfed75271b7bec7b10153b55c457c70d5cb2ae6acf5be591cd12fb8791f4`,
all dynamically captured modified/untracked files in both worktrees were
rechecked, and the five tests passed again. Positive and control packages are
`~/.local/state/sophia/development-evidence/t317-launch-client-01` and
`t317-launch-client-control-01`. Both manifests verify; the positive manifest
SHA-256 is `7e298ac4133d49f2d6cd5be517ed3844579e3684a2d02809dd30e460081b0eae`.
Only paths and hashes identify test binaries; none were copied into evidence.
This documentation update follows the captured candidate.

Next integrate a single nonblocking Session connection owner with PAM and the
provisioned child relay, preserving bounded request/result reservations and
closing failed transports. t317 remains open; nothing was committed, merged,
installed or changed in the live desktop during this slice.

### Shared connection owner and nonblocking unlock supervision

`SessionClient` now owns the ordinary PAM and private launch views on one
pipeline. The previous blocking PAM exchange has become a bounded state machine;
the compatibility API drives that same machine instead of maintaining a second
implementation. Both tag owners advance without socket waits. Fatal errors
drop the pipeline, buffered results and zeroizing secrets; this now establishes
actual transport closure, rather than merely requiring it from a future caller.

The concrete Session authenticator uses the shared client. Its supervisor now
begins and polls logins, continues turning while PAM waits, and notices owner
drop before a PAM verdict. Worker turns use a 1–20 ms cadence, with process
liveness and restart backoff retained separately. This is a scheduling choice,
not a latency result. Startup still performs bounded blocking negotiation on
that supervision thread. One active login is allowed; a concurrent attempt is
answered Unavailable rather than starting a second conversation.

The old unbounded unlock result channel is replaced by two reserved result
slots. Reservations include queued, active and undelivered attempts and end
only at Session consumption. Sends never block; input size is checked before
copying into the queue. PAM's smaller password limit remains a Rejected login
at the Session adapter, not an unavailable/restarted agent. Launch verification's
global queue and its 32-slot accounting are still unimplemented; this result
must not be represented as that obligation's closure.

Eight new shared-client tests use the actual 9P core and Factotum export with
supplied Session class, delegation and PAM outcomes. They verify launch progress
while PAM waits, subsequent accepted/rejected login, both timeout directions,
actual socket closure and PAM cancellation, peer loss, user/service refusal,
retained result slots, local input refusal and 130 synchronous compatibility
logins without exhausting fids. An initial automatic-outcome fixture omitted
the server wake and timed out in three tests; the fixture was corrected to wake
the owner as the production job pool does. No production workaround was made.

The unlock supervisor's eight tests include two new cases for progress while
a scripted login remains pending and admission while verdicts remain undelivered.
The concrete native-session adapter compiles; the supervisor tests substitute
agents. This is not a live PAM helper or physical lock acceptance run.

Final selected results: core/Factotum 208 passed in 29 summaries, Session lock,
lock-input and unlock-supervisor 32 passed in three summaries, SDK pipeline 29
passed. Total 269, zero failures and zero ignored in these final runs. Clippy
passes with warnings denied for core/Factotum all targets, the native Session
library and three selected tests, and the SDK pipeline test. Formatting and
diff checks pass. Development SDK path overrides remain in use; the public SDK
pin and full repository gate are not refreshed.

The final full core rerun initially exposed a pre-existing fixture assumption:
`a_closed_server_surfaces_as_an_io_error` allowed BrokenPipe and EOF, but the
kernel returned ConnectionReset. A standalone socketpair observation reproduced
ECONNRESET when a peer closes with unread request bytes. Both copies of the
fixture now accept that I/O error while still requiring a poisoned pipeline.
Production transport code is unchanged. The failed log is retained as
`core-factotum-failed-01.log`, and the corrected full run passes. This is a
separate fixture repair, not an authentication behavior change.

Two serial negative controls discriminate the new boundaries:

- Retaining the pipeline after a fatal deadline fails first at
  `fatal authentication failure must close the actual transport`, based on the
  server still holding the connection, before the client's closed-state check.
- Admitting three unlock attempts against two result slots fails first at
  `undelivered verdicts must retain their admission slots`.

Both controls exit 101 with one failed test. Source and all captured fixture
hashes are verified after restoration. Restored owner source SHA-256 is
`1126cfab16d11f56bbc057268fd3b110ff311e3f151b8cf1ec77ef57830d79ef`;
restored supervisor source is
`d61613fc9ed969e689f77de98f6f911bdecc842b6d8b2bd3e0b72914fb9d3775`.
The restored positive runs pass. No target or test binary is copied.

Closed evidence packages under `~/.local/state/sophia/development-evidence/`:

- `t317-shared-owner-01`, manifest SHA-256
  `394793eb9470c88f33fc2be61e6577bfd35896b1362807f3076db8247a4be3f4`.
- `t317-shared-owner-transport-control-01`, manifest SHA-256
  `3b9c2de0331fcdd731fef8c10b81dc28b018397d3948dc87deee1eb3f3e71f9b`.
- `t317-shared-owner-capacity-control-01`, manifest SHA-256
  `0a8a6dfb17ffaaece30c7c81119a808308f61490ddd5510ea9e2ede762e72f13`.

The packages' self-excluding manifests verify. `candidate.json` dynamically
captures every modified/untracked file in both worktrees before the controls;
`candidate-final.json` binds the final runs and differs only in the two pipeline
fixture files repaired afterward. Both complete path sets and final hashes were
checked. This documentation update follows those captures.

Next connect the provisioned child relay and bounded verification ingress to
this owner, then join protected launch activation and final role attach checks.
No child can use the new mechanism yet. No role's credential or pidfd filter
was removed. t317 stays open; no commit, merge, install or live desktop change
was performed.

## Provisioned 9P relay and bounded verification ingress

The next candidate adds `launch::queue` and `launch::relay` in Factotum's
library, used on the Session side. The relay adopts a private socketpair endpoint
through the actual 9P core, serves only one exclusive `rpc` conversation, and
adds its owner-selected launch generation to the challenge. No process identity
is supplied by the child. Reserved state waits within a fixed five-second
deadline; matching activation, revocation and expiry wake/service pending reads.
Factotum's separate original attempt deadline remains authoritative.

The queue has 32 global reservations covering queued, active, unread-result and
cancelling work. It is nonblocking, creates no thread and uses the existing
SessionClient connection. Dropping a queued ticket prevents execution. Dropping
a successful Begin or Reserve/Activate ticket schedules Cancel or Retire while
retaining its slot. Cleanup has its own five-second exchange bound, without
renewing any authentication deadline. Dropping the connected owner closes the
actual transport and prevents consumption of buffered results. Full ingress
does not stop an admitted PAM login on that connection.

Initial development tests exposed two fixture/integration issues: the fixture
must give the owner a turn to reclaim a consumed initialization slot; and
completion of a queued result must wake the relay's pending 9P read. The queue
now supplies completion/closure wakes. The relay wrapper independently services
Reserved expiry, including when both peer and Factotum are silent. No retained
passing evidence claims the initial failing candidate.

Restored positive core/Factotum gate: **220 passed, zero failed/ignored across
29 summaries**. This includes 12 new joined queue/relay cases plus the earlier
eight shared-client cases. Clippy with `-D warnings` passes for both crates and
all targets. Development SDK path overrides remain in use: this is not the
final immutable SDK pin or full repository gate.

Two single-site controls discriminate:

- Admit a 33rd reservation: first failure is `unread verification results must
  retain every admission slot`.
- Bypass Session relay's Reserved guard while Factotum is Active: first failure
  is `Factotum activation alone must not bypass Reserved Session custody`.

Both exit 101 with one failed test. The restored positive gate ran after both.
The stable worktree lease covered controls and builds; all dynamically captured
modified/untracked files in Sophia and the SDK matched before/after. Restored
queue SHA-256 is `d3521bdf6a733194c652dcadf60af78ec9a3e32a16360aab2354f09c71de094d`;
relay SHA-256 is `9e3623dc776666dc38716e3f94c0fc4911929a5044be361f163850b8efd2d887`.
The documentation update follows that source capture.

Closed packages under `~/.local/state/sophia/development-evidence/`, with
self-excluding SHA256SUMS hashes:

- `t317-relay-owner-01`: `adf9ee66e64ad2577d246b458f6a2d6a1788329f91e68724213eabcdab16e924`.
- `t317-relay-capacity-control-01`: `99bbe292e78dedbc897d3f6a6da60672ed65b26af1a7c3c4659235b1875df267`.
- `t317-relay-reserved-control-01`: `ab7f2da0495bdf6d482585bba5e1043442055c2a50ab5f813908e53628ae367c`.

These tests use actual wire/core/Factotum owners with supplied Session class,
launch activation and PAM outcomes. They do not join a protected child, final
role attach or independent C authentication. No role is enabled and no pidfd or
credential check is removed. Next connect the launch lifecycle and registration
to the shared supervision owner, then enforce verified identity plus current
custody at role attach. The live session is unchanged; t317 remains open.

## Session launch registry and protected relay join

`LaunchBook` now joins Session-selected policy, Factotum registration, the
single-use protected spawn reservation, relay activation and final live-custody
checking. It burns generations before fallible setup, retains retiring records
until Factotum acknowledges retirement, and closes the shared ingress if cleanup
cannot finish within its bound. Local revocation drops the channel and relay
before any remote cleanup. A verdict must match the recorded identity and
audience; custody independently requires the exact reservation and role, not
merely an equal numeric generation. A requested termination withdraws custody
before process exit.

An independent C client runs through the real Bubblewrap-protected supervisor.
It speaks 9P2000.L on the inherited channel, submits Factotum's actual nonce,
and keeps the protected child alive while Session checks the resulting verdict.
The C client implements its own framing, sets CLOEXEC on adoption and has a
15-second process deadline. Its nonce is supplied through fixture arguments;
it does not yet read that challenge from a public role afid. No password or
bearer grant is involved. Session class, launch policy and the afid attempt
tuple remain supplied; Factotum nonce/verdict, child relay, spawn and custody
checks are production paths.

Four registry tests cover this join, substituted spawn with the same generation,
failed-spawn retirement/generation burning, and the 64-record capacity including
unacknowledged retirement. The shared authentication supervisor now publishes
its current launch ingress separately from the PAM interface. Replacement
invalidates the previous ingress; owner drop prevents even a late startup from
republishing authority. Two added supervisor tests cover those boundaries with
scripted agents. The concrete Factotum adapter now drives the launch queue and
PAM through the same connection owner. The desktop owner loop does not yet
consume the published service or create role registrations.

Restored validation: **258 passed, zero failed/ignored across 33 summaries**
(core, Factotum, registry, lock/input and unlock supervisor). Native Session
library and selected-test Clippy pass with warnings denied; formatting and
diff checks pass. An initial fixture compilation used an unused import and
attempted to format a non-Debug response; both test-only mistakes were corrected
before the retained run. SDK development path overrides remain in use.

The single-site control replaces exact reservation comparison in
`ProcessSupervisor::has_live_launch` with numeric-generation comparison.
It exits 101 with exactly one failure, first at
`matching generation numbers must not substitute another spawn's custody`.
This detects the incorrect activation synchronously, before waiting for a
verdict. Source was restored to SHA-256
`b1b5aead46669cc7027d93f12989d223fdd5a302a9d36ce2e2915926695db56d`.
The dynamically captured modified/untracked file sets in both worktrees match
before and after the control; the positive suite ran after restoration.

Closed packages under `~/.local/state/sophia/development-evidence/`:

- `t317-launch-book-01`, SHA256SUMS SHA-256
  `8157c04023fc24b5af8fc31cc2592b557cfbc547c748f2730ef034d68d7cfe08`.
- `t317-launch-book-custody-control-01`, SHA256SUMS SHA-256
  `ce2b564cbe747a5ed01e49643d0017706aba064a9e5bedb2e6127a48212a3db2`.

Both self-excluding manifests verify. Executables are identified by path and
hash, not copied. This note follows the candidate capture. The supported minimum
Bubblewrap version, final public-role afid attach, global endpoint admission,
role migration, immutable SDK pin and full repository gate remain outstanding.
No t317 changes are installed, no existing peer/pidfd check is removed, and
the worktree remains uncommitted while the integration is incomplete.

## Correlated afid owner and final custody checks

The candidate `RoleAuthentication` now connects the transport-lifetime state
to Factotum Begin/Take/Cancel and the launch registry. It retains one bounded
conversation and cancellation, serves a fixed challenge and status, checks the
exact verdict tuple, and requires current custody both before qualifying a
verdict and before committing attach. Every retained role operation has a
separate current-custody check. Reset preserves the transport pin and original
first-attach deadline. Exhausting the transcript budget queues cancellation of
that exact attempt. Failed cleanup invalidates the shared ingress instead of
releasing a slot while remote work remains uncertain.

The wire join uses the real 9P server and Rust SDK with a minimal supplied role
view. The SDK reads its challenge from its afid; the fixture passes that public
nonce to the protected C relay peer from the previous slice. Factotum produces
the verdict and Session verifies actual spawn custody. This deliberately
exercises delegation within one launch, not an independent C implementation
of Tauth or a migrated shipped role. Fixed claims, Session control admission,
the role view and policy remain supplied.

Four new tests prove ordinary/early attach refusal, forged-name and
peer-asserted-success refusal, authenticated role access, retained-fid
revocation, revocation between status and attach without partial pinning,
first-attach expiry despite reset, and the bounded challenge-reread path with
actual Factotum cancellation. The existing core wire tests retain the Tversion
and immutable-identity controls; this slice does not claim a new full
reauthentication-across-Tversion join.

Restored positive results: **262 passed, zero failed/ignored across 33
summaries**, including core, Factotum and selected Session suites. Native Session
library and selected-test Clippy pass with warnings denied. Formatting and
diff checks pass. Initial fixture attempts incorrectly treated pipeline error
replies as transport errors; correcting the test to expect `Reply::Error`
required no production change. Development SDK path overrides remain in use.

Two single-site controls fail at their predicted first assertions:

- Cache custody after the successful status: `verified status cannot bypass
  revocation before attach`.
- Return the cached identity for retained operations without checking custody:
  `a retained role fid must lose authority when its launch is revoked`.

Both exit 101 with one failed test. Restored role-owner SHA-256 is
`80f253911dc2f9d42433d74b16f06b8ed1374617a8d0a6052b6de372ae7f8907`.
The exclusive lease covers mutation, builds and restoration. Complete dynamic
modified/untracked captures in Sophia and the SDK agree before/after; the
positive suites run after both restorations. No binary is copied into evidence.

Closed packages under `~/.local/state/sophia/development-evidence/` and their
self-excluding SHA256SUMS hashes:

- `t317-role-afid-01`:
  `0210342e157109ca01a087c286e8984d4d4450b5a427c8fdeb7433b6e11041c7`.
- `t317-role-afid-attach-control-01`:
  `2fd1d1fbfd7e06a5d650a9a07a068f55baa82dbad415ffa75215e988d67dcb8a`.
- `t317-role-afid-retained-control-01`:
  `5ce28742a5137becbeefe942134361bfb9a97fea4973bf808d2eacdb34e57ee3`.

Next integrate this owner with the shipped lock export's atomic root
publication and transport lifecycle, then the other role owners and endpoint
quotas. Minimum-version Bubblewrap execution, independent full authentication,
SDK pinning and repository/external gates remain required before role enabling
and merge. No credential or pidfd requirement has been removed, no t317 code
is installed, and the task remains open.

## Reproducible foundation checkpoint

The foundation now builds without development path overrides. Its immutable
SDK pins are signed commits published on the respective development branches:

- Rust SDK `5a1143816973d6bd1f758ee9fafc9dedb5e8421d`
  (`dev/t317-afid-records`): Tauth/Rauth records and pipeline operations,
  scripted-peer controls, and the candidate profile. Public enums gain
  variants; this is a development revision, not a compatibility release.
- C SDK `1a37b2da8e09f6dbd56f9c0fdabc6d425c217046`
  (`dev/t317-profile`): matching profile, digest and coverage/provenance text
  only. The C client still does not implement full authentication.

The Rust SDK's standalone workspace suite passed **343 tests, zero failed or
ignored**. The vendor command required one final bootstrap with the old path
override because the candidate core already names the new records. Subsequent
validation uses only the pinned archives. The first repository attempt stopped
at the C SDK's stale profile copy, before workspace tests; its STOP package is
retained as `t317-foundation-checkpoint-01`. Updating that contract copy did
not change C implementation bytes.

The restored repository gate, `cargo xtask check`, passed with **7,686 tests,
zero failed, 105 ignored across 556 summaries**, plus Clippy, SDK integrity,
profile/layout, wire and evidence-reader checks. It ran in private namespaces
with devices and live display/runtime sockets hidden, under the exclusive
worktree lease. The two hardware proof families correctly report not proved
without devices. Ignored and independent acceptance gates are not qualified
by this run. No path override is used. The SDK check now uses the caller's
leased target root directly instead of creating a separate target subdirectory.

`t317-foundation-checkpoint-02` contains the complete dynamic modified/untracked
file inventory, commands, source-unchanged comparison and log. Its closed,
self-excluding SHA256SUMS has SHA-256
`9a1c08eb9f4a488a3f1a6e6fb696a10ee35baf0ab96b1f4dbeb80ec70830aac5`.
This result paragraph and the plan link follow the source capture; production,
test and SDK bytes are unchanged. The signed checkpoint binding is retained
separately in `t317-foundation-checkpoint-binding-01`, with executable paths and
hashes rather than copied binaries.

This is a foundation checkpoint, not t317 completion or merge approval. The
next production join is atomic authenticated admission in the shipped lock
export, followed by role transport lifecycle and endpoint quotas. Supported
minimum Bubblewrap execution, independent full authentication, final role
controls and acceptance remain open. Existing peer/pidfd checks remain enabled;
no checkpoint code is installed in the live desktop.

## Atomic lock admission owner

On foundation `9f4494295`, the actual `LockFileExport` now separates checked
root preparation from infallible publication. Preparation borrows the export
exclusively without setting its attached flag. Session's `attach_lock` checks
claims and prepares that root before committing identity; failure drops the
preparation. `check_lock` combines current launch custody with the export's
existing epoch/custody checks. The legacy attach path uses the same preparation
and preserves its policy and one-attach-per-epoch behavior.

Four added tests use real Factotum, the existing independent C relay and a
protected process with the LockProvider role. They directly call the Session
admission owner and actual lock-file export: unverified admission leaves both
root and identity absent and permits a verified retry; a revoked lock view
cannot pin a verified identity; revoking the launch after status leaves the
prepared root unpublished; and a retained lock object loses authority even
while file custody itself remains live. Reading the decoded Locked object is
a protocol observation, not a physical cover claim.

Three single-site controls fail first at their named assertions:

- Publish during preparation: `unverified lock attach must not publish its root`.
- Commit identity before preparing the view: `lock view refusal must precede identity commit`.
- Skip the admission check on retained operations: `retained lock files must recheck launch custody`.

Each exits 101 with exactly one failed test. The same exclusive lease covers
mutation, compilation, restoration and the final positive gate. All dynamic
modified/untracked hashes match after each restoration and after the gate.
No binary is copied; paths and hashes are retained.

After restoration, the full isolated `cargo xtask check` passes **7,690 tests,
zero failed, 105 ignored across 556 summaries**, plus Clippy and repository
checks. Devices and live-session sockets are hidden. Hardware proofs remain
not proved, and the ignored acceptance cases retain their separate scope.

Closed packages and self-excluding SHA256SUMS hashes under the development
evidence directory:

- `t317-lock-owner-01`:
  `8050b9d6b79610e76e0e642c6f730045678dc5211645e50abe3d27a97681bbbb`.
- `t317-lock-owner-root-control-01`:
  `9a1582df416f5f5c8551fc739d88a8e0b1dd57d37b9d4b31eb2ca53c5e65ff18`.
- `t317-lock-owner-identity-control-01`:
  `48efd192a48ba5171f7f014a73d6f7a99e4fadf36f43ad1b8ffa7ddba128fa9c`.
- `t317-lock-owner-retained-control-01`:
  `de2a7703250de917efce16a6b83e03a43a29b3877df5ddf1afac0fc1221e4f3c`.

The result documentation follows the frozen source capture. This is an owner
join, not a migrated public lock endpoint: the lock worker still owns a plain
`Server<LockFileExport>` on another thread, whereas Session owns the launch
registry and supervisor. Next wire the authenticated worker without blocking
Session or substituting cached verification for live custody, including resets,
idle deadlines and disconnected cleanup. Per-fragment upload and waiting-read
dispatch need that integration's controls. Existing accept filters and the
hard SO_PEERPIDFD requirement remain; t317 stays open and nothing is installed.

## Worker launch custody

On `a372f464d`, Session can now hand current launch authority to a role worker
without sharing mutable supervisor or registry state. `WorkerAuthority` is
created only for an Active registration and its exact protected spawn. Every
qualification, final attach and retained-file check matches the verdict's
session, identity and audience, then checks registration, Factotum connection
and kernel process lifetime. The existing role admission owner accepts either
direct Session authority or this worker authority; identity pinning and atomic
root publication retain one implementation.

`ProcessSupervisor::launch_custody` captures pidfds for both its unreaped
wrapper and the checked protected peer. Zero-time polls observe spontaneous
exit without Session polling or reaping. A shared flag withdraws custody before
termination signals and on child retirement. Session's separate registration
flag is withdrawn before remote Factotum cleanup. Connection loss invalidates
the shared ingress. No clone can revive any of these authorities. This uses
the existing `pidfd_open` platform support, not `SO_PEERPIDFD`; the live lock
endpoint's old checks have not been removed. The
[contract diagram](../../factotum-launch-authentication.md#current-launch-authority-across-workers)
shows these separate owners.

The focused runtime suite passes four tests, and the Session authentication
suite passes fourteen. New runtime cases reject another reservation with the
same numeric generation and a wrong role, observe termination before reap,
reject revival by another live spawn, and observe natural exit without an
owner poll while leaving the exit status for its supervisor. One new Session
case moves the real admission and lock-file owners to another thread after
real Factotum verification and protected relay. A local revocation is visible
there without a worker command or remote cleanup poll. A second case proves a
lost Factotum connection cannot publish a prepared lock root or identity.

Three single-site controls fail at their named assertions:

- Omit supervisor revocation: `termination must revoke worker custody before reap`.
  Both processes are stopped and alive while TERM is pending, so kernel exit
  cannot mask the missing flag.
- Ignore the kernel poll result: `worker must detect kernel exit without Session progress`.
- Omit Session registration revocation: `worker retained files must observe local revocation without a worker command`.

Each exits 101 with exactly one failed test. The exclusive worktree lease
covers the positive checks, mutations, compilation, restoration and final
gate. A dynamic inventory captures all eleven modified/untracked candidate
files, including the normative contract, and matches after every restoration
and after the gate. The investigation and plan result follow that capture;
code, tests and contract bytes are unchanged. Executable paths and hashes are
retained, with no copied binaries.

The restored, device-hidden `cargo xtask check` passes **7,694 tests, zero
failed, 105 ignored across 556 summaries**, plus Clippy, SDK integrity and
repository checks. Hardware proofs remain not proved on the isolated host;
ignored acceptance tests are not counted as executed.

Closed packages and self-excluding SHA256SUMS hashes under the development
evidence directory:

- `t317-worker-custody-01`:
  `3521f175ba18a14af4cf698618bca0a041d30c2d1bc7e86ee2b1a76c8860f0e3`.
- `t317-worker-termination-control-01`:
  `9d093ea7e7ce77c57812fbc795b3de8720ac939edc60f79a14c51285e9d86465`.
- `t317-worker-exit-control-01`:
  `58ed2a962757f3eb24928602e048c257f812572735ff441099c35398f87ce429`.
- `t317-worker-revocation-control-01`:
  `18d099bd88eaa1d9303f03db32920f44e0a31ed0d49fd71b48c08538c811384f`.

This qualifies the cross-thread authority handoff through direct owner calls,
not authenticated dispatch in `LockFileService`. The live transport still
needs afid dispatch, reset and idle deadlines, disconnected-conversation
cleanup with bounded slots, and checks on retried reads and in-place upload
fragments. Other role integration, minimum Bubblewrap execution, independent
authentication and final acceptance remain open. No live desktop changes,
merge or t317 completion are claimed.
