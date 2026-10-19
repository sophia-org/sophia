# Experimental shell read scheduling

The `sophia-shell-client/experimental-write-read-scheduling` feature is a t331
performance candidate, disabled by default. It forwards the matching feature
of `sophia-9p-client`. No Sophia import pin or default SDK behavior changes.
There is no measured performance benefit or release qualification yet.

Each shell file drive creates a fresh `PollCycle`. Its first pipeline round
always takes the ordinary write/read path. A later round can defer its socket
read only when the preceding round actually reached WouldBlock without reading
any bytes, and the current round successfully wrote bytes. Zero-budget,
read-call-limited and reply-buffer-limited rounds do not establish that history.
Partial reply bytes count as productive even if no complete reply was decoded.
A deferral clears the history: the following round reads normally, including
when it writes. No-write rounds read normally. Buffered frame decoding remains
active during a deferral.

The experiment adds `PollCycle` and `Pipeline::poll_in_cycle` only under its
feature. Existing `poll`, `poll_with_budget`, `wait` and startup paths retain
their behavior; other role clients do not opt in. `None` supplies the ordinary
per-round budget; an explicit shared budget is never replenished. A deferred
read spends no read credit, so budget exhaustion and the shell's immediate-work
flag can occur on a different round. The fixed maximum round count is unchanged.

## Observable scheduling differences

A reply or EOF that arrives after a successful write may be observed by a later
round or the next drive instead of the current call. Readable interest remains
enabled, so a level-triggered external loop still services pending bytes or HUP.
A failed write still poisons the connection in the same round. Valid replies
received before EOF remain drainable before closure classification.

Consequently, an enqueue-and-drive call may return before it observes closure;
custody remains provisional until the later observation settles it. An ACK reply
can remain pending longer. A submit's EAGAIN reply can be processed at a later
dispatch timestamp, from which its retry deadline is calculated. These are
observable scheduling changes, not claims of identical call-by-call timing.
Wire records, byte order, ACK contents, custody classification rules, authority
checks and image-source handling remain unchanged.

Reading EOF does not require that drive to return an error. FileWire can return
`Ok` after processing a final valid Submitted event and settling custody, while
`take_content` and `io_interest` report `PeerClosed`. Callers must follow those
existing receive and readiness results, not assume a particular drive reports
`PeerClosed` itself.

## Controls and remaining gates

The pipeline scheduling controls use socket pairs with replies queued before
the tested round, proving deferral without a timing race. They cover next-entry
delivery, no consecutive deferrals, no-write and productive-read fallbacks,
partial replies, half-close with a final reply, failed writes, zero budgets and
shared syscall-credit exhaustion. Existing shell scripted custody, malformed
journal, retry, batch and dispatch tests also run with the feature enabled.

The shell dispatch control also checks bounded event delivery and immediate
service interest while buffered events remain, with the feature on and off.
Three public FileWire controls deliberately leave queued input unread with zero
I/O credit, then service it in a later dispatch. In both feature modes they pin
final Submitted custody and the closure API sequence, pending versus confirmed
ACKs, and EAGAIN retry deadlines derived from observation time. They establish
the delayed-input contract independently of the pipeline deferral controls.
They do not force the deferred-write/peer-close interleaving inside a FileWire
drive; that integration interleaving remains untested. In particular, a
background-thread timing race would not supply deterministic evidence of it.

These deterministic controls do not establish a CPU improvement. A feature-on
versus feature-off native CPU comparison and the applicable Sophia export gates
remain required before enabling the candidate by default or adopting new pins.
The candidate-17 diagnostic counts describe observed exposure, not expected
savings after scheduling changes.

## C comparison

The C SDK has no equivalent repeated socket polling within its session dispatch:
`ss_pump` gives the I/O budget only to pass zero; subsequent passes use zero
budget for local processing and queueing. Its requests and later observations
therefore already span dispatch boundaries. No C change is needed to reproduce
this Rust-specific experimental optimization. No new role capability or wire
contract is introduced; this does not close any outstanding t333 parity gate.
