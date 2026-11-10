-------------------------- MODULE SourceRelease --------------------------
EXTENDS Naturals, FiniteSets
CONSTANT Mutation
VARIABLE s
vars == <<s>>
\* Order/timeout witness, not measured timing or the full journal protocol.
Period == 16667
PeerWait == 5000000
ParkBound == 100000
Times == {0, Period, ParkBound, PeerWait - 1, PeerWait}
Init == /\ s \in [phase : {"idle"}, generation : {0}, pending : {{}}, age : {0}, room : BOOLEAN, finished : {FALSE}, journaled : {FALSE}, confirmed : {FALSE}, observed : {FALSE}, presented : {FALSE}, consumed : {FALSE}, badRelease : {FALSE}, timedOut : {FALSE}]
Slot == s.generation % 2
PeerReady == s.phase = "idle" \/ (s.presented /\ s.observed)
Begin == /\ PeerReady /\ ~s.timedOut /\ s.generation < 2
         /\ (s.generation + 1) % 2 \notin s.pending
         /\ s' = [s EXCEPT !.generation = @ + 1, !.phase = "validation", !.age = 0, !.pending = @ \cup {(s.generation + 1) % 2}, !.finished = FALSE, !.journaled = FALSE, !.confirmed = FALSE, !.observed = FALSE, !.presented = FALSE, !.consumed = FALSE]
Validate == /\ s.phase = "validation" /\ s.age < PeerWait
            /\ s' = [s EXCEPT !.phase = "parked", !.finished = TRUE]
StartRender == /\ s.phase = "parked" /\ s.age < ParkBound
               /\ s' = [s EXCEPT !.phase = "render"]
FinishRender == /\ s.phase = "render" /\ s.age < PeerWait
                /\ s' = [s EXCEPT !.phase = "result"]
Consume == /\ s.phase = "result" /\ s.age < PeerWait
           /\ s' = [s EXCEPT !.phase = "consumed", !.consumed = TRUE]
Publish == /\ s.consumed /\ s.finished /\ ~s.journaled /\ s.room
           /\ s' = [s EXCEPT !.journaled = TRUE, !.room = FALSE]
Confirm == /\ s.generation > 0 /\ ~s.confirmed
           /\ ((s.finished /\ s.journaled) \/ Mutation = "early")
           /\ s' = [s EXCEPT !.confirmed = TRUE, !.pending = @ \ {Slot}, !.badRelease = ~(s.finished /\ s.journaled)]
ObserveRelease == /\ s.confirmed /\ s.journaled /\ ~s.observed
                  /\ s' = [s EXCEPT !.observed = TRUE]
Present == /\ s.consumed /\ ~s.presented /\ s.age >= Period
           /\ s' = [s EXCEPT !.presented = TRUE]
\* Abstract prior-record ACK making publication room; no automatic fair ACK.
AckRoom == /\ ~s.room /\ s' = [s EXCEPT !.room = TRUE]
Tick == /\ s.generation > 0 /\ ~s.timedOut
        /\ \E next \in Times : next > s.age /\ s' = [s EXCEPT !.age = next]
ParkExpiry == /\ s.phase = "parked" /\ s.age >= ParkBound
              /\ s' = [s EXCEPT !.phase = "expired"]
Timeout == /\ s.age = PeerWait /\ ~PeerReady /\ ~s.timedOut
           /\ s' = [s EXCEPT !.timedOut = TRUE]
Next == Begin \/ Validate \/ StartRender \/ FinishRender \/ Consume \/ Publish \/ Confirm \/ ObserveRelease \/ Present \/ AckRoom \/ Tick \/ ParkExpiry \/ Timeout
Spec == Init /\ [][Next]_vars
TypeOK == /\ s.phase \in {"idle", "validation", "parked", "render", "result", "consumed", "expired"}
          /\ s.generation \in 0..2 /\ s.pending \subseteq {0, 1} /\ s.age \in Times
          /\ \A key \in {"room", "finished", "journaled", "confirmed", "observed", "presented", "consumed", "badRelease", "timedOut"}: s[key] \in BOOLEAN
ReleaseProof == ~s.badRelease
HeldOfferSafe == PeerReady => ((s.generation + 1) % 2 \notin s.pending)
JournalTimeoutAbsent == ~(s.timedOut /\ s.presented /\ s.consumed /\ ~s.journaled /\ ~s.room)
=============================================================================
