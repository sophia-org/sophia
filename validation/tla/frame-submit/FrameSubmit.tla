------------------------- MODULE FrameSubmit -------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS Combined, CheckBytes, RequireWriter, RequireAck
VARIABLES live, writer, staged, accepted, history, acked, lastReply, known, endedKnown
vars == <<live, writer, staged, accepted, history, acked, lastReply, known, endedKnown>>
Empty == [id |-> 0, bytes |-> 0]
Records == [id : 1..2, bytes : 1..2]
Init == /\ live = TRUE /\ writer = TRUE
        /\ staged = Empty /\ accepted = Empty /\ history = {}
        /\ acked = 0 /\ lastReply = Empty /\ known = {} /\ endedKnown = {}
MaxId == IF history = {} THEN 0 ELSE CHOOSE n \in 1..2 :
           n \in {r.id : r \in history} /\ \A r \in history : r.id <= n
\* Stage is the old transport's record write; it creates no custody.
Stage(r) == /\ live /\ writer /\ accepted = Empty
            /\ staged = Empty /\ r.id > MaxId
            /\ staged' = r /\ UNCHANGED <<live, writer, accepted, history, acked, lastReply, known, endedKnown>>
\* Backpressure retains the same complete bytes, rather than restarting custody.
Backpressure(r) == /\ Combined /\ live /\ writer /\ accepted = Empty
                   /\ staged = Empty /\ r.id > MaxId
                   /\ staged' = r /\ UNCHANGED <<live, writer, accepted, history, acked, lastReply, known, endedKnown>>
Accept(r) == /\ live /\ (writer \/ ~RequireWriter)
             /\ accepted = Empty /\ r.id > MaxId
             /\ IF Combined THEN staged \in {Empty, r} ELSE staged = r
             /\ accepted' = r /\ staged' = r /\ history' = history \cup {r}
             /\ lastReply' = r
             /\ UNCHANGED <<live, writer, acked, known, endedKnown>>
Retry(r) == /\ live /\ (writer \/ ~RequireWriter) /\ accepted # Empty
            /\ r.id = accepted.id /\ (~CheckBytes \/ r.bytes = accepted.bytes)
            /\ lastReply' = r
            /\ UNCHANGED <<live, writer, staged, accepted, history, acked, known, endedKnown>>
\* Either the full Rwrite or Submitted establishes known file custody.
Observe == /\ live /\ accepted # Empty
           /\ known' = known \cup {accepted}
           /\ UNCHANGED <<live, writer, staged, accepted, history, acked, lastReply, endedKnown>>
Ack == /\ live /\ accepted # Empty /\ accepted \in known
       /\ acked' = accepted.id
       /\ UNCHANGED <<live, writer, staged, accepted, history, lastReply, known, endedKnown>>
Recycle == /\ live /\ accepted # Empty /\ (acked >= accepted.id \/ ~RequireAck)
           /\ accepted' = Empty /\ staged' = Empty /\ lastReply' = Empty
           /\ UNCHANGED <<live, writer, history, acked, known, endedKnown>>
\* Clunk/assembly expiry never cancel accepted custody. Reopening requires ACK.
Clunk == /\ live /\ writer /\ writer' = FALSE
         /\ staged' = IF accepted = Empty THEN Empty ELSE staged
         /\ lastReply' = Empty
         /\ UNCHANGED <<live, accepted, history, acked, known, endedKnown>>
Open == /\ live /\ ~writer /\ accepted = Empty
        /\ writer' = TRUE /\ UNCHANGED <<live, staged, accepted, history, acked, lastReply, known, endedKnown>>
Disconnect == /\ live /\ live' = FALSE /\ writer' = FALSE
              /\ endedKnown' = known /\ lastReply' = Empty
              /\ UNCHANGED <<staged, accepted, history, acked, known>>
Next == (\E r \in Records : Stage(r) \/ Backpressure(r) \/ Accept(r) \/ Retry(r))
        \/ Observe \/ Ack \/ Recycle \/ Clunk \/ Open \/ Disconnect
Spec == Init /\ [][Next]_vars
ExactCustody == lastReply = Empty \/ lastReply \in history
LiveWriterOnSuccess == lastReply = Empty \/ (live /\ writer)
OneAcceptancePerId == Cardinality(history) = Cardinality({r.id : r \in history})
AckBeforeReuse == \A r \in history : r.id <= acked \/ r = accepted
KnownNeverInvented == known \subseteq history /\ (~live => known = endedKnown)
NoPrematureCustody == accepted = Empty \/ accepted \in history
=============================================================================
