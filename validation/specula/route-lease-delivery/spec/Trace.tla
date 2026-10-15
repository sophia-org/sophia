----------------------------- MODULE Trace -----------------------------
EXTENDS base
VARIABLE cursor
traceVars == <<vars, cursor>>
TraceInit == Init /\ cursor = 1
\* Design fixture only: no production trace is claimed.
Step == CASE cursor = 1 -> Reserve(1)
          [] cursor = 2 -> Effect(1)
          [] cursor = 3 -> Publish(1)
          [] cursor = 4 -> Transfer
          [] cursor = 5 -> Reserve(2)
          [] cursor = 6 -> Effect(2)
          [] cursor = 7 -> Publish(2)
          [] cursor = 8 -> Consume
          [] cursor = 9 -> Transfer
          [] cursor = 10 -> Advance
          [] cursor = 11 -> Consume
Post == CASE cursor = 1 -> phase'[1] = "reserved" /\ Reserved' = {1}
          [] cursor = 2 -> effect' = {1} /\ phase'[1] = "effect"
          [] cursor = 3 -> pending' = <<1>> /\ Published' = {1}
          [] cursor = 4 -> pending' = <<>> /\ wire' = <<1>>
          [] cursor = 5 -> Reserved' = {2} /\ wire' = <<1>>
          [] cursor = 6 -> effect' = {1, 2} /\ phase'[2] = "effect"
          [] cursor = 7 -> pending' = <<2>> /\ wire' = <<1>>
          [] cursor = 8 -> consumed' = {1} /\ wire' = <<>> /\ granted' = {<<1,1>>}
          [] cursor = 9 -> pending' = <<>> /\ wire' = <<2>>
          [] cursor = 10 -> epoch' = 2 /\ granted' = {}
          [] cursor = 11 -> consumed' = {1,2} /\ wire' = <<>> /\ granted' = {}
TraceNext == /\ cursor <= 11 /\ Step /\ Post /\ cursor' = cursor + 1
TraceSpec == TraceInit /\ [][TraceNext]_traceVars /\ WF_traceVars(TraceNext)
Matched == <> (cursor = 12)
=============================================================================
