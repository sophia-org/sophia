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

## Authenticated lock wire and bounded cleanup checkpoint

The runtime now wraps the shipped lock-file owner in `AuthenticatedFiles`.
Session supplies a factory carrying current protected-launch custody and one
shared transport budget. Root preparation precedes immutable identity commit;
publication is infallible. Pending construction cannot fall back to legacy
attach. The explicit authenticated mode removes only its socket-pidfd check;
the availability filter and supervisor-owned launch checks remain. Desktop
startup still selects legacy admission and has not changed.

A protected independent C peer uses raw 9P2000.L against the real lock worker,
reads its own afid challenge and relays it through the provisioned channel.
Real Factotum verification admits the root. The peer proves ordinary attach,
forged claims and unverified attach are refused; Tversion invalidates the old
afid and requires fresh authentication through the same launch channel. It then
negotiates the lock files and loses a waiting read on Session revocation. This
is independent wire evidence, not SDK authentication qualification.

The shared core now bounds pre-attach fids and pending/undrained requests to
four. Session's factory reserves eight transports per persistent endpoint and
32 across endpoints, retaining slots through acknowledged remote cancellation.
The quota join uses real launch custody and Factotum, with supplied transport
creation. Raising either quota or the pre-auth request limit causes its named
first assertion to fail. Each control exits 101 with one failed test, and all
candidate bytes are restored and compared under the exclusive worktree lease.

`t317-lock-wire-01` records seven core wire tests, sixteen Session launch tests,
and the full device-hidden gate: **7,700 passed, zero failed, 105 ignored across
557 summaries**, plus Clippy and repository checks. Its complete dynamic
candidate inventory covers modified and untracked files before the run.

Review then found that a queued connector could keep listener readiness hot
while all eight retirement slots prevented acceptance. The follow-up excludes
listener readiness until cleanup frees a slot. A supplied pending-cleanup
fixture with a real queued socket proves the worker waits for the owner wake;
removing the condition fails `full authentication cleanup must wait past queued
listener readiness`. After restoration, runtime lib/service/transport tests,
all sixteen Session launch tests and runtime Clippy pass. This follow-up is
recorded separately in `t317-auth-wait-01`; the earlier full gate is not claimed
to have run its later bytes. Documentation follows both source captures.

Closed packages under the development evidence directory, with SHA256SUMS
hashes (self-excluding manifests, verified; executable paths/hashes only):

- `t317-lock-wire-01`: `4830e77a01b3c280b399c5116d7f96ef521a10e6fe07708bc522918e3065f81a`.
- `t317-auth-endpoint-control-01`: `12b3c3d833140300db356e0d683298b2b116248f043981c93237970a6a9a4fb6`.
- `t317-auth-global-control-01`: `5165c73b8757359ce3dbbf2bd24a5b8185aeea03eddb30d410db9fef0f7e8666`.
- `t317-auth-requests-control-01`: `0f9c053c61faa6dd7a614e5e21793c5d2f3358b56c0dac865a9ed8a6eb6d7dd5`.
- `t317-auth-wait-01`: `4b90ffd43df42afcd5c9a1d46c7c2c3d13bebf6db21be49b750629b835edd605`.

t317 remains open: normal Session startup must consume the shared launch
service, SDK clients need the conversation, other required roles need the
same admission path, and in-place fragment controls, minimum Bubblewrap
execution and final acceptance remain. No merge, installation or completion
is claimed by this checkpoint.

## Independent C SDK afid interoperability

C SDK signed revision `2160218019841d2751c58ee74e9d20ba665dc313`
adds Tauth and explicit attach-with-afid to the generic client. It validates
QTAUTH only on Rauth, reclaims failed or flushed auth reservations, and retains
an afid when Rauth wins the flush race. Ordinary attach still sends NOFID.
The contract copy is unchanged. The strict C suite and clang ASan/UBSan suite
pass, with controls for accepting a non-auth qid and leaking a refused auth
reservation. `t317-sdk-c-afid-01` records those results; its SHA256SUMS hash is
`e277c561ca483e52e07364aef71fb26dfbcd1d7c9845e00745340bedb202d362`.

Sophia pins that exact archive and signed commit, with its offline tree/blob
and contract verification passing. A second build of the protected lock peer
uses the SDK for version, Tauth, challenge/status reads and authenticated
attach. It reauthenticates after Tversion using the same protected launch
channel, then reaches lock negotiation and loses its waiting read on Session
revocation. The existing independent raw-wire build also passes. The fixture
still drives the private relay and subsequent role-file records itself; this
does not claim SDK role bootstrap or a reusable SDK launch conversation.

A single-site control sends NOFID instead of the verified afid from the SDK.
The C peer fails its expected Rattach assertion, and the exact joined Rust test
exits 101 with one failure. Restoration is byte-checked, and the immutable
snapshot verifier passes again. All **17 Session launch-authentication tests**,
the existing legacy lock C SDK export test, focused Clippy and formatting pass.
This delta changes the pinned C client and fixtures, not Session/runtime
production owners. The earlier full gate remains evidence for its own
candidate; it is not represented as a rerun of this later SDK pin.

Closed, self-excluding manifests under the development evidence directory:

- `t317-sdk-lock-01`: `9a9d19151d17bc4b440600165faaad7926a33a32409ecdb44d05b3afce8ddcd3`.
- `t317-sdk-afid-control-01`: `6a8ed747253c77f884bfe4f6895ab061bab03c10819d2d181f582d97f028307f`.

The complete changed/untracked candidate inventory is captured before checks;
these result paragraphs follow it. No binaries are copied to evidence. t317
remains open for startup and role-client integration, remaining role joins,
in-place fragment controls, minimum Bubblewrap execution and final acceptance.

## Reusable C SDK conversation and admitted lock root

Signed C SDK `52d1e1897efc42e43d8dc5020349d4a1ed519258` owns the
bounded two-connection launch conversation. The caller supplies both clients,
storage and monotonic time; the helper relays only its own afid challenge,
sets CLOEXEC on the provisioned channel, enforces a five-second total deadline,
and exposes the admitted root only after temporary handles are clunked.
`sophia_lc_init_attached` resumes ordinary lock-file bootstrap on that root,
without resetting the transport or issuing an unauthenticated attach.

The strict C and ASan/UBSan suites pass, with controls for accepting a refused
status, dropping the deadline, omitting CLOEXEC, and repeating ordinary attach
in the lock constructor. The closed `t317-sdk-launch-03` package's SHA256SUMS
hash is `dfeff9a787310c59ae5faa8eff5c7112cd9eb8e19104a675b5c8b8b74ca6319d`.

The protected lock fixture now uses these public SDK entry points for both
authentication rounds and lock-file negotiation. It reaches the current lock
object, then observes loss of authority on Session revocation. The independent
raw-wire fixture retains its exact waiting-read check; the SDK case does not
claim that its final service call has already placed the same read on the wire.
Both use real Factotum and protected launch custody, without a physical device.

All 17 Session launch-authentication tests, legacy lock C SDK interoperability,
focused Clippy, formatting and immutable SDK verification pass. Dropping the
afid in the SDK makes the joined peer fail its conversation-service assertion;
the exact Rust test exits 101 with one failure. Sources are restored and the
snapshot verifier passes again. Complete modified/untracked inputs are captured
before testing; these documentation additions follow the capture. Closed
package SHA256SUMS hashes are:

- `t317-sdk-launch-lock-01`: `672ec066f5f3f0b8cd816998c15e61b3f66f7a312e2e9592cc455458bc905262`.
- `t317-sdk-launch-afid-control-01`: `f6d3b0dc1bff0da49818ea57fd30f01287bb718ec3a5dbdcdfe0280d33ca7f22`.

This is SDK/fixture qualification, not normal desktop startup integration or a
new full-repository gate. The remaining t317 exit is unchanged.

## Shared Session coordinator and Factotum recovery

Session startup now retains the shared launch-service publication and creates
one coordinator. Its nonblocking poll runs on ordinary owner turns, including
unlocking, independently of PAM requests. The coordinator owns one registration
book per Factotum generation and one Session-wide transport budget. A missing
service admits nothing; an initialization timeout invalidates the shared
ingress without retrying that same generation. Publication-lock contention
does not revoke an existing live book.

Source review caught a recovery mismatch before enabling provider launches:
`ProcessSupervisor` retains its last launch generation across Factotum
replacement, while a fresh book previously began again at one. The coordinator
now preserves the next counter, including numbers burned by failed setup. The
recovery test uses two real Factotum exports and the same protected process
supervisor. It refuses the old registration, reaps the old process, and starts
the replacement with a higher generation. Resetting that counter to one fails
`Factotum replacement must not reuse a supervisor launch generation` before
the new spawn. Sources are restored and compared under the worktree lease.

`t317-coordinator-02` passes all three coordinator tests, all 17 protected
authentication tests, all ten unlock-supervision tests, focused Clippy and
formatting. The tests supply trusted Factotum channel admission/publication;
the recovery case executes actual protected spawns, with no devices. The
initial `-01` development package predates the monotonic-counter correction
and accidentally ran zero supervision tests without `native-session`; it is
not the final qualification. The corrected package requires all ten tests.

Closed package SHA256SUMS hashes:

- `t317-coordinator-02`: `133ded9bdcec6a078e11dc10b21b6eb6ef04fdef68b08f175a6da6b7f37dbfef`.
- `t317-coordinator-generation-control-02`: `43c1a60d81fe42e41d5af63e23d5af64f2b7970c9a58d1e5b59fff079b417fa1`.

The dynamic candidate inventory covers all seven changed or untracked source
files. These documentation updates follow the runs. No full-repository gate,
live session or merge is claimed. The normal lock provider still uses legacy
launch/admission: connecting its reservation, spawn, activation and retirement
to this shared coordinator is the next integration step, not a completed exit.

## Normal lock-provider owner and protected spawn

The next candidate connects normal Session lock-provider startup to the shared
coordinator. Session supplies a private namespace without portal capabilities,
principal and audience; X listener allocation and X admission are unchanged.
The owner reserves before spawn and publishes an authenticated assignee only
after the exact protected spawn and Factotum activation. Missing authentication
starts no provider and never falls back to ordinary attach. Provider failure
leaves Engine's existing lock cover in place.

One bounded spawn worker keeps Bubblewrap discovery and protected spawn off
the Engine owner. Cancellation retains an outstanding worker until its result
returns, then terminates a late child without authorizing it. Bubblewrap's
parent-death signal follows the spawning thread: the returned supervisor holds
a lifetime lease until the child is reaped or dropped. Exits are observed
before queued provider events are admitted, preventing old Connected events
from restoring revoked authority.

Five focused provider tests pass. The main join drives the actual provider
owner, C SDK child, protected supervisor, Factotum export and shared client.
Trusted Factotum channel admission and the scene are supplied; there is no
PAM process or physical display. The child refuses plain/forged attach,
authenticates twice across Tversion, negotiates lock files, then observes
Factotum-loss revocation without another Session-owner poll. Normal polling
reaps and replaces it with a fresh registration and connection epoch. Other
cases cover unavailable authentication, delayed/cancelled spawn, device
succession and terminal service failure. The shutdown case makes no claim
that Bubblewrap delivers the application's TERM handler or grace interval.

Three restored single-site controls pass their expected failures:

- Omitting the authentication factory fails `protected SDK peer must complete authenticated lock bootstrap`.
- Blocking on the spawn result fails `owner poll must not wait for protected spawn completion`.
- Dropping the spawning-thread lifetime lease fails the same SDK bootstrap assertion because the protected peer does not complete.

Four selected-Bubblewrap configuration tests, focused Clippy and formatting
also pass. These are liveness bounds, not desktop latency measurements.
The closed `t317-provider-owner-02` package retains those positives and controls
but is **STOP** overall: its full gate found an existing panel-probe fixture
whose permissions depended on umask. The exact test passes under 022 and fails
with `UnsafeMode` under 000. Signed `31e72a747` gives the probe mode 0600;
the exact test then passes under 000. Production permission checks are unchanged.

`t317-provider-owner-03` is also **STOP**, after all tests and Clippy passed:
the source-layout audit rejected a source-side `cfg(test)` test mount from the
earlier authenticated-lock checkpoint. Signed `b98bb59c8` moves that guard to
the already external test file. The unchanged cleanup test and layout audit
pass; this is a mounting repair, not relocated test logic or a relaxed audit.

`t317-provider-owner-04` is **PASS** for the complete device-hidden
`cargo xtask check`: **7,707 passed, zero failed, 105 ignored, 556 Rust test
summaries**. The gate also passes SDK checks, Clippy, layout, tooling and archive
checks. Hardware pixel proofs are explicitly unavailable with devices hidden.
The dynamic modified/untracked inventory covers all 12 provider source files
and equals the earlier positive/control inventory; the two separate repairs
are in its base `b98bb59c8701ed4a3451e2c848c6924fdc100009`. This documentation
update follows the source capture. Binaries are recorded by path/hash only.

Closed self-excluding SHA256SUMS identities:

- `t317-provider-owner-04`: `5a24b01bd06a997c4142b3507026a4051a202ecc63bf8d5031cffb6f39175cec`.
- `t317-provider-owner-02` (STOP): `4b880a5ac606a20fd2a193565d85922b1dd8b55dff461a01bf1e87017f76c277`.
- `t317-provider-owner-03` (STOP): `3fd1f44c77bc150517cbe89489087d65b51ca3caa528618654bd8c4288afd4fd`.
- `t317-provider-attach-control-02`: `cfc00560b85121d2cbdd7e225bac02cbc1bb6126263efa8a4f3d965f36dc9d99`.
- `t317-provider-spawn-control-02`: `054a4e2974270a46dc3a7fa7ddb0cfc5ab698d8a002c46a32d81bd3b3aeb5478`.
- `t317-provider-lifetime-control-02`: `6475fc605dece738ec1bdf7c3c8198b03625f3c0ec64eacae1ff9d644455913b`.

t317 remains open. The lock contract and pinned SDK copies need admission-text
reconciliation, and deployed providers must adopt the SDK authentication path
before installation. Other required roles, the real Rust SDK exchange,
`ClientAdmissionContext` derivation, in-place fragment revocation, execution at
minimum Bubblewrap 0.11.2, and the final bounds/threat-model audit remain. This
checkpoint was executed with 0.13.0; no master merge, live installation or task
completion is claimed.

## Lock admission contract and SDK alignment

Signed Sophia `368734300` specifies the authenticated lock-provider admission
path without changing record layouts. `1d35469ca` reconciles the launch
document's earlier foundation-only qualification text with the actual normal
provider join. The C SDK pin is now
`6d982e1a5f774690a9550721e1980fc60d7f613f`; the Rust SDK pin is
`f7803dc1e5adfff1b05991c0a7addcb17ef42d4e`. Both SDK deltas contain only
contract copies, digest manifests and provenance; implementation is unchanged.
The Rust copy does not claim an implemented launch helper.

Both immutable-snapshot verifiers now compare the launch-authentication
document as well as the lock-file contract. The ten snapshot tests pass,
including drift controls for every contract pair, followed by xtask Clippy
with warnings denied and formatting. The first pin-test run is retained as
STOP because the Rust test's expected pair count had not yet been raised from
nine to ten; the corrected test executes the new pair's drift check too.
The closed `t317-contract-pins-02` package's self-excluding SHA256SUMS hash is
`a40c66e5d9c8e2d303cb5f8412d99ade2c2fc289746dbc1857fb7092ba0a946a`.
No full-repository rerun is claimed for this documentation/pin delta; the
prior provider gate remains bound to its own candidate. Remaining t317 role,
Rust exchange, admission-context, fragment and minimum-version obligations
are unchanged.

## Minimum Bubblewrap execution

At signed Sophia `09d27c2dfd15b5f5e3e496c319b80255bce0f3bc`, the actual
Bubblewrap 0.11.2 binary passes all **26 tests**: four protected-channel tests,
17 real launch-authentication tests and five normal lock-provider owner tests.
This adds execution evidence to the earlier source review. The source is the
upstream `v0.11.2` tag, commit `1b80120ef26a28e065e67f89bfef873f13bdd317`.
The signed upstream tag is retained; local trust in its signing key is not
claimed. Meson builds with `support_setuid=false`, and the binary is mode 0755.

The host's 0.13.0 creates an outer device-hidden namespace; only inside that
namespace is `/usr/bin/bwrap` replaced by the built 0.11.2 file. The version is
checked there before testing. Runtime and Session then use the actual minimum
binary for protected spawn and discovery. Tests include unintended-fd exclusion,
channel generation/custody, C SDK afid exchange, Tversion, revocation, delayed
spawn, and normal replacement after Factotum loss. No installed package, host
binary, running desktop or physical device was changed.

Build output stays in the leased cargo-slot root, and evidence records paths
and hashes rather than copying binaries. The host has libcap 2.78 but lacks its
development files. The matching Void `libcap-devel-2.78_1.x86_64.xbps` was fetched
and checked against repository metadata SHA-256
`cb1c0363d9c998c4f990a951e036a590d0e171d48808b2ce6faca084bdb24c9b`.
Only its header/pkg-config input and linker symlink were extracted for this
build; execution uses the installed libcap shared library. SELinux integration
is disabled in this minimum-version build and is not qualified by this run.

The first two attempts remain closed STOP packages: `-01` lacks the development
dependency; `-02` built successfully but leaked its temporary pkg-config override
into Cargo, which failed before the selected test sequence completed. `-03`
unsets that build-only override and passes all three suites, zero failures or
ignored tests. Its self-excluding SHA256SUMS hash is
`75e2b2c291a2bb12096dbc5d245241b43a237b780428908819b278628fc304c2`.
No new production code or broader gate rerun is claimed. This closes the
minimum-version execution obligation for these Linux unprivileged launch
paths; other t317 role, Rust client, admission-context and fragment joins remain.

The qualification-text follow-up is copied from signed Sophia `49eacf646` into
signed C SDK `48a2d6083f03c67c0679b060fdf35f59d992ede5` and Rust SDK
`8c0140bbab85d66005469e405e214bbc4b674dd2`. The final immutable snapshots
contain no SDK implementation delta. All ten pin tests, every contract-pair
drift check and formatting pass again. The closed `t317-contract-pins-03`
SHA256SUMS hash is
`19110baec0946074f9fbe52874e9eb4e507f18207633979320a0b30f219c1fc0`.
The verifier implementation is unchanged from the Clippy-qualified `-02` run.

## Revocation between upload fragments

The next test-only candidate, based on `011eb7b0c`, joins the production 9P
`Connection`, `AuthenticatedFiles<LockFileExport>`, Session's
`RoleFileAuthentication` and `WorkerAuthority`, the launch book and real
Factotum. A protected independent C peer reads its own afid challenge, relays it
through its provisioned channel, authenticates again after `Tversion`, and
negotiates the lock export. It submits a real ResourceBegin and writes eight
pixel bytes to `upload/0`; no authentication verdict or upload binding is
supplied by the fixture.

The fixture drives the production connection directly and chooses a receive
boundary after four bytes. Those bytes enter the lock owner's private upload
destination, with no write reply yet. It then revokes the launch and asks for
the next destination. The core must return Discard before idle authentication
cleanup runs. The remaining four bytes are drained; the client checks the exact
write tag and `ESTALE` response. This proves fragment refusal independently of
the final write-commit and idle-disconnect defenses. It does not claim a
ResourceEnd, accepted image, rendering or physical display.

The one-site control in `connection/in_place.rs` changes
`export.check(&access).map(...)` to
`export.check(&access).or(Ok(())).map(...)`, ignoring the fragment authority
refusal. The first failure is the named assertion:
`revoked launch must refuse the next upload destination before idle cleanup`.
The control leaves the final commit check intact. Restoration is checked
against HEAD, all four dynamically captured modified/untracked fixture files
are unchanged, and the restored exact test passes. No production change is
retained. The shared C peer also reports unexpected reply kind/tag/errno, and
its test executable names are unique across concurrent test cases.

Device-hidden qualification runs all 18 launch-authentication tests (zero
failures or ignored), focused Clippy with warnings denied, and formatting.
The self-excluding SHA256SUMS identities are:

| Package | Manifest SHA-256 |
| --- | --- |
| `t317-upload-revocation-01` | `162d0ec44b75496fd0acb635276ecb48d45f88f810da5b1f5f56941ccd247a4d` |
| `t317-upload-fragment-control-01` | `d2887acaf960dc3ac23e0da11ee2c7b7552546448c535e8fb831aec559d07ae3` |
| `t317-upload-restored-01` | `e38493dfcd4f27760aeb26041b9aa304d373d75c1e0130841f7a973313d070cf` |

This is a supplied receive schedule around actual production owners, not the
normal lock service's scheduling loop. Session launch policy, the Factotum
Session-channel admission and the empty locked scene are supplied. The earlier
normal-provider join covers the shipped worker and idle retirement; this case
specifically prevents that stronger teardown path from masking a broken
fragment guard. Exploratory fixture failures corrected overlapping sandbox
mounts, message limits, reuse of a consumed transaction handle, an occupied
afid number and the upload directory spelling before qualification. No broader
repository gate or new minimum-Bubblewrap run is claimed for this test-only
delta. Other roles, the independent Rust exchange, immutable admission-context
derivation and final t317 reconciliation remain outstanding.

## Independent Rust launch client

Signed Rust SDK `c8e559106bc3eda1ff8abeadbd3c8d43adde2e64` adds
`sophia-launch-client`, a separate crate around the generic 9P pipeline. It
accepts exclusive role and provisioned-generation Unix streams, marks the
generation descriptor CLOEXEC, and relays only the challenge read from its own
afid. Both version handshakes and the subsequent exchange share one bounded
deadline. Only Tauth EAGAIN is retried, without renewal. It validates whole
challenge/status records, uses authenticated attach, clunks temporary fids,
and returns the admitted root plus both pipelines. Reauthentication resets
wire state and verifies the same claims through the retained generation
channel; it does not reuse a verdict. Failure/drop closes pending authority.
No unsafe Rust or Sophia-specific behavior was added to the generic pipeline.

Eight independent scripted-peer tests cover this helper. The SDK workspace
passes 351 tests, zero failed or ignored, all-target Clippy with warnings denied
and formatting. Three controls independently omit the afid at attach, corrupt
the relayed nonce, and omit CLOEXEC; each fails the corresponding named
wire/descriptor assertion. Restored source passes all eight tests again. The
SDK's provenance records the closed package identities; its positive
`t317-rust-launch-sdk-01` SHA256SUMS hash is
`cbee6ce3adab7d99257ea3b2ac72699ba76efac3ed4437cb85e5cd7bca418160`.

Sophia's candidate based on `e5a083be1` pins that exact signed SDK through the
maintained vendor command. A protected Rust child then uses the SDK against
the shipped `LockFileService`, Session authentication factory, live launch
book/worker custody and actual Factotum export over 9P. It authenticates,
reauthenticates after Tversion, checks temporary-fid cleanup, reads the lock
API, negotiates using the SDK's lock codec, acknowledges events, and retains
an events read. The parent observes Connected, revokes the exact launch, and
requires the child's ESTALE/closed-stream observation plus one Disconnected.

The descriptor bootstrap is deliberately separate: a small test-only C exec
shim moves the supervisor's inherited stream to stdin and execs the Rust test
child in the same process. It performs no wire exchange and knows no challenge,
verdict or role root. Rust marks stdin CLOEXEC and safely duplicates it into an
owned Unix stream. This avoids adding a production test API or unsafe raw-fd
adoption merely for the fixture. It does not qualify an application-specific
descriptor bootstrap. Session policy, its private Factotum-channel admission
and an empty locked scene are supplied; the role authentication, negotiation
and revocation are real owner operations. No PAM attempt or display is used.

The device-hidden join suite passes 19 tests; its one ignored entry is the
parent-invoked Rust child, whose exact filtered execution passes in the same
log. Both SDK pin suites (ten tests), the pinned Rust workspace's tests, focused
Session Clippy with warnings denied and formatting pass. The actual-owner
control replaces the Rust helper's authenticated attach with plain attach in
an isolated temporary mutation. After real successful verification, Session
refuses that attach with EACCES. The first child failure is
`Rust SDK must authenticate through the protected launch channel` with
`Remote(Errno(13))`; the parent then fails its child-success assertion. The
vendored bytes are restored exactly to the signed upstream blob, every
captured candidate file is unchanged, and the protected positive rerun passes.

| Package | SHA256SUMS SHA-256 |
| --- | --- |
| `t317-rust-lock-01` | `e216307d3ebc2a0e9271f0174449256404eee9bf429db47b188052781c732bf5` |
| `t317-rust-lock-afid-control-01` | `ba959f49b38a371508de8fb70cc2120492a49045623e9d2d5b9089f8606af114` |
| `t317-rust-lock-restored-01` | `ec5591075237830e5fe4e7f91bddff35ee2ff6f2f1983839a55135f813df0548` |

The main Cargo.lock delta adds only the two SDK path packages and their Session
test dependencies; no registry dependency is updated. An exploratory broad
lockfile regeneration was reverted and its build stopped. Qualification uses
the final retained dependency versions. The Rust helper's first compile also
corrected rustix's descriptor-operation module, and the joined fixture was
aligned to the existing lock codec's names/signatures before qualification.
This slice makes no new whole-repository or minimum-version execution claim.
Other required roles, immutable admission-context derivation, deployed-client
rollout and final contract/threat-model reconciliation still prevent t317 closure.

## Immutable admission context and retained authority

The candidate based on `a8e795fd9` joins the authenticated lock transport to
Session's actual namespace registry. `TransportBudget` now requires that
registry; the owner loop passes the registry used for X namespaces and the
private lock namespace through `LaunchCoordinator`. There is no fallback
registry, peer-selected namespace, or automatic namespace creation at attach.

```text
Factotum verdict ----> exact live WorkerAuthority
                             |
                     verified Identity
                             |
              Session NamespaceRegistry (try_lock)
                 prepare immutable admission
                             |
             pin transport identity -- failure --> discard reservation
                             |
             publish admission + prepared role root
                             |
                  transport AdmissionLease
                    |                 |
            immutable context    live revocation flags
                    |                 |
              survives Tversion      +-- namespace / admission / registry
                    |                 |
          fresh afid, same identity   +-- retained operation checks
                                      |   plus live launch custody
                          close/drop -+-> revoke without registry lock
```

The passive context records the Session-selected namespace profile and portal
capabilities, Session generation, admission ID, and sanitized Factotum principal
and launch generation. It contains no challenge, secret, callback or custody
handle. Factotum's connection generation and Session's generation are distinct;
the joined fixture deliberately uses 3 and 31 respectively. A context is not
proof of current authority: retained operations check the transport's admission
lease and current launch custody independently.

`PreparedAdmission` holds an exclusive registry borrow through the final
identity commit. Dropping it publishes nothing and burns its reserved ID.
Publishing the context and the already-prepared role root is infallible. A busy
registry refuses the first attach with EAGAIN rather than blocking the worker;
this is not a new SDK retry guarantee. Reauthentication reuses the exact context
and does not acquire the registry mutex. X admission still uses its existing
registry-owned lifetime and authentication methods.

The transport lease observes namespace and admission revocation through local
atomic flags. Closing or dropping it invalidates its registry entry without
locking; registry lookup/count exclude the retired entry immediately, and the
next admission allocation reclaims tombstones. Namespace revocation invalidates
all of its leases before map cleanup. Registry destruction also invalidates
retained leases. These handles live in the runtime owner, not protocol records.

The existing independent C upload peer authenticates twice across Tversion,
including its failed plain/early attaches. The receiver requires one exact
context throughout and one published admission. New variants revoke either
the namespace or that admission after four of eight upload bytes. The next
fragment must be discarded with ESTALE before idle cleanup runs, while the
registry mutex is held. The existing launch-revocation variant remains. All
three require that transport close leaves no registry admission. The shipped
raw, C SDK and Rust SDK service joins also check one admission after
reauthentication and none after retirement.

Device-hidden qualification passes the runtime suite (502 passed, 18 ignored
across 61 summaries), protocol data-model tests (29), launch-authentication suite
(21 passed, one parent-invoked child marked ignored; that protected Rust child
executes successfully in the same log), five normal-provider tests and three
coordinator tests. All-target Clippy for protocol, runtime and Session with
warnings denied, formatting and diff checks pass. The initial exploratory run
found that clearing a retired admission changed a waiting read's refusal to
EACCES; the closed-owner check restores ESTALE before qualification.

Two isolated controls test the new consequences. Removing only the retained
admission check fails each namespace/admission variant at its named next-upload
refusal, before idle cleanup can mask the missing guard. Clearing the context
on wire reset fails `Tversion and reauthentication must preserve the exact
admission context`. Both restore the exact candidate bytes; the full protected
authentication suite passes again. Packages capture all 19 modified/untracked
source and test files, command lines, test-binary paths and hashes, with closed
self-excluding manifests. No binaries are copied into evidence.

| Package | SHA256SUMS SHA-256 |
| --- | --- |
| `t317-admission-context-01` | `842982d44d1a17fcce8293d9ab7d1f92d5db78ec6b5597b80cd828b38405caae` |
| `t317-retained-admission-check-control-01` | `8d786fe3f6516ac1913c35bac1fcdff752125564982c2b64256fd37d1b1e1539` |
| `t317-wire-reset-context-control-01` | `513478f7b0ce1415202d24ab1932f81985e978abee1abdf6b41c939e7ae2f447` |
| `t317-admission-context-restored-01` | `953e774df15a9b581c782297268d3eae4557b887e2eaed7d31148e68df5ec294` |

This qualifies the shared admission factory through the protected lock role.
The lower-level direct `RoleAuthentication` helpers remain conversation/pinning
tests, not namespace-registry joins. Other required roles have not been migrated
by this change. No new whole-repository gate, minimum-Bubblewrap execution,
deployed-client rollout, master merge or live installation is claimed. t317's
remaining integrations and final contract/threat-model audit remain required.

## Protected output transport authentication

The output file transport now accepts the shared authenticated-file owner from
its supervisor-checked assignee. Root preparation borrows the export exclusively
and publishes nothing until verified admission commits. Tversion resets the root
without resetting output negotiation, topology identity or logical qids. Both
the expected-peer filter and output's SO_PEERPIDFD check remain in force. The
worker polls authentication and retains bounded retirement owners until remote
cancellation drains, including while role delivery is paused.

A protected generic Rust SDK peer authenticates through the real Factotum,
launch relay and OutputFileService, refuses plain attach, reauthenticates across
Tversion, reads and negotiates output files, then retains an events read during
launch revocation. The transport publishes one namespace admission and removes
it on retirement. The fixture supplies Factotum's trusted Session connection
class and one output topology; it uses Sophia's output record codec. The launch
authentication exchange itself is the pinned Rust SDK, not a fixture exchange.

Device-hidden qualification passes 505 runtime tests (18 ignored, 61 summaries),
22 launch-authentication parent tests (two ignored child entry points, each
executed successfully by its parent), all-target runtime/Session Clippy with
warnings denied and formatting. `t317-output-authentication-01` remains STOP:
Clippy rejected an explicit drop of a non-Drop preparation in a test. Replacing
that drop with a lexical scope preserves the abandonment check; run 02 passes.

The afid control replaces the SDK's verified attach with plain attach and fails
at `Rust SDK must authenticate through the protected launch channel`. The root
control publishes during preparation and fails at `an abandoned output root
must remain inaccessible`. Both restore exact candidate bytes. The restored
authentication suite and all 15 output-export tests pass. Dynamic candidate
capture binds all 13 changed source/test files; manifests retain binary paths
and hashes, without copying binaries.

| Package | SHA256SUMS SHA-256 |
| --- | --- |
| `t317-output-authentication-02` | `ebe24325a260ec7bd39bc278da0605646fff5cc51e0b6dc87acde992564dd366` |
| `t317-output-afid-control-01` | `397bc34c48d626874ce58a1b7967b9d7d40d5a05fbe5f6a8147ad2fcb7e448e5` |
| `t317-output-root-publication-control-01` | `ec6c6a3ce38d762fb3c97e7f0fc98d3a6491b5ed540bec4bcb005522156aa3f6` |
| `t317-output-authentication-restored-01` | `02ebc1bd0c3dcca0fccf20fc8681fd5cfdf4573bbe079514fb174abaefc5a846` |

Normal output startup still selects the legacy transport admission. Enabling
authenticated normal output, WM and shell launches requires moving the one
shared Session coordinator before those launches and passing their registered
launch authority to each transport. This checkpoint does not qualify that
startup wiring, minimum-version Bubblewrap, a whole-repository gate, deployed
clients or a live installation. These remain t317 work.

## Shared authentication owner before role startup

Session now creates its generation, namespace registry, supervised authenticator
and launch coordinator immediately after its owner wake, before activating
public role processes. It moves those same owners into the running loop instead
of creating a second coordinator there. Namespace allocation remains in its
existing order; no X namespace or capability changes accompany the relocation.
Startup errors drop the owners through their existing cleanup paths.

`t317-early-coordinator-01` records a device-hidden native-session compile,
three shared-coordinator regressions, five normal-provider regressions and
formatting, all passing. Its self-excluding manifest has SHA-256
`f46b0bcbd611fdc939187fbb29839a48f03817982813ee4169915eb3496136e1`;
the dynamic candidate records both changed source files. These tests cover the
existing owners, not an authenticated normal WM/output/shell startup.

Those launches still need policy namespaces, reservations and transport
factories. In particular, WM activation's blocking negotiation wait must poll
the coordinator once it depends on launch authentication; otherwise it would
wait for an exchange whose relay cannot advance. Replacement paths must use
fresh registrations on this same owner. This is unfinished integration work,
not an accepted legacy fallback or a reduced t317 exit.

## Normal output owner admission

Normal output preparation now binds an unassigned service and retains an idle
supervisor. It refuses configuration without Factotum before creating the
endpoint. The running Session owner waits for its shared coordinator, reserves
the output principal/audience (2), and launches through `ProtectedSpawn` with
that reservation's inherited channel. Activation rechecks the actual protected
child before passing the shared afid factory to the output worker. Its private
namespace has no portal capabilities. No credential-only fallback remains in
this normal launch path; the expected-peer and SO_PEERPIDFD checks still apply.
The spawning thread's lifetime follows the retained lease.

The first assignment now arrives after service creation. Its correlated marker
names the existing epoch 1 and no abandoned proposals, so it must not cancel
Session's startup output transaction or advance its connection epoch. The first
exploratory join exposed the former replacement-only assumption as
`InvalidConnectionEpoch`; the explicit initial marker fixes it. Later assignment
and disconnect handling retain the existing cancellation path. An outstanding
launch is not reported as an exited peer to the output proof observer.

The existing supplied-topology Session fixture now boots a protected Rust SDK
peer through this production owner. A C shim only moves its actual inherited
descriptor to stdin and execs the same process. The SDK authenticates; the
fixture then takes the quiescent root for its independent raw output-file
exchange. Both automatic and administrative WM restarts preserve the output
PID, role epoch and opened topology. Factotum's trusted Session-channel class
and topology remain supplied. Separate tests refuse missing Factotum and keep
the process unspawned while configured Factotum is unavailable.

`t317-output-owner-01` passes 17 output-file parent tests (three ignored entries:
two parent-invoked children and the opt-in native proof), the 22-test protected
authentication suite with both SDK children executed, all-target Session Clippy
with warnings denied and formatting. The output child executes successfully in
the parent log. An exploratory Clippy run found a fixture closure reduced to one
call; replacing it with the literal argument vector resolves that warning.

The control removes the factory from the supervisor-checked assignee. The real
SDK child fails at `normal output owner must authenticate its protected peer`,
and the exact parent fails. After restoring all ten captured candidate files,
the 17 output-file tests pass again. Binary paths and hashes are recorded;
no binaries are copied into evidence.

| Package | SHA256SUMS SHA-256 |
| --- | --- |
| `t317-output-owner-01` | `c6da0010d603ca22f6ee220a822e7cf66699df2218088c10d35d4bd87e62f30a` |
| `t317-output-owner-afid-control-01` | `6d28e77c4fdf8f7baeaccef92dc8e016dbdfcd0c0cd96f4bb911e8fef35fbde8` |
| `t317-output-owner-restored-01` | `35ca5a41d76f2364e85176e09adfaa539f0f1c58d44afe91c85dfc7eb2f1a684` |

Output remains a one-shot role: loss of its registration fails closed and
terminates that child; this does not introduce automatic output restart or
command replay. The existing opt-in native-output proof still assumes a
pre-spawned credential-only peer and needs migration together with its C client.
It was not executed or requalified here. Output's normative contract and its
SDK copy must be updated together with that client handoff; the old one-attach
wording does not describe this authenticated branch. WM/shell integration,
deployed clients, minimum-version and final repository gates remain open.
No master merge, installation or physical acceptance is claimed.

## Authenticated C output proof

The generic output proof now uses the public C launch client when Session
provisions `SOPHIA_LAUNCH_AUTH_FD`, then hands its exact quiescent role wire
and root to `sophia_os_open_attached`. Invalid descriptors or authentication
failure terminate the client; they never select raw attach. The unprovisioned
mode remains for the existing supplied-admission runtime fixtures. Both wire
buffers outlive the output session, and the inherited relay is marked CLOEXEC
by the launch client. Output records and topology semantics are unchanged.

The opt-in Session fixture now drives the actual asynchronous output launch
owner and shared authentication coordinator rather than expecting a child at
bind time. It checks the actual protected launch identity. Its real Factotum
export has a supplied trusted Session-channel class; topology preparation,
apply, first presentation and rollback remain supplied. The four stages prove
validation, unknown-mode refusal, commit-and-restore and supervisor termination
with rollback through the authenticated C SDK connection.

The first joined run authenticated but stalled until its bootstrap deadline:
the C SDK's new admitted-root constructor had not scheduled its first request.
Signed SDK `b8fc3a466d301e5672d78bff04866e82fe6359d6` fixes this by calling the
same bounded bootstrap driver as the fresh-fd constructor. Both constructors
now have a test requiring writable work or an immediate timeout without peer
input. Omitting that call fails the named scheduling assertion; the full C
suite passes before and after restoration. SDK provenance records those
packages and the earlier reset and reattach controls. The updated contract
copy comes from signed Sophia `d5c9b63a3`.

`t317-output-native-auth-01` passes all 13 runtime proof tests, the one Session
test exercising all four authenticated stages, the vendored SDK identity check,
all-target Session Clippy with warnings denied, formatting and diff checks.
Its `SHA256SUMS` has SHA-256
`4110f9f790f9e0e9deff8ed1d0769f95c50dfee33ea9b1cb4104f99bf4b166a9`.
The package captures the complete modified/untracked candidate file set and
records executable paths/hashes. It contains no binaries. Earlier exploratory
commands omitted `native-session` and selected zero Session tests; those are
not counted. The failing joined run is retained in the local development logs.

The legacy native-proof xtask still copies its prepared binaries into evidence;
it was not used. This run builds directly in the leased cargo slot inside a
device-hidden sandbox. There is no new physical acceptance, live install or
whole-repository gate claim. Normal WM and shell authentication, deployed-client
rollout and the remaining t317 closure gates remain required.

## WM root preparation

The WM file owner now implements the shared `ProtectedFiles` preparation
contract. Its exclusive prepared root publishes the attached bit only after
the authentication owner can commit; dropping preparation changes no authority.
Auth qids use the same logical allocator as WM files and snapshots. The reset
entry point clears only the matching connection's attached bit, preserving
the role epoch, selected capabilities and submission/domain replay watermarks.
It cannot revive a revoked owner. Existing raw attach delegates to the same
preparation, without gaining authenticated-reset privileges.

`t317-wm-root-01` passes 56 tests in the WM 9P subtree (three remain ignored),
including the three new preparation/reset/qid checks, plus all-target Session
Clippy with warnings denied. Formatting and diff checks pass. The package's
`SHA256SUMS` has SHA-256
`9a528ac45402639c297366d13d1e16e121af2c149d9d24d395d16b6b36cd7521`.
An initial compile exposed insufficient visibility on the prepared-root
associated type; matching its owner's crate-internal visibility fixed it.
The complete changed-file hash capture was made after the tests and Clippy,
with no intervening edits. These are root mechanics, not a real afid exchange:
the WM reactor, normal launch/replacement and deployed-client integration
still need connection to the authentication owner.

## Authenticated WM reactor

The WM reactor now wraps its existing file owner in `AuthenticatedFiles`.
An owner-supplied factory travels through worker creation and endpoint startup;
authenticated construction installs it before any reactor turn. Factory refusal
closes the socket, and pre-adoption cancellation reserves no authentication.
Every reactor turn polls the authentication owner and bounds idle waits by its
deadline cadence. WM semantics still belong to the existing file owner.

Dropping a worker cannot wait for Session, which may be joining that thread.
The factory can hand closed authentication custody to the Session transport
budget's bounded retirement channel. The coordinator polls it even without a
current Factotum generation. Pending owners retain endpoint/global permits
until exact remote cancellation completes; a failed handoff retains fail-closed
disposal. The receiver is owned separately from the permits, so its final drop
drains queued owners rather than retaining an Arc cycle. The real-Factotum
capacity test proves that handoff returns no early credit, eventual cancellation
returns one slot, and cleanup preserves other launches.

The protected WM test reserves and activates actual SpatialPolicy launch
custody, then starts the production pending-endpoint adapter with the resulting
factory. A C descriptor shim execs the generic Rust SDK child unchanged. The
SDK authenticates through Factotum; its quiescent root then passes to the raw
WM fixture for Negotiate/Submitted/ACK traffic without Tversion or reattach.
Factotum's trusted Session-channel class is supplied. This is a real protected
WM transport join, not yet the normal desktop startup owner or a deployed WM.

`t317-wm-reactor-auth-01` passes 59 WM-file parent tests (four ignored entries,
including the parent-invoked authenticated child), the 23-test authentication
suite with both SDK children executed, all-target Session Clippy with warnings
denied and formatting. Discarding the factory at reactor adoption fails the
protected child at `protected WM must authenticate before file negotiation`
and then its exact parent. Restoring all candidate bytes restores the 59-test
pass. Packages record dynamic source hashes and executable identities, without
binary copies. The reused driver calls its WM subtree log `output.log`; the
recorded command and test paths identify the actual WM scope.

| Package | SHA256SUMS SHA-256 |
| --- | --- |
| `t317-wm-reactor-auth-01` | `c670dfc6845ad8a9c43f8590d7e2aeb44587788d46b23871e45de377e51c8e62` |
| `t317-wm-reactor-afid-control-01` | `4283fa83c704f5b9418d5e9a54ddded358c40d4d18bf5e9ea1901caf999ba9bd` |
| `t317-wm-reactor-restored-01` | `a129eef377a511a7a497121458133dc400978a140ebcad3b8d8aa9042eee5fbe` |

Normal WM startup, automatic/control restart and profile replacement still
supply no factory. They must register each launch and poll the coordinator
during startup admission before that fallback is removed. The C/Nim WM SDK
handoff and deployed-client migration remain open, as do shell integration and
the final t317 closure gates. No live admission rollout is claimed here.
