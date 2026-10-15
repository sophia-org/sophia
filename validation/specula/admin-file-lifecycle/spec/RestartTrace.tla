---------------------------- MODULE RestartTrace ----------------------------
EXTENDS Restart, Sequences
CONSTANT BadSettlement
VARIABLE cursor
traceVars == <<vars, cursor>>
\* Design fixture, not runtime instrumentation. Columns:
\* action, stage1, connected1, phase1, result1, settled1, finishing1,
\* expired1, phase2, result2, settled2, finishing2, shown.
Events == <<
 <<"submit1", "running", TRUE, "queued", "none", FALSE, FALSE, FALSE, "unused", "none", FALSE, FALSE, 0>>,
 <<"check1", "running", TRUE, "checking", "none", FALSE, FALSE, FALSE, "unused", "none", FALSE, FALSE, 0>>,
 <<"approve1", "running", TRUE, "checked", "none", FALSE, FALSE, FALSE, "unused", "none", FALSE, FALSE, 0>>,
 <<"claim1", "running", TRUE, "dispatched", "none", FALSE, FALSE, FALSE, "unused", "none", FALSE, FALSE, 0>>,
 <<"stop1", "stopping", TRUE, "dispatched", "none", FALSE, FALSE, FALSE, "unused", "none", FALSE, FALSE, 0>>,
 <<"close1", "stopping", FALSE, "dispatched", "none", FALSE, FALSE, FALSE, "unused", "none", FALSE, FALSE, 0>>,
 <<"expire1", "stopping", FALSE, "dispatched", "indeterminate", BadSettlement, FALSE, TRUE, "unused", "none", FALSE, FALSE, 0>>,
 <<"join1", "joined", FALSE, "dispatched", "indeterminate", FALSE, FALSE, TRUE, "unused", "none", FALSE, FALSE, 0>>,
 <<"bind", "joined", FALSE, "dispatched", "indeterminate", FALSE, FALSE, TRUE, "unused", "none", FALSE, FALSE, 0>>,
 <<"submit2", "joined", FALSE, "dispatched", "indeterminate", FALSE, FALSE, TRUE, "queued", "none", FALSE, FALSE, 0>>,
 <<"finish1", "joined", FALSE, "dispatched", "indeterminate", FALSE, TRUE, TRUE, "queued", "none", FALSE, FALSE, 0>>,
 <<"settle1", "joined", FALSE, "dispatched", "indeterminate", TRUE, TRUE, TRUE, "queued", "none", FALSE, FALSE, 0>>,
 <<"check2", "joined", FALSE, "dispatched", "indeterminate", TRUE, TRUE, TRUE, "checking", "none", FALSE, FALSE, 0>>,
 <<"approve2", "joined", FALSE, "dispatched", "indeterminate", TRUE, TRUE, TRUE, "checked", "none", FALSE, FALSE, 0>>,
 <<"claim2", "joined", FALSE, "dispatched", "indeterminate", TRUE, TRUE, TRUE, "dispatched", "none", FALSE, FALSE, 0>>,
 <<"finish2", "joined", FALSE, "dispatched", "indeterminate", TRUE, TRUE, TRUE, "dispatched", "completed", FALSE, TRUE, 0>>,
 <<"publish2", "joined", FALSE, "dispatched", "indeterminate", TRUE, TRUE, TRUE, "dispatched", "completed", FALSE, TRUE, 2>>,
 <<"settle2", "joined", FALSE, "dispatched", "indeterminate", TRUE, TRUE, TRUE, "dispatched", "completed", TRUE, TRUE, 2>>
>>
Step(action) == CASE action = "submit1" -> Submit(1)
 [] action = "check1" -> RequestCheck(1) [] action = "approve1" -> Approve(1)
 [] action = "claim1" -> Claim(1) [] action = "stop1" -> Stop(1)
 [] action = "close1" -> Close(1) [] action = "expire1" -> Expire(1)
 [] action = "join1" -> Join(1) [] action = "bind" -> Bind
 [] action = "submit2" -> Submit(2) [] action = "finish1" -> FinishResult(1)
 [] action = "settle1" -> FinishSettled(1) [] action = "check2" -> RequestCheck(2)
 [] action = "approve2" -> Approve(2) [] action = "claim2" -> Claim(2)
 [] action = "finish2" -> FinishResult(2) [] action = "publish2" -> Publish(2)
 [] action = "settle2" -> FinishSettled(2)
ValidatePostState(e) ==
 /\ current' = IF cursor < 9 THEN 1 ELSE 2
 /\ stage' = [s \in Instances |-> IF s = 1 THEN e[2] ELSE IF cursor < 9 THEN "absent" ELSE "running"]
 /\ connected' = [s \in Instances |-> IF s = 1 THEN e[3] ELSE cursor >= 9]
 /\ accepted' = [s \in Instances |-> IF s = 1 THEN TRUE ELSE cursor >= 10]
 /\ phase' = [s \in Instances |-> IF s = 1 THEN e[4] ELSE e[9]]
 /\ result' = [s \in Instances |-> IF s = 1 THEN e[5] ELSE e[10]]
 /\ settled' = [s \in Instances |-> IF s = 1 THEN e[6] ELSE e[11]]
 /\ finishing' = [s \in Instances |-> IF s = 1 THEN e[7] ELSE e[12]]
 /\ expired' = [s \in Instances |-> IF s = 1 THEN e[8] ELSE FALSE]
 /\ effects' = [s \in Instances |-> IF (s = 1 /\ cursor >= 4) \/ (s = 2 /\ cursor >= 15) THEN 1 ELSE 0]
 /\ shown' = e[13] /\ stoppedQueued' = {}
TraceInit == Init /\ cursor = 1
TraceNext == /\ cursor <= Len(Events) /\ Step(Events[cursor][1])
             /\ ValidatePostState(Events[cursor]) /\ cursor' = cursor + 1
TraceSpec == TraceInit /\ [][TraceNext]_traceVars /\ WF_traceVars(TraceNext)
TraceMatched == <>(cursor > Len(Events))
=============================================================================
