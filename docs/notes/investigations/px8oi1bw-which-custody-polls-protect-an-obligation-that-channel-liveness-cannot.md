---
id: px8oi1bw
date: 2026-11-08
kind: investigation
status: investigating
tags: [investigation, session, shell, custody, performance]
---
# Which custody polls protect an obligation that channel liveness cannot

## Question

The liv-01 counts (`~/.local/state/sophia-liveness-native-receipts-01`,
sealed `7c6d9a58…`, audited `68db201f…`) put the owner thread at 27
`LaunchCustody::is_live` polls and 7 image connector `process_alive` polls per
completed frame on the lean one-bar-wire lane. The counts closed under their
predeclared rule without triggering the custody-liveness authority review.
Before anyone asks for that review, this note answers, for each poll site:
what action the poll guards, whether the same death is already visible to the
owner by another route, and what window a different observation point would
open. It distinguishes a readiness notification, which is a wake reason, from
a fresh liveness observation, which is authority. It changes no source and
selects no mechanism.

## Method

Source read on master `0fdf784e9`. Counts from liv-01. Unit cost from the
device-free `sophia-custody-poll-cost-01` package as corrected by its sibling
`sophia-custody-poll-cost-correction-claude-01` (`7ccb2fa5…`) after Codex's
bounded review: one LaunchCustody-shaped zero-timeout poll took on the order
of 0.4 us per call in a tight userspace loop on this host, harness included,
consistent with the 408 ns ppoll mean in `sophia-custody-poll-screen-01`.
That is a microbenchmark figure, not a floor, a bound or an owner-thread
cost. Nothing here is a CPU claim.

## Custody on one transport entry

An entry is one `ShellFileWire::service` call (`shell_transport/files.rs`):
an input turn or a publication flush. The lean lane makes 4 turns and 3
flushes per frame. On one entry the owner reaches the custody pair this way:

| step | code | what runs | pidfd polls |
|---|---|---|---|
| pre-turn | `files.rs service_turn` → `export.poll` → `launch_authentication/files.rs poll` | `auth.poll` (Factotum attempt state), `worker.live()`, `grant.is_live()`, `admission.is_live()` | 1 (both custody fds) |
| 9P turn | `turn_observed` / `turn` / `flush` | requests; each protected file access runs `check` → `check_admitted` → `authority.authorize` → `live()` | 1 per protected access |
| post-turn | `files.rs service` after the turn | same `poll` again, then `expire()` (drops expired staging only) | 1 |
| image service | `files/image.rs service` → `check_image_peer` → `image_channel.rs ImageChannelPeer::check` | `process_alive(connector pidfd)`, then `check_side_channel` → `check` → `live()` | 1 connector + 1 custody |
| image accept | `files/image.rs service` on a flush (`listener_idle` is `None` when `flush_only`) | `accept4` on the listener | 0 |

liv-01 per frame: pre-turn 7, post-turn 7, per-file 6 (3 on turns, 3 on
flushes), image-check 7 custody plus 7 connector. A flush with nothing to
publish still pays pre, post, one file check and the image check: four
custody polls and one connector poll for zero requests served.

## Table A: what each poll guards

| site | action on a dead or withdrawn custody | who initiates the guarded action |
|---|---|---|
| pre-turn `poll` | `close(now)`: authentication closed permanently, admission dropped, `ESTALE` → `NotConnected` → `export.revoke()` (`files.rs service`) | Session, before serving any request on this entry |
| per-file `check` | `ESTALE` for that request only | the peer's request; Session replies |
| post-turn `poll` | as pre-turn; comment: "retain caller-fresh liveness" before `expire()` and image service | Session, before image service and before the next publication is readable |
| image `check` (Ready) | `State::Closing(GrantRevoked)`: channel torn down | Session, before reading the connector's datagram |
| connector `process_alive` | `STALE` on the join or the check | Session, before accepting or reading from the connector socket |
| `WorkerAuthority::live` itself | also refuses when `active` was withdrawn or ingress closed, before and after the poll | owner path retirement (`Authority::Owner`) |

Every guarded action is Session's own next step, not something the peer can
do. A dead peer sends nothing new. What a late observation could let through
is work Session completes for a process that has already exited: a reply
written to a socket nobody reads, an image datagram accepted from a connector
whose socket was inherited by a child, or a publication left readable for one
more entry. Which of those matters is the review's question; this note only
locates them.

## Table B: is the death already visible by another route

| route | code | what it is | what it is not |
|---|---|---|---|
| 9P wire readiness | `sophia-9p/src/unix.rs:294` treats `HUP` and `ERR` as readiness; the next turn reads EOF and drops the connection; `connection_count() == 0` revokes (`files.rs service`) | channel liveness of the connected process | custody of the launched wrapper and peer; a forked child holding the socket keeps the wire open |
| custody pidfds in the Receive wait | `authority_receive.rs:55-57` → `subscribe_wait` → `visit_poll_roles` → `visit_authentication_fds` → `WorkerAuthority::visit_wait_fds` → `LaunchCustody::visit_poll_fds` | a wake on exit of either exact process | an observation: `ShellReadinessSubscription::observe` reads fds 0..2 only and `ComponentWaits::observe` counts the cause; the exit is seen by the next pre-turn poll |
| custody pidfds in the Service wait | `authority_service_wait.rs` → `service_wait_components::subscribe` (since B3, `2070399e1`) | same wake | same |
| idle cadence | `launch_authentication/files.rs wait` returns 20 ms: "checks launch exit/revocation and deadlines when neither peer sends traffic" | a bound on detection latency when idle | a liveness observation |
| image socket | `receive_image_datagram` `PIPE` → `Closing(PeerDisconnect)` | connector socket closed | connector process exit while a child holds the socket; that is what `process_alive` catches |

So the owner already wakes on the death of either custody process, on both
wait kinds, and the comment in `launch_custody.rs:40-41` states the contract:
"Readiness is only a wake reason; callers must still use `is_live`." The
repeated polls are the fresh observations the contract asks for, made at
every boundary where Session is about to act. The wire's HUP covers the
connected process, not the launch custody, so it cannot replace the pidfd.

## Repeated observations inside one entry

Within one entry the pre-turn poll, the post-turn poll and the image check's
`live()` observe the same two pidfds at successive instants, and the per-file
checks observe them again once per protected access inside the turn. Of the
27 custody polls per frame, 7 are the first observation of an entry and 20
are later observations in the same entry. Either process can exit at any
instant, including between two of these observations, so a later observation
is not redundant by count: each one shortens the window between the last
observation and the Session action it precedes. What the count cannot say is
whether that shorter window protects anything. That is the question for the
review, per site: which Session action follows each later observation, and
what happens if the exit falls in the window the previous observation left.

The window that one observation per entry would open is the rest of that
entry: an exit after the pre-turn poll and before the image check. In that
window Session can serve the turn's replies and read one image datagram from
a connector whose own `process_alive` still runs. It cannot admit a new
attach, because `begin`, `read` and `commit` keep their own `grant` and
`admission` checks and the Factotum verdict, and it cannot extend custody,
because `active` is owned by Session and withdrawn before signals. Whether
serving those replies or reading that datagram for an exited process is
acceptable is for the authority to decide, not for this note.

## Prior art the review must not repeat

- c37 registered the custody pidfds in the owner wait set; both c41 arms had
  it, so no package measured registration against an unregistered baseline.
- c40 `CustodyDispatch` (`supervisor/launch_custody/dispatch.rs`, feature
  `experimental-custody-dispatch`, default off): one aggregate poll at scope
  entry, then at most 64 in-memory checks within 1 ms, scope wrapping the 9P
  turn and ending before the post-turn poll. c41
  (`development-evidence/t331-isb-cost-20261018/candidate-41-custody-dispatch-screen`,
  2026-10-20): eight files-panel runs, Session +8.1 us/frame candidate over
  control on the mean with same-arm spreads of 64 to 72 us; frozen screen
  refused on one major fault; "no consistent measured benefit; no promotion".
  Its own next discriminator was scope coverage and work per scope, which was
  never captured; liv-01's cell grid now supplies the per-site counts it asked
  for. The scope also added a poll rather than reusing the wait's, and it
  allocated per observation.
- `sophia-custody-poll-screen-01`: a level-triggered epoll substitute for the
  fresh poll was rejected from kernel source ordering (`pidfd_poll` reads
  `exit_state` directly; a zero-timeout `epoll_wait` only checks the ready
  list), so an empty epoll is not a fresh observation. Any mechanism the
  review accepts must keep a fresh `pidfd` poll wherever a fresh observation
  is required.
- The plan `4rifczhw` records that frequency alone is not cost and that the
  negative dispatch results stand unless new evidence arrives. The new
  evidence is the per-site count and the unit-cost bound, not a CPU result.

## Findings

1. Every custody poll site guards a Session action; none guards a peer
   action. The guarded actions are serving replies, servicing the image
   channel and leaving a publication readable.
2. The death of either custody process already wakes the owner on both wait
   kinds, and idle detection is bounded at 20 ms. What the repeated polls add
   is freshness at each boundary, under the stated contract that readiness is
   a wake, not authority.
3. Seven of the 27 polls per frame are the first observation of an entry;
   20 are later observations in the same entry. An exit can fall between
   any two of them, so the count alone does not make the later ones
   redundant; each shortens a window before a named Session action, and the
   review decides per site whether that window matters.
4. The owner-thread cost of these polls is not measured. The microbenchmark
   gives a per-call figure in a tight loop only, and the earlier note
   recorded that between-arm variability is not a resolution threshold.
   This note therefore asks for no CPU comparison and makes no claim about
   what one could resolve. The review's object is correctness: which
   observations the contract requires, and which mechanism keeps them.

## Limits

- No owner-thread measurement of wrapper work: `auth.poll`, lease checks and
  clocks are unpriced; the earlier inclusive figure cannot be decomposed
  against the microbenchmark (correction `7ccb2fa5…`).
- One bar wire, one image channel; a second wire multiplies every per-entry
  row. Lock wire image service (`lock_files/image_service.rs:222`) and the
  `experimental-custody-dispatch` turn were not traced.
- Revocation on the owner path (`Authority::Owner`, `LaunchBook`) was not
  traced; "cannot extend custody" rests on `active` being Session-owned.
- The window analysis names what Session can do in the window; it does not
  decide whether any of it is acceptable. That is the review.

## Next

This note is input to a custody-liveness authority review request, drafted
by Claude and recorded by Codex in `4rifczhw` and todo t344 under the
approved plan. The request will carry the per-site counts, the unit-cost
bound with its correction, Table A and Table B, and the three mechanisms in
the plan (one observation per entry, wake-poll observation reusing the
existing wait registration, reap on pidfd readiness), each with the refusal
fixtures that must still refuse and the receipt cell that must move. No CPU
arm is requested for any of them.

## Connections

- [Which component-walk visits do useful work per ready source](3u93ti2w-which-component-walk-visits-do-useful-work-per-ready-source.md)
  priced the custody poll as the largest component-walk term and placed
  custody caching out of its lane.
- [Reduce continuous-frame CPU without weakening custody](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
  owns the liv-01 result, the routing rule and the review decision.
- [Keep broker and portal file authority and custody separate](../decisions/xa78u03g-keep-broker-and-portal-file-authority-and-custody-separate.md)
  fixes that peer identity and the admission record confer authority, not
  transport state.
- [Readiness must name an obligation](../concepts/readiness-readiness-must-name-an-obligation.md)
  is the standard Table A applies.
- [Which Redox ring and Orbital patterns can reduce Sophia frame overhead safely](sy7lhvb8-which-redox-ring-and-orbital-patterns-can-reduce-sophia-frame-overhead-safely.md)
  supplies the pattern that liveness is a channel property and obligations
  hang off event sources; Table B shows where Sophia already has that and
  where the launch custody is the exception.
