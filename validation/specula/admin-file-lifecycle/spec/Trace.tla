---------------------------- MODULE Trace ----------------------------
EXTENDS base, TraceData, Sequences
VARIABLE l
traceVars == <<vars, l>>
IOEnv == [default |-> TRUE]
JsonFile == IF "JSON" \in DOMAIN IOEnv THEN IOEnv.JSON ELSE "../traces/admin.ndjson"
\* TraceData is a design fixture. JSON ingestion awaits runtime instrumentation.
TraceInit == Init /\ l = 1
Step(name) ==
    CASE name = "Discover" -> Discover
      [] name = "Submit" -> Submit(1)
      [] name = "RequestCheck" -> RequestCheck(<<1,1>>)
      [] name = "Approve" -> Approve(<<1,1>>)
      [] name = "Dispatch" -> Dispatch(<<1,1>>)
      [] name = "Expire" -> ExpireDispatched(<<1,1>>)
      [] name = "Publish" -> Publish(<<1,1>>)
      [] name = "Read" -> ReadStatus
      [] name = "BeginComplete" -> BeginComplete(<<1,1>>)
      [] name = "Settle" -> Settle(<<1,1>>)
      [] name = "Reclaim" -> Reclaim(<<1,1>>)
ValidatePostState(e) ==
    /\ connection' = 1 /\ live' /\ catalog' = 1 /\ discovered' = 1
    /\ last' = IF l = 1 THEN 0 ELSE 1
    /\ accepted'[<<1,1>>] = IF l = 1 THEN 0 ELSE 1
    /\ phase'[<<1,1>>] = e.phase /\ result'[<<1,1>>] = e.result
    /\ settled'[<<1,1>>] = e.settled /\ Cardinality(active') = e.active
    /\ pending' = e.pending /\ visible'.outcome = e.shown /\ visible'.request = last'
    /\ effects'[<<1,1>>] = e.effects /\ ending'[<<1,1>>] = e.ending
    /\ pin'.outcome = e.pinned /\ ~badDispatch'
TraceNext == /\ l <= Len(TraceLog) /\ Step(TraceLog[l].action)
             /\ ValidatePostState(TraceLog[l]) /\ l' = l + 1
TraceSpec == TraceInit /\ [][TraceNext]_traceVars /\ WF_traceVars(TraceNext)
TraceMatched == <>(l > Len(TraceLog))
=============================================================================
