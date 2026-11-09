----------------------------- MODULE QueueIndex -----------------------------
\* Proposed pread-first CQ reservation arithmetic. Local tail/head validated
\* monotonically; remote head is hostile. No semantic ACK is derived from it.
EXTENDS Naturals, TLC
CONSTANT CheckHead
VARIABLE head, tail, observed, closed
vars == <<head, tail, observed, closed>>
Init == /\ head = 0 /\ tail = 0 /\ observed = 0 /\ closed = FALSE
PeerWord == /\ ~closed /\ \E h \in 0..5 : observed' = h
            /\ UNCHANGED <<head, tail, closed>>
Valid == observed >= head /\ observed <= tail
Check == /\ ~closed
         /\ IF Valid \/ ~CheckHead THEN /\ head' = observed /\ UNCHANGED closed
            ELSE /\ closed' = TRUE /\ UNCHANGED head
         /\ UNCHANGED <<tail, observed>>
\* Capacity is two entries; the finite projection stops before integer wrap.
Publish == /\ ~closed /\ tail < 4 /\ tail - head < 2
           /\ tail' = tail + 1 /\ UNCHANGED <<head, observed, closed>>
Next == PeerWord \/ Check \/ Publish
Spec == Init /\ [][Next]_vars
Bounded == head <= tail /\ tail - head <= 2
=============================================================================
