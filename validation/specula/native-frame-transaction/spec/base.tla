------------------------------ MODULE base ------------------------------
EXTENDS Integers, FiniteSets, Sequences, TLC

CONSTANTS Frames, Heads, CreditLimit, Capacity, WorkerLimit, Fault, AllowReopen
VARIABLE s
vars == <<s>>
Kinds == {"source", "prepared", "terminal"}
Empty == [phase |-> "absent", epoch |-> 0, facts |-> 0, stream |-> 0,
          reading |-> FALSE, valid |-> FALSE, released |-> FALSE, handed |-> FALSE, preparedQueued |-> FALSE,
          reused |-> FALSE, prepared |-> {}, submitted |-> {}, flipped |-> {},
          owed |-> {}, terminal |-> "none", charged |-> FALSE]
Initial == [epoch |-> 1, facts |-> 1, stream |-> 1, streamFacts |-> 1,
            unsafeReopen |-> FALSE, open |-> TRUE, credits |-> CreditLimit,
            tx |-> [f \in Frames |-> Empty], queue |-> <<>>,
            flight |-> [h \in Heads |-> 0], visible |-> [h \in Heads |-> 0],
            closeDue |-> TRUE, closedNotice |-> FALSE,
            refused |-> {}, retired |-> FALSE, freed |-> FALSE,
            unsafeSubmit |-> FALSE, unsafeEpoch |-> FALSE]
Init == s = Initial
Current(f) == s.tx[f].epoch = s.epoch /\ s.tx[f].facts = s.facts
Live(f) == s.tx[f].phase # "absent"
Held == {f \in Frames : Live(f) /\ s.tx[f].epoch = s.epoch
                            /\ s.tx[f].stream = s.stream /\ s.tx[f].terminal = "none"}
Obligations == {p \in (Frames \X Kinds) : p[2] \in s.tx[p[1]].owed}
Used == Cardinality(Obligations) + Len(s.queue) + IF s.closeDue THEN 1 ELSE 0
Event(f, k) == [frame |-> f, kind |-> k, epoch |-> s.tx[f].epoch]
NoReader(f) == ~s.tx[f].reading

\* D1: escrow before consuming the complete bounded input, including async copy.
Admit(f) ==
    /\ ~Live(f) /\ f \notin s.refused /\ s.open
    /\ Cardinality({g \in Frames : s.tx[g].reading}) < WorkerLimit
    /\ (s.credits > 0 \/ Fault = "unreserved-copy") /\ Used + 3 <= Capacity
    /\ s' = [s EXCEPT !.credits = IF Fault = "unreserved-copy" THEN @ ELSE @ - 1,
             !.tx[f] = [Empty EXCEPT !.phase = "copy", !.epoch = s.epoch,
                        !.facts = s.facts, !.stream = s.stream, !.reading = TRUE, !.owed = Kinds,
                        !.charged = Fault # "unreserved-copy"]]

\* A refusal needs a slot too; a full budget leaves input untouched.
Refuse(f) ==
    /\ ~Live(f) /\ f \notin s.refused
    /\ (~s.open \/ s.credits = 0) /\ Used < Capacity
    /\ s' = [s EXCEPT !.refused = @ \cup {f},
             !.queue = Append(@, [frame |-> f, kind |-> "refusal", epoch |-> s.epoch])]

\* D2: arbitrary captured bytes; matching producer generation is not authentication.
CopyDone(f) ==
    /\ s.tx[f].reading
    /\ s' = [s EXCEPT !.tx[f].reading = FALSE,
             !.tx[f].phase = IF @ = "copy" THEN "copied" ELSE @]
Validate(f, good) ==
    /\ s.tx[f].phase = "copied"
    /\ s' = [s EXCEPT !.tx[f].valid = good,
             !.tx[f].phase = IF good /\ Current(f) THEN "ready" ELSE "aborted"]
Cancel(f) ==
    /\ Live(f) /\ s.tx[f].terminal = "none" /\ s.tx[f].phase # "aborted"
    /\ ~s.tx[f].handed /\ s.tx[f].submitted = {}
    /\ s' = [s EXCEPT !.tx[f].phase = "aborted"]

\* D3: source publication is independent of candidate terminal settlement.
PublishSource(f) ==
    /\ "source" \in s.tx[f].owed /\ (NoReader(f) \/ Fault = "early-source")
    /\ s' = [s EXCEPT !.tx[f].owed = @ \ {"source"},
             !.queue = Append(@, Event(f, "source"))]
Reuse(f) ==
    /\ s.tx[f].released /\ ~s.tx[f].reused
    /\ s' = [s EXCEPT !.tx[f].reused = TRUE]

\* D4: no head submits before complete validated preparation; every submit is fresh.
Prepare(f, h) ==
    /\ s.tx[f].phase = "ready" /\ Current(f) /\ s.tx[f].handed
    /\ h \notin s.tx[f].prepared
    /\ s' = [s EXCEPT !.tx[f].prepared = @ \cup {h}]
Handoff(f) ==
    /\ s.tx[f].phase = "ready" /\ Current(f)
    /\ ~s.tx[f].handed
    /\ s' = [s EXCEPT !.tx[f].handed = TRUE]
PublishPrepared(f) ==
    /\ s.tx[f].handed /\ "prepared" \in s.tx[f].owed
    /\ s.tx[f].phase = "ready" /\ Current(f)
    /\ s' = [s EXCEPT !.tx[f].owed = @ \ {"prepared"}, !.tx[f].preparedQueued = TRUE,
             !.queue = Append(@, Event(f, "prepared"))]
Submit(f, h) ==
    /\ s.tx[f].phase = "ready" /\ h \notin s.tx[f].submitted
    /\ (s.tx[f].prepared = Heads \/ Fault = "partial-scene")
    /\ (Current(f) \/ Fault = "stale-submit")
    /\ s.flight[h] = 0 /\ ("prepared" \notin s.tx[f].owed \/ Fault = "partial-scene")
    /\ s' = [s EXCEPT !.flight[h] = f, !.tx[f].submitted = @ \cup {h},
             !.unsafeSubmit = @ \/ ~s.tx[f].valid \/ s.tx[f].prepared # Heads,
             !.unsafeEpoch = @ \/ ~Current(f)]
\* Exact callback identity is represented by flight[h]; stale callbacks stutter.
Flip(h) ==
    /\ s.flight[h] # 0
    /\ LET f == s.flight[h] IN
       s' = [s EXCEPT !.flight[h] = 0, !.visible[h] = f,
             !.tx[f].flipped = @ \cup {h}]

\* D5: terminal transfer, not Prepared/submit/ACK, returns credit and retires.
Terminal(f) ==
    /\ Live(f) /\ s.tx[f].terminal = "none"
    /\ (NoReader(f) \/ Fault = "cancel-before-handoff")
    /\ (1 \in s.tx[f].flipped \/ s.tx[f].phase = "aborted"
        \/ ~Current(f))
    /\ LET presented == 1 \in s.tx[f].flipped /\ Current(f)
           sameEpoch == s.tx[f].epoch = s.epoch
           sameStream == sameEpoch /\ s.tx[f].stream = s.stream
           emit == sameEpoch
       IN s' = [s EXCEPT
           !.tx[f].terminal = IF ~emit THEN "discarded"
                             ELSE IF presented THEN "presented" ELSE "rejected",
           !.tx[f].owed = @ \ {"prepared", "terminal"},
           !.credits = IF s.open /\ sameStream THEN @ + 1 ELSE @,
           !.retired = @ \/ presented,
           !.queue = IF emit THEN Append(@, Event(f, "terminal")) ELSE @]

\* D6: revocation discards old logical notices, never physical in-flight ownership.
ChangeFacts == s.facts = 1 /\ s' = [s EXCEPT !.facts = 2,
    !.open = IF Fault = "retarget-stream" THEN @ ELSE FALSE,
    !.credits = IF Fault = "retarget-stream" THEN @ ELSE 0]
Close == s.open /\ s' = [s EXCEPT !.open = FALSE, !.credits = 0]
CloseNotice ==
    /\ ~s.open /\ s.closeDue
    /\ s' = [s EXCEPT !.closeDue = FALSE,
             !.queue = Append(@, [frame |-> 0, kind |-> "close", epoch |-> s.epoch])]
StreamDrained ==
    /\ s.closedNotice /\ ~s.closeDue /\ Len(s.queue) = 0
    /\ (\A f \in Frames : Live(f) /\ s.tx[f].epoch = s.epoch =>
            s.tx[f].terminal # "none" /\ NoReader(f) /\ s.tx[f].owed = {})
Reopen ==
    /\ AllowReopen /\ ~s.open /\ s.stream = 1
    /\ (StreamDrained \/ Fault = "early-reopen")
    /\ s' = [s EXCEPT !.stream = 2, !.streamFacts = s.facts,
             !.open = TRUE, !.credits = CreditLimit,
             !.closeDue = TRUE, !.closedNotice = FALSE,
             !.unsafeReopen = @ \/ ~StreamDrained]
Revoke ==
    /\ s.epoch = 1
    /\ s' = [s EXCEPT !.epoch = 2, !.facts = 1, !.open = TRUE,
             !.credits = CreditLimit, !.stream = 1, !.streamFacts = 1,
             !.queue = <<>>, !.closeDue = TRUE,
             !.closedNotice = FALSE,
             !.tx = [f \in Frames |-> [s.tx[f] EXCEPT !.owed = {}]]]
\* Old source tickets still finish physically, but no old notice enters new journal.
DiscardOld(f) ==
    /\ Live(f) /\ s.tx[f].epoch # s.epoch /\ s.tx[f].owed # {}
    /\ s' = [s EXCEPT !.tx[f].owed = {}]
DrainHead(h) ==
    /\ s.visible[h] \in Frames /\ s.flight[h] = 0
    /\ LET f == s.visible[h] IN
       /\ ~Current(f)
       /\ s' = [s EXCEPT !.visible[h] = -1]
\* 0 is the original predecessor. Returning to it is disallowed after retirement.
RetireBlank(h) ==
    /\ s.retired /\ s.visible[h] = 0 /\ s.flight[h] = 0
    /\ s' = [s EXCEPT !.visible[h] = -1]
FreePredecessor ==
    /\ s.retired /\ ~s.freed
    /\ ((\A h \in Heads : s.visible[h] # 0) \/ Fault = "early-free")
    /\ s' = [s EXCEPT !.freed = TRUE]
Deliver ==
    /\ Len(s.queue) > 0
    /\ LET e == Head(s.queue) IN
       s' = [s EXCEPT !.queue = Tail(@),
             !.closedNotice = @ \/ e.kind = "close",
             !.tx = IF e.kind = "source" THEN
                  [@ EXCEPT ![e.frame].released = TRUE] ELSE @]

Next == (\E f \in Frames : Admit(f) \/ Refuse(f) \/ CopyDone(f)
         \/ (\E good \in BOOLEAN : Validate(f, good)) \/ Cancel(f)
         \/ PublishSource(f) \/ Reuse(f) \/ Handoff(f) \/ PublishPrepared(f) \/ Terminal(f)
         \/ DiscardOld(f) \/ (\E h \in Heads : Prepare(f, h) \/ Submit(f, h)))
        \/ (\E h \in Heads : Flip(h) \/ DrainHead(h) \/ RetireBlank(h))
        \/ ChangeFacts \/ Close \/ CloseNotice \/ Reopen \/ Revoke \/ Deliver \/ FreePredecessor
Spec == Init /\ [][Next]_vars

TypeOK == /\ s.epoch \in 1..2 /\ s.facts \in 1..2
          /\ s.open \in BOOLEAN /\ s.credits \in Nat
          /\ s.flight \in [Heads -> (Frames \cup {0})]
          /\ s.visible \in [Heads -> (Frames \cup {-1, 0})]
WorkerSafe == Cardinality({f \in Frames : s.tx[f].reading}) <= WorkerLimit
PhysicalSettlement == \A f \in Frames : s.tx[f].terminal # "none" => NoReader(f)
ReopenSafe == ~s.unsafeReopen
StreamBound == s.open => s.facts = s.streamFacts
BudgetSafe == Used <= Capacity
CreditSafe == s.open => s.credits + Cardinality(Held) = CreditLimit
SourceSafe == \A f \in Frames : s.tx[f].released => NoReader(f)
SceneSafe == ~s.unsafeSubmit
AuthoritySafe == ~s.unsafeEpoch
JournalSafe == \A i \in 1..Len(s.queue) : s.queue[i].epoch = s.epoch
RetirementSafe == s.freed => s.retired /\ (\A h \in Heads : s.visible[h] # 0)
PreparedOrder == \A f \in Frames :
    s.tx[f].terminal = "presented" => s.tx[f].handed /\ s.tx[f].preparedQueued
TerminalSafe == \A f \in Frames : s.tx[f].terminal = "presented" => 1 \in s.tx[f].flipped
FairSpec == Spec
 /\ (\A f \in Frames : WF_vars(CopyDone(f))
      /\ WF_vars(\E good \in BOOLEAN : Validate(f, good))
      /\ WF_vars(Handoff(f)) /\ WF_vars(PublishSource(f)) /\ WF_vars(PublishPrepared(f))
      /\ WF_vars(Terminal(f))
      /\ (\A h \in Heads : WF_vars(Prepare(f, h)) /\ WF_vars(Submit(f, h))))
 /\ (\A h \in Heads : WF_vars(Flip(h))) /\ WF_vars(Deliver)
 /\ WF_vars(CloseNotice)
Settles == \A f \in Frames : Live(f) ~> (s.tx[f].terminal # "none" /\ NoReader(f))
ResponsesDrain == \A f \in Frames : Live(f) ~> (s.tx[f].owed = {})
=============================================================================
