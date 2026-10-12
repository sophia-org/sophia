# Local launch authentication

This implements the Factotum owner portion of the approved
[t133 handoff](notes/plans/esnqxpqw-pidfd-and-namespace-admission-optimizations.md#local-authentication-design-and-t317-handoff-2026-10-10).
The current t317 candidate has a generic afid seam, a private Factotum
control exchange, a shared PAM/launch client, bounded verification ingress,
a conversation-only child relay, protected spawn registration/activation and
a Session afid owner. A generic role-view test joins the Rust SDK, real core,
Factotum and a protected independent C relay peer. A separate owner-level
join covers atomic admission into the actual lock-file export. The lock worker
now uses authenticated dispatch in normal Session startup, exercised with a
protected independent C wire peer and the reusable C SDK. Normal WM, output,
descriptor-shell, bar, launcher and dock startup also use the shared admission
owner. Protected C and Rust SDK joins and separately supplied client joins
are recorded in the [qualification investigation](notes/investigations/etyvymxi-authenticated-attach-transport-state-and-factotum-handoff.md).
Final candidate qualification and integration remain separate from these
checkpoints; this document does not claim a live installation.

### Transport boundaries

| Interface | Admission in this candidate |
| --- | --- |
| Supervised WM, lock, output, descriptor shell, bar, launcher and dock | Factotum-correlated afid plus current Session launch custody and namespace admission. Expected-peer acceptance remains an availability filter. |
| Private Session–Factotum control and provisioned child relay | Session-owned channels with their own fixed views; neither is an anonymously accessible role endpoint. |
| Live portal requester | The approved protocol-authority socketpair custody model, bound to an adopted connection and exact namespace pair. No protected-process or afid claim. |
| Metadata broker and administrative control | Existing protected-broker and HostDomain admission on legacy transports. Their file migrations remain t314 and t254 respectively. |
| X11 applications | Existing X authority admission; t317 does not replace it with afids. |

The [portal contract](sophia-portal-files.md#admission-and-functionality)
does not yet integrate an out-of-process protected requester. Its generic
protected-peer export tests do not establish such a live owner. The
[delivery plan](notes/plans/jsschoen-converge-public-roles-on-one-9p-core.md)
retains these separate migrations and namespace composition. Authentication
of the supervised roles does not imply that every Sophia transport uses afids.

```text
                 Session (trusted admission owner)
                    |                     |
          register delegation      reserve role transport
          confirm own spawn        start exact afid attempt
                    |                     |
                    +---------+-----------+
                              | private 9P control connection
                              v
                 Factotum launch controller
                 +-------------------------------+
                 | <=64 reserved/active launches |
                 | <=32 attempts/results         |
                 | kernel-generated 256-bit nonce|
                 | exact attempt + audience      |
                 +-------------------------------+
                              ^                 |
             channel-attested nonce             | correlated verdict
                              |                 v
   child -- P --> Session relay          Session final custody check
      ^                                  + immutable identity pin
      |                                       |
      +-- nonce read on own afid -------------+--> admitted role view
```

Session selects the principal, namespace, launch generation and role audience.
The child cannot supply these registration facts. Factotum trusts Session's
attestation of which provisioned channel relayed a nonce; it neither inspects
that channel nor proves the holder's PID, executable or human identity. A
cooperating holder can delegate its existing capability. Session must still
check live launch custody and current policy before installing a verdict or
admitting an attach.

## Role afid conversation (t317 candidate)

The Session `RoleAuthentication` owner lives outside the core's fid table.
An export checks its fixed, policy-selected uname/aname and connection before
calling `begin`; numeric uname must be `NOUID`. Claims never choose identity.
Ordinary attach without an afid and writes asserting verification are refused.

| Afid read | Whole record | Meaning |
| --- | --- | --- |
| offset 0, count at least 40 | version u32=1, kind u32=1, nonce [32] | The fixed Factotum challenge; rereads return the same bytes and charge the budget. |
| offset 40, count at least 8 | version u32=1, errno u32=0 | Correlated verification plus a current Session custody check. Until ready, the core retains the read. Failures use the ordinary 9P error. |

Other offsets or short counts receive `EINVAL`. The child relays the challenge
through `P` before reading status. The initial charge includes the relay's
40-byte request and eight-byte response; afid replies charge their actual record
length. Oversize cancels the exact attempt. This status is not a grant to cache:
attach rechecks the launch and pins identity only after the export has reserved
its root/reply and completed scope-policy checks. Retained role fids recheck
current launch custody independently of the old attempt's lifetime.

Clunk/reset drops the local conversation and queues exact cancellation. The
owner retains cleanup until acknowledgement; a failed cleanup deadline
invalidates the shared ingress so its connection owner closes the actual
Factotum transport. A transport owner must poll even without peer traffic,
close an expired transport, and retain its bounded slot until cleanup drains.
Tversion preserves the identity pin and original first-attach deadline.
The lock, output, WM and shell transports implement these hooks through
`AuthenticatedFiles`, selected by their normal authenticated startup paths.
Legacy fixture entry points remain explicit; an authentication failure on a
required path must never select them.

### Atomic lock-root publication

The lock-file owner offers a checked, exclusively borrowed root preparation.
It cannot be copied, and dropping it publishes nothing. Session's `attach_lock`
checks claims, prepares the lock view, then verifies current custody and commits
the transport identity. Publishing the prepared root is infallible:

```text
core reserves root fid and reply space
                |
                v
lock.prepare_attach -- refusal --> identity unchanged
                |
                v
auth.commit_attach  -- refusal --> drop preparation; no root
                |
                v
prepared.publish    ------------> identity and lock root admitted

retained operation --> auth.check_admitted --> lock.check --> file operation
```

The preparation must not cross a suspension or worker handoff. Its exclusive
borrow prevents another operation from revoking the view between preparation
and publication. `check_lock` checks both current launch custody and lock-file
epoch/custody; the transport must call it for ordinary operations, retried
reads and each in-place upload fragment.

This sequence is joined both by direct owner calls and through the real lock
worker with a protected independent C wire peer. The ordinary legacy attach
path uses the same checked preparation and retains its existing policy.

### Authenticated lock transport

Session gives the lock assignee an authentication factory backed by current
`WorkerAuthority`. A pending export refuses requests until construction installs
that owner; failure cannot select legacy attach. The runtime adapter preserves
the role's file semantics and in-place writes, delegating authority checks to
Session for ordinary operations, retried reads and upload fragments.

```text
expected-peer availability filter
             |
             v
Session factory -- reserve endpoint + global slot --> afid owner
             |                                         |
             v                                         v
AuthenticatedFiles<LockFileExport> <-------- current launch custody
             |
             +-- prepare root --> commit identity --> publish root
             +-- retained operation --> custody check --> lock file owner
             +-- disconnect --> pending cancellation --> acknowledgement
                                                           |
                                                           v
                                                    return quota slot
```

One `TransportBudget` belongs to Session; each persistent endpoint holds its
own budget across launch replacements. Limits are 32 globally and eight per
endpoint. This implementation conservatively retains a slot for the whole
admitted transport as well as disconnected cleanup. Slots return after cleanup
is acknowledged, or after failing the shared Factotum connection closed.
An abandoned undrained authentication owner invalidates that shared ingress.

Before authenticated attach, the core allows at most four fids and four
outstanding requests, including undrained replies. A fifth request closes the
transport before dispatch; a fifth fid receives `EMFILE`. Smaller configured
limits still apply. Attach restores normal role limits, and Tversion restores
pre-attach limits while preserving the pinned identity. The lock uses a 64 KiB
msize and 128 KiB output bound. Its role-negotiation timer begins only after
authentication; the absolute first-attach deadline is not renewed.

The worker polls idle custody and authentication deadlines at a bounded 20 ms
cadence, with Factotum answers also waking it. Disconnected cleanup remains
bounded; when all eight retirement slots are full, listener readiness is
excluded from the wait so a queued connector cannot cause a polling loop.
These are progress bounds, not input latency claims.

Only the explicitly authenticated path omits `SO_PEERPIDFD`. It retains the
expected-peer availability filter, supervisor-owned pidfds and every afid/
current-custody check. The legacy path still requires `SO_PEERPIDFD`. The C
wire test proves plain/forged/unverified refusal, successful authentication,
fresh authentication after Tversion using the same launch channel, negotiation,
and revocation of a retained waiting read. A second build uses the pinned C
SDK's reusable launch conversation against the same real owners. The helper
owns both wire exchanges, enforces its total deadline and sets CLOEXEC on the
provisioned channel. The lock SDK accepts its admitted root without another
Tversion or ordinary attach, then performs its normal file bootstrap. The SDK
case reaches the lock object and observes revocation; the raw-wire case retains
the exact waiting-read check.
The investigation separately records revocation between upload fragments,
the other role owners and independent client joins. Final acceptance and SDK
snapshot alignment must be checked against the final candidate. The lock-file
contract specifies the same admission path.

### Session coordinator

Normal Session startup retains the launch-service handle alongside its unlock
authenticator. One `LaunchCoordinator` polls that publication every owner turn,
including unlocked and unlocking turns. It owns one `LaunchBook` for the current
Factotum connection and one transport budget shared across role endpoints.
Each role registration includes the Factotum generation; an old registration
cannot take a channel or publish custody through a replacement book.

```text
Factotum supervisor -- current generation + ingress --> Session coordinator
                              |                          |          |
                     PAM on same owner                   |    shared 32-slot
                                                         |    transport budget
                                                   one LaunchBook
                                                         |
                                               <=64 role registrations
                                                         |
                                          reserve -> spawn -> activate
```

The launch counter survives Factotum replacement because role supervisors also
survive and reject reused launch generations. Failed setup burns its number.
Initialization has a five-second deadline; failure closes the shared ingress,
and only a new supervisor publication can recover. A contended publication
mutex does not revoke a still-live connection. Role endpoints retain their
budget through replacements and pending cleanup.

The normal lock-provider owner now reserves and activates through this
coordinator. Session allocates a private namespace with no portal capabilities
after allocating the X listener namespaces; X admission is unchanged. Session
selects the lock principal and audience, and the client cannot choose either.

```text
Session owner                       bounded protected-spawn worker
    | reserve delegation                         |
    | wait for Factotum registration              |
    | hand off supervisor + P ------------------>|
    | continue Engine turns                      | spawn + protection check
    |<------------------------- result + lifetime lease
    | activate exact launch                      | retain spawning thread
    | publish authenticated assignee              | until child reaped/dropped
    v                                            |
lock transport --> afid + current custody --> admitted lock files
    |
    +-- exit / Factotum loss --> revoke locally --> retire / reap / replace
```

Missing authentication admits no provider and never selects ordinary attach.
Engine's locked cover is independent of provider availability. One worker per
provider keeps spawn and protection discovery off the owner loop. Cancelling a
pending spawn retains the worker until its result returns; a late success is
terminated without authorization. Bubblewrap's parent-death signal follows the
spawning thread, so a lifetime lease retains that thread until the supervisor
reaps or drops its child. The owner observes exit before admitting queued
provider events, so an old Connected event cannot restore revoked authority.

The joined test uses the actual provider owner, protected C SDK child, Factotum
export and control client, with supplied trusted Factotum channel admission.
It authenticates, reauthenticates after Tversion and negotiates lock files.
Factotum loss revokes the worker independently of another owner poll; normal
owner polling then reaps and replaces the provider under a fresh registration.
No PAM process, physical display or installed desktop is claimed by this test.

### Current launch authority across workers

Session can publish `WorkerAuthority` only for an Active registration and the
exact protected spawn that consumed its reservation. The role admission owner
accepts either direct Session authority or this worker handoff; it uses the
same afid and atomic root-publication sequence in both cases.

```text
Session LaunchBook                      ProcessSupervisor
 Active registration                     exact reservation + role
       |                                        |
       +------ publish WorkerAuthority ---------+
                          |
role worker: verdict -----+--> check session / identity / audience
                          +--> registration still active
                          +--> Factotum ingress still open
                          +--> wrapper AND peer pidfds still alive
                          +--> supervisor has not withdrawn custody
                          |
                          v
                  commit identity / retained operation

Session revoke ------> shared registration flag false (before remote Retire)
termination ---------> shared custody flag false (before signaling the child)
kernel exit ---------> pidfd readable (without a Session poll or reap)
```

These checks use atomics and zero-time kernel polls, with no worker request to
Session and no wait on authentication I/O. A clone cannot reactivate a launch.
The wrapper handle is captured while it is the supervisor's unreaped child;
the peer handle uses the existing checked wrapper-child relationship. This
retains the existing `pidfd_open` platform requirement. The authenticated lock
path uses this custody instead of the socket's `SO_PEERPIDFD`; the legacy path
and current desktop startup retain that requirement. The expected
peer availability filter is unchanged. This is a handoff of launch custody,
not a substitute for Factotum verification or role-view policy.

## Private owner view

An already-admitted `ChannelClass::Session` connection can attach with aname
`sophia-launch-v1`. Other channel classes receive `EACCES`. The view contains
only `rpc`: no parent walk escapes to the ordinary factotum tree, and it has no
ctl, keys, log, PAM or password-retrieval path. This is an owner interface, not
the future child relay. The existing Session connection can separately access
ordinary factotum files under its existing authority.

Only one launch-control handle may be open in the agent. A second open receives
`EBUSY`. Clunk, wire reset or disconnect drops all registrations, pending
nonces and undelivered results owned by that handle. Reopening requires fresh
initialization and registration; Session must invalidate its corresponding
outstanding work, rather than carry verdicts across this boundary.

Each write is one complete command at offset zero; each read returns its whole
response. Offsets do not select fragments. A short read receives `EINVAL` and
keeps the response; a read with no response receives `ENOMSG`. Until the response
is consumed, another write receives `EAGAIN` before applying a command. Unknown
versions/kinds, truncation and trailing bytes are `EINVAL`. No record exceeds
556 bytes. The original commands and responses retain their encodings and
84-byte maximum; the recipe registration command below extends only the private
owner interface. No nonce or response body is logged.

All numbers are little-endian. Every record starts with version `u32 = 1` and
kind `u32`. An **identity** is `(principal u64, namespace u64, launch u64)`.
An **attempt** is `(server u64, connection u64, session u64, audience u64,
wire_generation u64, attempt_generation u64, afid u32)`.

| Command kind | Body | Effect / response |
| --- | --- | --- |
| 1 Initialize | session u64 | Set the control owner's nonzero session once; Done. |
| 2 Reserve | identity, audience u64 | Reserve an increasing, never-reused launch generation; Done. |
| 3 Activate | launch u64 | Mark that exact reservation active after Session's spawn confirmation; Done. |
| 4 Retire | launch u64 | Remove registration and its queued verified results; Done. |
| 5 Begin | attempt, remaining_msec u32 | Reserve an attempt and generate a nonce; Challenge. |
| 6 Relay | launch u64, nonce [32] | Check Session-attested registration, active state and audience; consume nonce and queue verdict; Done. |
| 7 Take | attempt | Return Pending or consume its exact Verified result. |
| 8 Cancel | attempt | Cancel only the exact attempt/result; Done. |
| 9 ReserveAudiences | identity, primary_audience u64, count u32, audiences[count] u64 | Reserve one launch with an immutable approved audience set; Done. |

Kind 9 has 1–64 strictly increasing, nonzero audiences, including the primary
audience. Duplicate, unordered, empty or oversized sets refuse. Only trusted
Session recipe policy selects the set. Kind 2 still registers exactly one
audience; it never means all roles. Re-registering the same launch cannot extend
its set. An older Factotum refuses the new command rather than silently reducing
or widening policy.

The same launch capability can authenticate separate connections to approved
endpoints. Each attempt still names one exact audience, server, connection and
afid; all successful endpoints pin the same launch identity. Session checks
membership independently and narrows each worker to a single audience. Role
permissions remain separate export policy: an audience set does not grant
operations merely by authenticating. Existing launch, principal, attempt and
transport quotas are unchanged. Abandoned kind-9 registrations use the same
Retire cleanup as kind 2; closure or retirement invalidates the whole set.

| Response kind | Body |
| --- | --- |
| 0 Done | empty |
| 1 Challenge | nonce [32] |
| 2 Pending | empty |
| 3 Verified | attempt, identity |

The remaining duration must be positive and at most 5000 ms. Factotum starts
its own bounded deadline on receipt. **Session retains the original absolute
attempt and first-attach deadlines**, rejects late results and does not renew
them for queueing, retries or Reserved state. A Factotum deadline alone cannot
prove the end-to-end deadline. There is no blocking entropy wait: nonblocking
kernel randomness, a short random read or unavailable entropy refuses the
attempt. Random-source failure is not yet fault-injected in the tests.

Reserved launches cannot verify. A wrong audience, stale registration or quota
refusal does not consume another valid nonce. Successful relay consumes its
nonce exactly once. Results remain inside the bounded attempt table until
consumed, cancelled, expired or retired. Full correlation is checked for Take
and Cancel, including wire generation and afid; knowing a numeric afid does
not identify an attempt.

Before relay, Factotum charges only its global/transport quotas; no unverified
claim charges a principal. After relay it permits at most two undelivered
results per launch and eight per registered principal. Session must retain its
own launch/principal and owner-work reservations through delivery or cancellation
acknowledgement. The Factotum table is not a replacement for Session's endpoint,
first-attach, lifetime-attempt and 4 KiB transcript budgets.

Expiration removes pending and verified-but-undelivered entries without renewing
their deadlines. Backwards owner time permanently closes the authority. Reserved
registrations are released by explicit retirement or control closure. Unknown
or spent work is `ESTALE`; invalid facts are `EINVAL`; quotas, Reserved state and
unavailable randomness are `EAGAIN`; a wrong audience is `EACCES`. A clock fault
or closed owner is `EIO`.

## Qualification boundary

The foundational private-owner tests use real kernel randomness, the production launch owner,
Factotum export, 9P server and Rust pipeline. Channel class, registration and
activation are supplied trusted facts. They do not prove protected spawn,
Session's final custody check, child isolation, role migration or a remote
authentication mechanism on their own. The protected joins described above
add those local custody and lock-role observations. The published child
afid/relay transcript is separate: these private owner records must never be
handed to the child as its launch channel.

## Protected channel handoff

`LaunchChannel::reserve` creates a fresh connected Unix socketpair for a
nonzero launch generation. Session retains the owner endpoint; a non-cloneable
handoff owns the child endpoint. Both endpoints are moved above stdio if
necessary. Neither belongs in the reusable `ProcessLaunchSpec`.

```text
 Session reservation L                  ProcessSupervisor
 +---------------------+                +-------------------------+
 | relay endpoint      |                | reusable spec: no fd    |
 | fresh child endpoint| -- consume --->| burn generation L       |
 +---------------------+                | require protected domain|
                                        +------------+------------+
                                                     |
                                       child-only pre-exec hook
                                       mark fds >2 CLOEXEC
                                       clear CLOEXEC on P only
                                                     |
                                                     v
                                           Bubblewrap -> child P
                                                     |
                            own protection result + generation L
                                                     v
                              Session may activate reservation L
                              (exact protected spawn required)
```

The new `start_with_launch_channel` path rejects an unprotected domain and a
configured `SOPHIA_LAUNCH_AUTH_FD` claim. The launcher supplies that variable's
descriptor number. It clears the host environment and excludes unrelated
descriptors for both the Bubblewrap version probe and the final launch. The
version probe never receives the designated channel. On the final command a
narrow Linux pre-exec hook retains only the consumed descriptor above stdio.
Parent flags are unchanged, and the parent's child-end copy is dropped as soon
as spawn returns. An error drops the handoff too.

Generation numbers are spent on attempts, including failures. The supervisor
records the successful child's own generation beside its protection evidence;
a competing reservation cannot relabel that child. Once the supervisor has
used this path, its ordinary start command refuses without a fresh channel.
Spec replacement does not reset that requirement or its generation watermark.
Session still owns the global reservation allocator and the final Active check.

Existing non-authenticated starts retain their previous behavior. This primitive
uses the current supervisor's synchronous spawn/protection confirmation; it does
not prove nonblocking scheduling on its own. The normal lock-provider join
above runs it on a bounded worker before activation.

The independent C descriptor probe runs through the production supervisor and
the installed Bubblewrap. It checks the designated connected socket, the absence
of all other non-stdio descriptors (including a deliberately inheritable
sentinel), unchanged parent flags and channel closure after exit. It sets
CLOEXEC on adoption and checks that an ordinary exec excludes the channel.
That is a C descriptor observation, not SDK adoption or a C authentication
exchange. Fork and intentional capability transfer are not prevented.

The supported minimum's [Bubblewrap 0.11.2 source](https://github.com/containers/bubblewrap/blob/v0.11.2/bubblewrap.c)
closes passed descriptors in its monitor while preserving them for the
application child. The supported path uses unprivileged user namespaces, not
setuid mode. The [minimum-version execution record](notes/investigations/etyvymxi-authenticated-attach-transport-state-and-factotum-handoff.md#minimum-bubblewrap-execution)
binds the upstream source and built binary to the protected-handoff and lock
authentication tests. Build inputs, versions and qualification limits belong
to that record. No live desktop or device is used by those tests.

## Nonblocking control client

`launch::client::LaunchClient` advances the private owner exchange on a
caller-owned, already negotiated SDK pipeline. It never polls or waits on the
socket. The connection owner routes completed replies by tag and gives each
client a bounded turn:

```text
                     one private Session -> Factotum stream
                                      |
                            SDK Pipeline (one owner)
                                      |
                          poll, then dispatch by tag
                         /                         \
                PAM conversation              LaunchClient
                waiting for helper            attach / walk / open
                                              write -> read -> result
```

There is one launch command or unconsumed result at a time. Further commands
are refused without submission. Pipeline capacity refusal preserves an unsent
next step; recovery queues that step, not a repeat of an accepted mutation.
The connection owner retains the bounded request queue described below. The
low-level client itself admits only one exchange/result at a time.

Setup and each command use caller-supplied absolute deadlines. A Begin record's
remaining duration is clamped to that deadline before queueing; neither a write
answer nor a buffered result renews it. Session must still recheck the attempt's
original deadline when using a result. The owner must service the client to
observe timeout, then close the shared connection and retire its delegations
on fatal error. An accepted write may have executed even when its reply was
lost; this client does not retry uncertain mutations. A verified reply must
name the exact requested attempt. Its identity still needs Session's final
registration, custody and policy checks.

An actual core/Factotum test holds a PAM read pending while this client completes
the launch exchange on the same stream, then delivers a supplied PAM rejection
to the original tag. It also checks result retention, deadlines and capacity
recovery. Admission facts and PAM completion are supplied. The shared owner
below integrates this client with PAM. The protected activation and final-attach
joins described above add the launch-custody evidence; this control-client test
alone does not prove it. No second privileged Factotum process or connection
was added.

## Shared Session connection owner

`client::SessionClient` owns the pipeline, one PAM conversation/result and one
launch exchange/result. It opens the ordinary Factotum view and private launch
view on the same Session stream. Its nonblocking poll advances each conversation
and dispatches replies only to the owner of that tag. PAM's password write uses
the SDK's zeroizing output path; retained and temporary password copies are
zeroed on drop. An accepted write is never repeated to recover capacity.

```text
 Session owner loop                    unlock supervision thread
 +--------------------+                +----------------------------+
 | begin unlock       |--bounded------>| begin_login                |
 | poll verdict       |<--result-------| poll_login                  |
 | <=2 reserved slots |                |   SessionClient            |
 +--------------------+                |   + PAM conversation       |
                                       |   + launch control client  |
                                       |   + one 9P pipeline        |
                                       +-------------+--------------+
                                                     |
                                             one private stream
                                                     |
                                             Factotum 9P server
                                           /                    \
                                    PAM job pool          launch controller

 Child relay / verification queue ---- shared SessionClient launch ingress
                                      normal lock-provider lifecycle
```

The unlock supervisor uses begin/poll/take rather than waiting for a whole PAM
conversation. It keeps its existing process restart and backoff policy. Its
worker readiness cadence is clamped to 1–20 ms; the configured process-liveness
cadence is separate. These are worker scheduling bounds, not latency claims or
Engine timers. Startup/version negotiation still blocks on the supervision
thread within the handshake deadline. Dropping Session after startup ends a
pending login without waiting for PAM's verdict. A concurrent extra login is
answered Unavailable; it never starts another PAM conversation.

Two unlock reservations cover queued, active and undelivered attempts, and are
released only when Session consumes the corresponding verdict. The result
channel has two slots and uses nonblocking sends. Input is bounded before
copying into the worker queue. The PAM client enforces its smaller protocol
bound; an oversized password is a local rejection, not agent unavailability.
This unlock accounting is distinct from the global 32-slot launch verification
queue below, now used by the lock-provider launch lifecycle.

PAM success requires the password exchange, AuthInfo naming the requested user,
and the conversation's completed clunk. Client results retain their original
deadline until consumption through the client API. A fatal error or deadline closes the actual shared socket,
drops buffered verdicts and secrets, and lets Factotum reclaim the control
handle and cancel outstanding PAM work. Session's supervisor replaces the
failed agent. A normal login refusal or launch-command refusal does not close
the connection. The `UnlockClient` compatibility name and synchronous `login`
method use this same implementation; Session itself uses the nonblocking API.

Tests join the shared client to the actual core and Factotum export with supplied
admission and PAM outcomes. They cover simultaneous progress, both timeout
directions, peer loss, refusal, result custody and 130 repeated logins (past the
pipeline's fid bound). Separate supervisor tests exercise pending-login progress,
owner drop, bounded results and replacement with scripted agents. The concrete
Session adapter compiles with `native-session`; an actual PAM process/physical
lock run is not claimed. The protected lock join is separate evidence. Remaining
candidate-wide gates and contract reconciliation are required before t317 can
close; later role and independent Rust client joins are recorded separately
in the qualification investigation.

## Provisioned relay and bounded verification ingress

The child endpoint speaks 9P2000.L through the same core. Session adopts exactly
the endpoint it created for one launch generation; there is no listener path.
The attach name is `sophia-launch-relay-v1`, with an empty uname and no numeric
user claim. The only reachable child is `rpc`, with one exclusive read/write
handle. `ctl`, keys, logs, prompts and PAM are absent. A write cannot contain
a launch ID, principal or namespace. Session supplies the launch registration
when forwarding the challenge to Factotum.

```text
 protected child's P                  Session-owned relay
 +--------------------+    9P         +----------------------------+
 | attach relay view  |-------------->| adopted connection only    |
 | open rpc           |               | fixed launch generation L  |
 | write challenge N  |               | Reserved: bounded wait     |
 | read relay status  |<--------------| Active: enqueue Relay(L,N)  |
 +--------------------+               +-------------+--------------+
                                                    |
                                      <=32 reserved requests/results
                                                    |
                                      ConnectionOwner + SessionClient
                                          /                  \
                                    PAM tags            launch tags
                                          \                  /
                                           one private 9P stream
                                                    |
                                           Factotum controller
                                                    |
                                      exact attempt verdict to Session
                                                    |
                                      live custody + final attach checks
                                      (lock-provider integration above)
```

The local challenge record is 40 bytes: little-endian `u32` version 1, `u32`
kind 1, then the 32-byte nonce. A relay write and read use offset zero and whole
records. The 8-byte status is version 1 followed by a `u32` errno, zero on
success. A successful relay status is not itself an authenticated role attach.
Malformed length/version/kind refuses before queueing. The role afid transcript
and independent SDK exchange are described above.

Reserved relays do not forward even if Factotum's activation has already
completed. Only Session's matching activation opens that gate; a wrong launch
or a retired relay cannot activate it. A five-second relay deadline is not
renewed by activation. Factotum separately enforces the original afid attempt's
deadline. `RelayServer` services local expiry without child or Factotum activity,
and result notifications wake pending reads. Queue saturation returns EAGAIN.
Its core has one connection, four fids/tags, 64 KiB msize and 128 KiB output.
Session must multiplex these owners; the relay creates no thread.

`launch::queue` reserves one of 32 slots before allocating a request. That same
slot covers queue residence, the exchange, the unread result and cancellation.
The owner advances at most one exchange per turn. Dropped queued work never
executes. An abandoned successful Begin is cancelled remotely; an abandoned
reservation or activation is retired remotely. Cleanup retains the slot until
its answer and has a separate five-second bound, without renewing authentication.
Fatal cleanup failure closes the shared transport. Dropping the connected owner
closes that transport and invalidates even buffered results. Completion wake
callbacks must be nonblocking.

Relay-focused tests join the actual relay/core, shared client and Factotum
export. They supply Session admission, launch activation and PAM completion;
the protected-child joins above provide the separate custody evidence.
Full verification ingress coexists with
an admitted PAM login. Controls discriminate a 33rd reservation and bypass of
Session's Reserved gate. These relay tests alone do not qualify final attach,
per-role availability, aggregate afid charging or an independent client exchange.
