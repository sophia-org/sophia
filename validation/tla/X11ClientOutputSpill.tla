-------------------- MODULE X11ClientOutputSpill --------------------
EXTENDS Naturals, Sequences

\* One X11 client connection's output, as t165 changes it: what the kernel's
\* send buffer refuses is kept in order in a per-connection spill instead of
\* blocking the thread that reads the client's requests, a drain thread moves
\* the spill into the kernel as the client reads, and two bounds end a client
\* that will not take its output -- a byte bound, and a silence allowance for
\* a client that neither reads nor writes.
\*
\* Spill = FALSE is today's blocking writer, the negative control.

CONSTANTS Spill, EndAtBound, EndOnSilence, PromptReader,
          Bound, Kernel, Burst, Events, Allowance, LastTick

VARIABLES inbox, written, clientPhase, produced, kernel, spill, received,
          readerBlocked, ended, silence, now, events
vars == <<inbox, written, clientPhase, produced, kernel, spill, received,
          readerBlocked, ended, silence, now, events>>

Phases == {"writing", "reading", "gone"}
Records == 1..(Burst + Events)

Init ==
    /\ inbox = 0 /\ written = 0 /\ clientPhase = "writing" /\ produced = 0
    /\ kernel = <<>> /\ spill = <<>> /\ received = <<>>
    /\ readerBlocked = FALSE /\ ended = FALSE /\ silence = 0 /\ now = 0
    /\ events = Events

KernelHasRoom == Len(kernel) < Kernel

\* One record leaving the server, numbered in production order, which is the
\* order the output mutex admits it in.
\* crates/sophia-x-authority/src/x11_socket/connection/io.rs write_x11_socket_output_record:
\* into the kernel when it has room and nothing is queued ahead; else onto the
\* spill (Spill), ending the connection when the spill passes its bound; else,
\* today, held by a writer that blocks -- and the reader with it.
Emit ==
    /\ produced' = produced + 1
    /\ IF KernelHasRoom /\ spill = <<>> THEN
           /\ kernel' = Append(kernel, produced')
           /\ spill' = spill /\ readerBlocked' = FALSE /\ ended' = ended
       ELSE IF Spill THEN
           /\ kernel' = kernel /\ readerBlocked' = FALSE
           /\ IF EndAtBound /\ Len(spill) + 1 > Bound
                  THEN spill' = <<>> /\ ended' = TRUE
                  ELSE spill' = Append(spill, produced') /\ ended' = ended
       ELSE
           /\ kernel' = kernel /\ spill' = Append(spill, produced')
           /\ readerBlocked' = TRUE /\ ended' = ended

\* The client writes its burst, then reads. All writing happens by tick 1 so
\* the clock saturates after every deadline this run can reach.
ClientWrite ==
    /\ clientPhase = "writing" /\ written < Burst /\ ~ended /\ now <= 1
    /\ inbox' = inbox + 1 /\ written' = written + 1
    /\ clientPhase' = IF written + 1 = Burst THEN "reading" ELSE "writing"
    /\ UNCHANGED <<produced, kernel, spill, received, readerBlocked, ended,
                    silence, now, events>>

\* A client that stops reading for good, at any point.
ClientStopReading ==
    /\ clientPhase # "gone" /\ clientPhase' = "gone"
    /\ UNCHANGED <<inbox, written, produced, kernel, spill, received,
                    readerBlocked, ended, silence, now, events>>

\* crates/sophia-x-authority/src/x11_socket/connection/dispatch.rs: the reader
\* takes one request and answers it. Reading is the client's activity, so it
\* resets the silence. With Spill the reader is never held by the recipient.
ServerRead ==
    /\ inbox > 0 /\ ~readerBlocked /\ ~ended
    /\ inbox' = inbox - 1 /\ silence' = 0
    /\ Emit
    /\ UNCHANGED <<written, clientPhase, received, now, events>>

\* An event writer thread -- input, protocol, control -- emitting under the
\* same mutex, independent of the client's requests.
EventEmit ==
    /\ events > 0 /\ ~readerBlocked /\ ~ended /\ now <= 1
    /\ events' = events - 1
    /\ Emit
    /\ UNCHANGED <<inbox, written, clientPhase, received, silence, now>>

\* crates/sophia-x-authority/src/x11_socket/connection/writers.rs
\* spawn_x11_output_drain: the head of the spill moves into the kernel when it
\* has room. Progress resets the silence. In today's design this is the
\* blocked write completing, which frees the reader.
Drain ==
    /\ spill # <<>> /\ KernelHasRoom /\ ~ended
    /\ kernel' = Append(kernel, Head(spill)) /\ spill' = Tail(spill)
    /\ silence' = 0
    /\ readerBlocked' = IF Spill THEN FALSE ELSE Tail(spill) # <<>>
    /\ UNCHANGED <<inbox, written, clientPhase, produced, received, ended,
                    now, events>>

\* The client takes what the kernel holds for it. An ended connection still
\* yields what the kernel had; the spill it dropped never arrives.
ClientRead ==
    /\ clientPhase = "reading" /\ kernel # <<>>
    /\ received' = Append(received, Head(kernel)) /\ kernel' = Tail(kernel)
    /\ UNCHANGED <<inbox, written, clientPhase, produced, spill,
                    readerBlocked, ended, silence, now, events>>

\* Time in allowance ticks. It does not pass while the server is reading a
\* request, the drain has room to move a record into, or a prompt client has
\* something to take: those are activity within one slice, and the allowance
\* measures its absence. It passes when the kernel is full and nobody reads.
ServerBusy == inbox > 0 /\ ~readerBlocked /\ ~ended
DrainBusy == spill # <<>> /\ KernelHasRoom /\ ~ended
ClientBusy == PromptReader /\ clientPhase = "reading" /\ ~ended
              /\ (kernel # <<>> \/ spill # <<>>)
Tick ==
    /\ now < LastTick /\ ~ServerBusy /\ ~DrainBusy /\ ~ClientBusy
    /\ now' = now + 1
    /\ silence' = IF spill # <<>> /\ ~ended THEN silence + 1 ELSE 0
    /\ UNCHANGED <<inbox, written, clientPhase, produced, kernel, spill,
                    received, readerBlocked, ended, events>>

\* The drain thread's clock: output owed, and neither drained nor any request
\* read for the whole allowance, ends the connection and drops what it owed.
EndSilent ==
    /\ EndOnSilence /\ ~ended /\ spill # <<>> /\ silence >= Allowance
    /\ ended' = TRUE /\ spill' = <<>>
    /\ UNCHANGED <<inbox, written, clientPhase, produced, kernel, received,
                    readerBlocked, silence, now, events>>

Next == ClientWrite \/ ClientStopReading \/ ServerRead \/ EventEmit
        \/ Drain \/ ClientRead \/ Tick \/ EndSilent

Spec == Init /\ [][Next]_vars
        /\ WF_vars(ClientWrite) /\ WF_vars(ServerRead) /\ WF_vars(EventEmit)
        /\ WF_vars(Drain) /\ WF_vars(ClientRead) /\ WF_vars(Tick)
        /\ WF_vars(EndSilent)

TypeOK == /\ inbox \in 0..Burst /\ written \in 0..Burst /\ inbox <= written
          /\ clientPhase \in Phases
          /\ produced \in 0..(Burst + Events)
          /\ kernel \in Seq(Records) /\ Len(kernel) <= Kernel
          /\ spill \in Seq(Records) /\ received \in Seq(Records)
          /\ readerBlocked \in BOOLEAN /\ ended \in BOOLEAN
          /\ silence \in 0..LastTick /\ now \in 0..LastTick
          /\ events \in 0..Events

\* The reader is never held by a recipient that will not take its output.
ReaderNeverWaitsOnRecipient == ~readerBlocked
\* What a connection may owe is bounded; beyond it the connection is ended.
OutstandingBounded == Len(spill) <= Bound
\* Everything leaves in production order, across the three places a record
\* can be, with nothing skipped ahead: received, then the kernel, then the
\* spill, read together, are 1, 2, 3, ...
Order == LET all == received \o kernel \o spill
         IN \A i \in 1..Len(all) : all[i] = i
\* An ended connection owes nothing: the spill was dropped, not kept.
EndedIsQuiet == ended => spill = <<>>

\* The reader drains every request it was given, or the connection ended.
ReaderProgress == (inbox > 0) ~> (inbox = 0 \/ ended)
\* A prompt client that keeps reading receives every record produced.
ReadingClientReceivesEverything ==
    (clientPhase = "reading" /\ ~ended /\ inbox = 0 /\ events = 0)
        ~> (Len(received) = produced \/ clientPhase = "gone" \/ ended)
\* A client that stops reading while the server still holds output for it is
\* ended, unless what it was owed fit into the kernel's buffer after all --
\* then the server owes nothing and has no reason to act.
SilentClientIsEnded ==
    (clientPhase = "gone" /\ spill # <<>>) ~> (ended \/ spill = <<>>)
=======================================================================
