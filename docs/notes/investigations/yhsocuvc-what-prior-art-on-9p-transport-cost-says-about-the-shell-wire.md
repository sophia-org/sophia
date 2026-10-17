---
id: yhsocuvc
date: 2026-10-17
kind: investigation
status: investigating
tags: [investigation, shell, 9p, performance]
---
# What prior art on 9P transport cost says about the shell wire

## Question

t331 must bring the shell file wire's CPU and allocations per frame within
1.10 times the paired IPC path. The
[reconciled ledger](2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md)
attributes the gap to Session passes, one per dependent client round trip,
and the approved first slice (the reusable transaction writer, Sophia
`ee22309ba`, adopted with both SDKs in `a3c94bb86`) removed the walk, open
and clunk per record. The candidate ledger then showed that client-side ACK
coalescing saved one write per frame rather than the six the plan assumed.
This note asks what other 9P-family transports learned about the same cost,
which of it transfers to the shell wire, and which of it does not. It also
answers niltempus's question whether tcell's draw path holds anything
actionable for the shell.

This is a reading of published work and one library, done on 2026-10-17 as a
read-only consult. Nothing in it measures Sophia. The pass model it supports
remains a hypothesis until the native pilots attribute passes by cause on
both wires.

## Evidence

Sources, with the retained copy noted where one exists. The survey text with
all citations is kept at
`~/.local/state/sophia/development-evidence/t331-claude-consult-20261017/9p-transport-cost-survey.md`;
the Op and ZX papers are public PDFs under the author's `fjballest/docs`
repository on GitHub.

**Op: Styx batching for high latency links** (Ballesteros, Guardiola,
Soriano, Lalis; IWP9 2007). The problem statement is the one the ledger
arrived at independently: Styx and 9P "behave badly for high latency links,
no matter what the bandwidth is because of the RPC round-trip time
accumulation", since "an RPC issuer waits synchronously for the responses
before issuing new RPCs". The authors single out the fid lifecycle: "fids
require extra RPCs to maintain them (and thus extra latency)". Op has no
walk, open or clunk. The first put or get in a series carries the path and an
OMORE flag; the server allocates a descriptor and the last message in the
series frees it, "without requiring extra RPCs for open or close
operations". One Tput creates, writes and sets metadata in a single RPC. One
Tget returns metadata and data and may answer with several replies to one
request. The measured effect was that "the number of RPCs for normal usage of
a remote file tree is reduced an order of magnitude", and the protocol "is
worth its overhead when RTT goes above 1 millisecond". The paper also records
why it rejected the alternative of modifying Styx so that "the client can
send RPCs using the same tag without waiting for the answers" with an error in
the first RPC cancelling the rest: "many programs have to be modified. This
includes both clients and servers."

**ZX: a distributed file system for high latency networks** (Ballesteros,
Guardiola, Soriano; Lsub technical report, later Software: Practice and
Experience 2018). ZX abandons batching and caching as the primary remedy. An
RPC "is actually a stream of input data for the RPC, sent through a channel,
and a stream of output data sent through a reply channel"; tags identify the
channel, not a single message, so that a connection multiplexes several
streams. "There are no data fields in any of the messages. If there is data
flowing to (or from) the server due to a request, the data follows the request
in the channel used to issue the request." The client is optimistic: it
"is already streaming data while the server starts processing the request",
and a refusal closes the channel so that later sends fail at the client.
Traversal moves to the server through find and findget, which return a stream
of entries or entries plus data for one request. Atomicity attaches to the
request message, "not the request stream". The authors' recorded design error
is directly relevant: an early ZX made a whole put atomic, which "kept files
locked while data was being sent from the client", and "a broken client may
hold the lock on the file for too long (or forever)"; they now release the
lock once the data phase begins. Measured: finding a file over a 50 ms RTT
took ZX 5.2 s against 1231 s for NFSv4 and 706 s for SSHFS, while on the
loopback ZX ran at roughly a third of NFS's operations per second through its
FUSE adaptor. The paper is explicit that ZX is the wrong choice for a
low-latency network and the right one once latency dominates.

**The 9front 9P2020 sketch** (aiju.de). Proposes chaining (a request names an
earlier tag it depends on, so the server must remember which requests
completed, with a suggested bound of 64), grouping (messages with the same
group tag are not reordered, meant for "walk and open"), and streaming
Treads and Twrites with a 64 KB client window. None shipped. The page is
candid that chaining's cost is server-side memory of completions and that
grouping "might be vastly simpler to implement".

**vNFS** (Chen et al., FAST 2017). NFSv4 compounds existed from the first
NFSv4 RFC, yet "compounding has been underused since its conception because
the synchronous POSIX file-system API issues only one (small) request at a
time"; in practice "each compound contains only one significant operation".
A vectorized API above the wire produced speedups of up to 133 times at
latencies at or below 5.2 ms and 263 times at 30 ms, and a 4 times faster cp
on a LAN. The same group's earlier pattern paper, "Batching: a design pattern
for flexible and efficient client-server interaction" (TPLoP 2009), makes the
general point.

**WSL2 drvfs** (brainwagon.org, July 2026). 9P over Hyper-V sockets to the
Windows host: about 5.8 ms per file create and delete on `/mnt/c` against
about 0.1 ms on ext4 on the same physical disk, while a sequential 1 GiB
write was only seven times slower. Per-request serialization dominates
small-operation traffic; bytes dominate bulk traffic.

**Linux v9fs and virtio-9p.** The kernel documentation's levers are `msize`
and the cache modes; a 2021 measurement moved a sequential read from about
95 MB/s at a 16 KiB msize to about 1846 MB/s at 512 KiB. The diod server
README warns that 9P "has no way for the server to invalidate cached
objects". Floren's RIT thesis (2010) added out-of-band streams so that
sequential 9P transfers ran "at equal or superior speeds to HTTP". Van
Hensbergen and Minnich (FREENIX 2005) describe the Linux client and its
comparison with NFS for a static tree.

**tcell** (gdamore/tcell, head `cf5820d`, cloned to `~/src/tcell` on crunch
from the same commit niltempus holds on the x13). A terminal cell library.
Its cell buffer keeps the last shown string and style per cell and defines
dirtiness as a comparison, not a flag (`cell.go` lines 156 to 170). Its draw
pass writes only dirty cells, skips cursor moves and style sequences when
position or style are unchanged, and emits everything into one buffer that it
writes once per frame (`tscreen.go` lines 860 to 899 and 1043 to 1098).
`Show` is that incremental pass; `Sync` invalidates every cell and clears the
screen, and the interface documentation reserves it for resize and
corruption (`screen.go` lines 146 to 161). Its history holds two cautionary
fixes: a width comparison that marked every cell dirty on every frame
(`c2fa7b1`), and a fill path that allocated once per cell (`5a6c8bf`).

## Finding and resolution

Six points transfer to the shell wire.

The second slice is the mainstream remedy, not an experiment. Every
historical fix for 9P-family cost removed dependent waits: Op's implicit open
and close, ZX's optimistic streams, the 9P2020 chaining and grouping sketch,
Floren's streams. The first t331 slice removed the open and close. The
remaining panel cost is three waited rounds per record and one per pixel
chunk, which is the queued submit (B2) and pipelined pixel writes (B3) in
the plan. Op's objection to pipelining under one tag, that every client and
server would need modification, does not apply here because Sophia owns both
SDKs and the export.

Prefer grouping semantics to chaining. The contract's wire-order rule, with
"a short or failed write invalidates the writes queued behind it", is the
grouping shape and needs no server memory of completed requests. ZX's
optimistic stream has the same failure model: a refusal closes the stream and
the client's later sends fail.

Do not hold anything exclusive across a stream. ZX's abandoned atomic put is
the warning. An upload between Begin and End must keep its existing admitted
slot, budget and deadline semantics and must not acquire any Session resource
for the duration of the client's streaming.

The ACK fix is on the server side. Client coalescing cannot do better than
one ACK per read while the export answers the pending events read on every
journal append. Op's multi-reply get, ZX's reply streams and Floren's
streams all let the server choose reply boundaries. Answering the pending
read at the end of the Session component pass, so that Submitted and its
consequence travel in one Rread when the owner produced them in one pass,
is already permitted by the contract and cuts reads and ACKs together. It
must be shown by the deterministic ledger not to produce EAGAIN or ENOSPC
from delayed retention release.

Batch in the SDK outbox and nowhere else. vNFS shows that a compound wire is
useless when the API above it issues one operation at a time. The outbox
already sees whole frames: Begin, chunks, End, FrameDemand, the candidate
group and Retire. Neither the component API nor the shell clients should
change.

Panel and bulk fail for different reasons. The WSL2 figures and the ledger
agree: small-record traffic pays per request, pixel traffic per byte and per
chunk pass. The panel gate turns on B2 and the ACK cadence; the bulk gate
turns on B3 alone. A pilot cannot judge both unless it emits passes per
frame by cause on both wires.

Three points do not transfer. Op's fid-less, path-in-every-request design
trades custody for statelessness; Sophia's contract is built on fid custody
for admission, revocation and the ESTALE fences, so the lesson is to amortize
the fid, which the reusable writer does, not to drop it. Client caching (Op's
coherency window, v9fs loose caches, cfs) suits read-mostly trees; the shell
wire is write-mostly with server-authored events and nothing cacheable in the
steady frame. Shared memory and DAX are the deferred DMA-BUF and memfd
contract, outside version 1. A fully implicit open and close for upload
series, in Op's manner, is worth recording as a candidate for a later
contract revision; it is not part of t331.

tcell has nothing for the t331 gate, which is a ratio at fixed work. It has
two items for the work itself, recorded as candidates below, and one piece
of attribution advice: when t331 attributes allocations by site in the native
pilot, the patterns to look for in the per-pass phases are allocation per
item inside a fill or collect loop and repeated work on input identical to
the previous frame, which are the two patterns tcell's own performance
history fixed.

## Validation and remaining work

This note contains no measurement of Sophia. Its claims about the shell wire
rest on the source ledger in the linked investigation and on the candidate
ledger evidence under
`~/.local/state/sophia/development-evidence/t331-reuse-20261017/candidate-ledger/`.
The decisions it bears on are open: the second slice (B2 and B3) and the ACK
cadence are niltempus's to approve under the t331 plan, and the native pilots
are the next evidence gate.

## t334: measure and skip unchanged shell frames

Measure how often each production shell component submits a frame whose
pixels are identical to the previous one, on the installed desktop, using
the existing per-role diagnostics and no new client code. If the rate is
material for any role, add an unchanged-frame short-circuit in the SDK
outbox, in both the C and Rust SDKs, that compares the staged pixels against
the last accepted resource and skips the Begin, chunk, End, candidate and
Retire cycle for that frame while still answering the frame demand. The
comparison must be cheaper than the cycle it removes; a per-chunk digest kept
from the last upload is the expected shape. This is tcell's dirty check
applied to a whole frame. It changes work, not the ratio, so it is a t289
lever and must stay out of the t331 comparator workload. Exit: a measured
rate per role from the installed desktop, and either a recorded decision not
to proceed or the short-circuit landed in both SDKs with the paired
conformance tests and the pass count per skipped frame recorded.

## t335: damage update for retained shell resources

Specify a damage update for a retained shell resource: a sub-rectangle write
into a retained resource that keeps presented pixels immutable, through a
generation or double-buffer rule, so that a component can present the
changed region of a panel instead of re-uploading all of it. This is tcell's
Show against Sync. It is a content-contract revision candidate and must
follow the t252 shell acceptance; it touches resource immutability,
retirement and the compositor's hold on presented pixels, so it needs its
own decision record. Exit: a contract section with the custody and
retirement rules, paired SDK support, and the panel's steady-state bytes and
Session passes per frame measured against the full-upload path on the
installed desktop.

## Connections

The question comes from the
[reconciled request ledger and ACK cadence finding](2o32e7ui-which-plan-9-implementation-techniques-can-reduce-sophia-shell-costs.md)
and the
[t331 plan](../plans/mvbubiv9-reduce-shell-9p-allocation-overhead-against-the-paired-ipc-baseline.md),
whose approved first slice is the
[reusable transaction writer decision](../decisions/7mls2e2q-reuse-a-shell-transaction-writer-without-changing-custody.md)
and the [shell files contract](../../sophia-shell-files.md). The damage
candidate relates to the damage sections of the
[t289 CPU plan](../plans/7habxzm4-next-sophia-cpu-reductions-under-real-animated-workloads.md),
which already treats repaint damage inside the Engine; t335 would extend the
idea to the shell content wire. The upload concurrency candidate in
[t332](gjov8u9g-can-independent-shell-uploads-progress-concurrently-without-weakening-custody.md)
is the other open question about the same upload path. The earlier
[Kitty protocol review](u399qgtv-kitty-protocols-as-interoperability-evidence-for-sophia-portals.md)
reached the portal and input conclusions and, as recorded there, has no
bearing on this transport question.
