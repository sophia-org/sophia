------------------------------ MODULE ExitWake ------------------------------
\* Producer stops before notification. Its pidfd becomes readable; owner can
\* close even when no doorbell is readable. No wall-time bound is proved here.
EXTENDS Naturals
CONSTANT WatchExit
VARIABLE pending, dead, closed
vars == <<pending, dead, closed>>
Init == /\ pending = FALSE /\ dead = FALSE /\ closed = FALSE
Publish == /\ ~dead /\ pending' = TRUE /\ UNCHANGED <<dead, closed>>
Exit == /\ ~dead /\ dead' = TRUE /\ UNCHANGED <<pending, closed>>
Close == /\ WatchExit /\ dead /\ ~closed /\ closed' = TRUE
         /\ pending' = FALSE /\ UNCHANGED dead
Next == Publish \/ Exit \/ Close
Spec == Init /\ [][Next]_vars /\ WF_vars(Close)
ExitProgress == dead ~> closed
=============================================================================
