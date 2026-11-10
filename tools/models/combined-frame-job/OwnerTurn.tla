----------------------------- MODULE OwnerTurn -----------------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC
CONSTANT Mutation
VARIABLE s
vars == <<s>>
Slots == 0..2
Free == Slots \ (s.otherHeld \cup (IF s.specHeld THEN {0} ELSE {}))

Init == \E q \in {<<>>, <<"noise">>, <<"noise", "noise">>}:
  s = [phase |-> "result", credit |-> TRUE, lease |-> "result",
       q |-> q, specHeld |-> TRUE, otherHeld |-> {1, 2}, ordinarySlot |-> 3,
       render |-> "absent", attempts |-> 0, entries |-> 0,
       returns |-> 0, collections |-> 0, laterCycles |-> 0,
       releaseFull |-> FALSE, renderFull |-> FALSE, refused |-> FALSE,
       frame |-> 1, supersessions |-> 0, ordinaryDiscard |-> "occupied"]

Consume == /\ s.phase = "result"
           /\ s' = [s EXCEPT !.phase = "consumed", !.lease = "row"]
Settle == /\ s.phase = "consumed"
          /\ s' = [s EXCEPT !.phase = "settled"]
Retain == /\ s.phase = "settled" /\ s.lease = "row"
          /\ s' = [s EXCEPT !.lease = "cell"]
ClearCredit == /\ s.phase = "settled" /\ s.credit
               /\ s.lease \in {"cell", "channel", "disposed"}
               /\ s' = [s EXCEPT !.credit = FALSE]
DeclineFast == /\ s.phase = "settled" /\ ~s.credit
               /\ s' = [s EXCEPT !.phase = "latched"]
OrdinaryEntry == /\ s.phase = "latched"
                 /\ s' = [s EXCEPT !.phase = "ordinary", !.entries = @ + 1,
                                   !.render = "ready"]

FlushRelease == /\ s.lease = "cell"
                /\ IF Len(s.q) = 2
                      THEN s' = [s EXCEPT !.releaseFull = TRUE,
                          !.lease = IF Mutation = "drop" THEN "lost" ELSE @]
                      ELSE s' = [s EXCEPT !.q = Append(@, "release"),
                                           !.lease = "channel"]
TryRender == /\ s.phase = "ordinary" /\ s.render = "ready"
             /\ s.attempts < 2
             /\ IF Len(s.q) = 2
                   THEN s' = [s EXCEPT !.renderFull = TRUE]
                   ELSE s' = [s EXCEPT !.q = Append(@, "render"),
                                        !.render = "queued", !.attempts = @ + 1]
DrainNoise == /\ Len(s.q) > 0 /\ Head(s.q) = "noise"
              /\ s' = [s EXCEPT !.q = Tail(@)]
ProcessRelease == /\ Len(s.q) > 0 /\ Head(s.q) = "release"
                  /\ s' = [s EXCEPT !.q = Tail(@), !.specHeld = FALSE,
                                       !.lease = "disposed"]
\* Actual slot acquisition, not early Render enqueue, is the safety boundary.
ProcessRender == /\ Len(s.q) > 0 /\ Head(s.q) = "render"
                 /\ IF Free # {} \/ Mutation = "reuse"
                       THEN \E slot \in (IF Free # {} THEN Free ELSE {0}):
                         s' = [s EXCEPT !.q = Tail(@), !.ordinarySlot = slot,
                                            !.render = "done"]
                       ELSE s' = [s EXCEPT !.q = Tail(@),
                           !.render = "deferred_reply", !.returns = @ + 1]
\* Deferred is a worker RESULT; it cannot retry inside the worker.
CollectDeferred == /\ s.render = "deferred_reply"
                   /\ s' = [s EXCEPT !.render = "pending",
                                        !.collections = @ + 1]
LaterOwnerExport == /\ s.render = "pending"
                    /\ s' = [s EXCEPT !.render = "ready", !.laterCycles = @ + 1]
\* A newer ordinary frame can supersede returned pending_frame; no new fallback.
Supersede == /\ s.render = "pending" /\ s.frame = 1
             /\ s' = [s EXCEPT !.frame = 2, !.supersessions = 1]
\* External last-use proof for the OTHER slots is assumed, never inferred here.
PriorLastUse(k) == /\ k \in s.otherHeld
                  /\ s' = [s EXCEPT !.otherHeld = @ \ {k}]

Next == Consume \/ Settle \/ Retain \/ ClearCredit \/ DeclineFast \/ OrdinaryEntry
        \/ FlushRelease \/ TryRender \/ DrainNoise \/ ProcessRelease \/ ProcessRender
        \/ CollectDeferred \/ LaterOwnerExport \/ Supersede
        \/ (\E k \in {1, 2}: PriorLastUse(k))
Spec == Init /\ [][Next]_vars
TypeOK == /\ s.phase \in {"result", "consumed", "settled", "latched", "ordinary"}
          /\ s.credit \in BOOLEAN /\ s.specHeld \in BOOLEAN
          /\ s.lease \in {"result", "row", "cell", "channel", "disposed", "lost"}
          /\ Len(s.q) <= 2 /\ \A i \in DOMAIN s.q: s.q[i] \in {"noise", "release", "render"}
          /\ s.otherHeld \subseteq {1, 2} /\ s.ordinarySlot \in 0..3
          /\ s.render \in {"absent", "ready", "queued", "deferred_reply", "pending", "done"}
          /\ s.attempts \in 0..2 /\ s.entries \in 0..1
          /\ s.returns \in 0..2 /\ s.collections \in 0..2 /\ s.laterCycles \in 0..2
          /\ s.frame \in 1..2 /\ s.supersessions \in 0..1
CreditProtected == ~s.credit => s.lease \in {"cell", "channel", "disposed"}
LeaseHeld == s.lease # "lost"
FullIsDeferred == ~s.refused
OneEntry == s.entries <= 1
NoOccupiedSlotReuse == s.ordinarySlot # 3 =>
                       /\ s.ordinarySlot \notin s.otherHeld
                       /\ ~(s.specHeld /\ s.ordinarySlot = 0)
NoPrematureCandidate == s.phase # "ordinary" => s.entries = 0 /\ s.render = "absent"
DeferredReturnsToOwner == /\ s.collections <= s.returns
                          /\ s.laterCycles <= s.collections
                          /\ s.attempts <= 1 + s.laterCycles
SeparateRetention == s.ordinaryDiscard = "occupied"
\* Check queue ownership, not an acknowledgement from the worker.
ReleaseInChannel == (s.lease = "channel") <=>
                    (\E i \in DOMAIN s.q: s.q[i] = "release")
\* Deliberately false reachability queries, never asserted as safety claims.
NoBothFull == ~(s.releaseFull /\ s.renderFull)
NoSupersededRetry == ~(s.returns > 0 /\ s.laterCycles > 0 /\ s.attempts = 2
                       /\ s.supersessions = 1)
=============================================================================
