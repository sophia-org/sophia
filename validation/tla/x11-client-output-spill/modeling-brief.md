# t165 client output spill

Category B: one connection's reader thread, its event writer threads, the
kernel's send buffer and the client on the other side. The issue is a client
that writes a burst before it reads: the server's replies fill the kernel's
buffer, the writer blocks, the reader stops reading, and the client's own
write never completes. XTS5's `TOO_LONG` purpose does this in 120 of 122
Xproto cases.

Scenarios:
1. A client writes its burst and then reads. Every reply it was owed arrives,
   in production order, and its requests were all read while it wrote.
2. A client writes and never reads. Output is kept up to a declared bound and
   the connection is ended at the bound, with the spill dropped, not kept.
3. A client that stops reading and stops writing while output is owed is
   ended after a silence allowance, so a watcher that goes quiet does not hold
   its output forever below the byte bound.
4. Today's writer: the first refused record holds the reader. The negative
   control reproduces the deadlock as a violated invariant.

Safety: `ReaderNeverWaitsOnRecipient`, `OutstandingBounded`, `Order` and
`EndedIsQuiet`. Liveness: `ReaderProgress`, `ReadingClientReceivesEverything`
and `SilentClientIsEnded` under weak fairness of the client's burst, the
reader, the event writers, the drain, a prompt client's reads, time and the
silence ending. A prompt client is one that takes what the kernel holds
before the allowance clock advances; no such promptness is assumed of a
client that has stopped reading. Byte sizes, descriptor passing with a
record's first bytes, and the private ordered writer's own six-second
custody policy are Rust obligations, not modelled here.

Finite configuration: a burst of three requests and two writer events, a
kernel that holds two records, a spill bound of two, a silence allowance of
three ticks and a clock that saturates at six. All production happens by
tick 1, so the clock outlives every deadline a run can reach.
