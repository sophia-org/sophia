------------------------------ MODULE Wake ------------------------------
\* Proposed clear/check/wait ordering. Publish-before-notify is a separate
\* action; progress assumes the publisher completes notification or exit wakes.
EXTENDS Naturals
CONSTANT ClearFirst
VARIABLE pending, signal, pc, sent, done, notified
vars == <<pending, signal, pc, sent, done, notified>>
Init == /\ pending = FALSE /\ signal = FALSE /\ pc = "start"
        /\ sent = FALSE /\ done = FALSE /\ notified = FALSE
Publish == /\ ~sent /\ pending' = TRUE /\ sent' = TRUE
           /\ UNCHANGED <<signal, pc, done, notified>>
Notify == /\ sent /\ ~notified /\ pending /\ signal' = TRUE /\ notified' = TRUE
          /\ UNCHANGED <<pending, pc, sent, done>>
Clear == /\ pc = "start" /\ ClearFirst /\ signal' = FALSE
         /\ pc' = "check" /\ UNCHANGED <<pending, sent, done, notified>>
Check == /\ pc = IF ClearFirst THEN "check" ELSE "start"
         /\ IF pending THEN /\ done' = TRUE /\ pending' = FALSE /\ pc' = "done"
            ELSE /\ pc' = IF ClearFirst THEN "sleep" ELSE "late_clear"
                 /\ UNCHANGED <<done, pending>>
         /\ UNCHANGED <<signal, sent, notified>>
LateClear == /\ pc = "late_clear" /\ signal' = FALSE /\ pc' = "sleep"
             /\ UNCHANGED <<pending, sent, done, notified>>
Wake == /\ pc = "sleep" /\ signal /\ pc' = "start"
        /\ UNCHANGED <<pending, signal, sent, done, notified>>
Next == Publish \/ Notify \/ Clear \/ Check \/ LateClear \/ Wake
Spec == Init /\ [][Next]_vars /\ WF_vars(Notify) /\ WF_vars(Clear)
        /\ WF_vars(Check) /\ WF_vars(LateClear) /\ WF_vars(Wake)
Progress == sent ~> done
=============================================================================
