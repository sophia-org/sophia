---------------------------- MODULE Restart ----------------------------
EXTENDS Naturals, FiniteSets
CONSTANTS SkipCancel, CrossInstance
Instances == 1..2
PreDispatch == {"queued", "checking", "checked"}
VARIABLES current, stage, connected, accepted, phase, result, settled,
          finishing, expired, effects, shown, stoppedQueued
vars == <<current, stage, connected, accepted, phase, result, settled,
          finishing, expired, effects, shown, stoppedQueued>>

Init ==
    /\ current = 1
    /\ stage = [s \in Instances |-> IF s = 1 THEN "running" ELSE "absent"]
    /\ connected = [s \in Instances |-> s = 1]
    /\ accepted = [s \in Instances |-> FALSE]
    /\ phase = [s \in Instances |-> "unused"]
    /\ result = [s \in Instances |-> "none"]
    /\ settled = [s \in Instances |-> FALSE]
    /\ finishing = [s \in Instances |-> FALSE]
    /\ expired = [s \in Instances |-> FALSE]
    /\ effects = [s \in Instances |-> 0]
    /\ shown = 0 /\ stoppedQueued = {}

\* files/export.rs:104-173: each service has its own queue and ticket Arc.
Submit(s) ==
    /\ s = current /\ connected[s] /\ ~accepted[s]
    /\ accepted' = [accepted EXCEPT ![s] = TRUE]
    /\ phase' = [phase EXCEPT ![s] = "queued"]
    /\ UNCHANGED <<current, stage, connected, result, settled, finishing,
                    expired, effects, shown, stoppedQueued>>
\* control.rs:44-58; request, worker recheck and owner claim are distinct CASes.
RequestCheck(s) ==
    /\ phase[s] = "queued"
    /\ phase' = [phase EXCEPT ![s] = "checking"]
    /\ UNCHANGED <<current, stage, connected, accepted, result, settled,
                    finishing, expired, effects, shown, stoppedQueued>>
\* files/export.rs:67-82 executes before the stop loop exits.
Approve(s) ==
    /\ connected[s] /\ phase[s] = "checking"
    /\ phase' = [phase EXCEPT ![s] = "checked"]
    /\ UNCHANGED <<current, stage, connected, accepted, result, settled,
                    finishing, expired, effects, shown, stoppedQueued>>
\* control.rs:54-58; a successful claim may race the worker's close.
Claim(s) ==
    /\ phase[s] = "checked"
    /\ phase' = [phase EXCEPT ![s] = "dispatched"]
    /\ effects' = [effects EXCEPT ![s] = @ + 1]
    /\ UNCHANGED <<current, stage, connected, accepted, result, settled,
                    finishing, expired, shown, stoppedQueued>>
\* control.rs:271-274; setting stop is not itself cancellation.
Stop(s) ==
    /\ stage[s] = "running"
    /\ stage' = [stage EXCEPT ![s] = "stopping"]
    /\ UNCHANGED <<current, connected, accepted, phase, result, settled,
                    finishing, expired, effects, shown, stoppedQueued>>
\* files.rs:166-168 -> files/export.rs:404-409 -> control.rs:89-103.
Close(s) ==
    /\ stage[s] = "stopping" /\ connected[s]
    /\ connected' = [connected EXCEPT ![s] = FALSE]
    /\ IF phase[s] \in PreDispatch
       THEN /\ stoppedQueued' = stoppedQueued \cup {s}
            /\ phase' = IF SkipCancel THEN phase ELSE [phase EXCEPT ![s] = "cancelled"]
       ELSE /\ UNCHANGED <<phase, stoppedQueued>>
    /\ UNCHANGED <<current, stage, accepted, result, settled,
                    finishing, expired, effects, shown>>
\* disconnect's release store follows cancellation, control.rs:91-92.
SettleCancelled(s) ==
    /\ phase[s] = "cancelled" /\ ~settled[s]
    /\ settled' = [settled EXCEPT ![s] = TRUE]
    /\ UNCHANGED <<current, stage, connected, accepted, phase, result,
                    finishing, expired, effects, shown, stoppedQueued>>
\* files.rs:169-170 expires retained tickets after connection close finishes.
Expire(s) ==
    /\ stage[s] = "stopping" /\ ~connected[s] /\ ~expired[s]
    /\ (phase[s] # "cancelled" \/ settled[s])
    /\ expired' = [expired EXCEPT ![s] = TRUE]
    /\ result' = IF accepted[s] /\ result[s] = "none"
                  THEN [result EXCEPT ![s] = "indeterminate"] ELSE result
    /\ UNCHANGED <<current, stage, connected, accepted, phase, settled,
                    finishing, effects, shown, stoppedQueued>>
\* control.rs:274; join observes the worker's completed teardown.
Join(s) ==
    /\ stage[s] = "stopping" /\ expired[s]
    /\ stage' = [stage EXCEPT ![s] = "joined"]
    /\ UNCHANGED <<current, connected, accepted, phase, result, settled,
                    finishing, expired, effects, shown, stoppedQueued>>
\* control.rs:127-232; bind creates a fresh identity, channel and worker.
Bind ==
    /\ current = 1 /\ stage[1] = "joined"
    /\ current' = 2 /\ stage' = [stage EXCEPT ![2] = "running"]
    /\ connected' = [connected EXCEPT ![2] = TRUE] /\ shown' = 0
    /\ UNCHANGED <<accepted, phase, result, settled, finishing, expired,
                    effects, stoppedQueued>>
\* control.rs:61-72; first outcome and settlement are separate publications.
FinishResult(s) ==
    /\ phase[s] = "dispatched" /\ ~finishing[s]
    /\ finishing' = [finishing EXCEPT ![s] = TRUE]
    /\ result' = IF result[s] = "none" THEN [result EXCEPT ![s] = "completed"] ELSE result
    /\ UNCHANGED <<current, stage, connected, accepted, phase, settled,
                    expired, effects, shown, stoppedQueued>>
FinishSettled(s) ==
    /\ finishing[s] /\ ~settled[s]
    /\ settled' = [settled EXCEPT ![s] = TRUE]
    /\ UNCHANGED <<current, stage, connected, accepted, phase, result,
                    finishing, expired, effects, shown, stoppedQueued>>
\* files/export.rs:70-97 reads only its own pending ticket Arc.
Publish(s) ==
    /\ connected[current] /\ accepted[current] /\ result[s] # "none"
    /\ (s = current \/ CrossInstance)
    /\ shown' = s
    /\ UNCHANGED <<current, stage, connected, accepted, phase, result,
                    settled, finishing, expired, effects, stoppedQueued>>
Next == Bind \/ (\E s \in Instances : Submit(s) \/ RequestCheck(s) \/ Approve(s)
    \/ Claim(s) \/ Stop(s) \/ Close(s) \/ SettleCancelled(s) \/ Expire(s)
    \/ Join(s) \/ FinishResult(s) \/ FinishSettled(s) \/ Publish(s))
TypeOK == /\ current \in Instances /\ shown \in 0..2
          /\ stoppedQueued \subseteq Instances
          /\ effects \in [Instances -> 0..1]
OldTransportClosed == current = 2 => ~connected[1] /\ stage[1] = "joined"
ShutdownCancels == \A s \in stoppedQueued : effects[s] = 0
ResultIsolation == shown = 0 \/ shown = current
=============================================================================
