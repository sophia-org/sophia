---------------------------- MODULE base ----------------------------
EXTENDS Naturals, FiniteSets

CONSTANTS MaxConnection, MaxRequest, Capacity,
          AcceptReplay, ReleaseOnOutcome, ConsumeOnRead
Connections == 1..MaxConnection
Requests == 1..MaxRequest
Tickets == Connections \X Requests
PreDispatch == {"queued", "checking", "checked"}
NoStatus == [request |-> 0, outcome |-> "ready"]
VARIABLES connection, live, catalog, discovered, last, visible, pending,
          phase, result, settled, active, ending, generation,
          accepted, effects, cancelled, checked, badDispatch, pin
publicVars == <<connection, live, catalog, discovered, last, visible, pending, pin>>
ticketVars == <<phase, result, settled, active, ending, generation,
                accepted, effects, cancelled, checked, badDispatch>>
vars == <<publicVars, ticketVars>>

Init ==
    /\ connection = 1 /\ live = TRUE /\ catalog = 1
    /\ discovered = 0 /\ last = 0 /\ visible = NoStatus /\ pending = 0
    /\ pin = NoStatus
    /\ phase = [t \in Tickets |-> "unused"]
    /\ result = [t \in Tickets |-> "none"]
    /\ settled = [t \in Tickets |-> FALSE]
    /\ ending = [t \in Tickets |-> "none"]
    /\ generation = [t \in Tickets |-> 0]
    /\ accepted = [t \in Tickets |-> 0]
    /\ effects = [t \in Tickets |-> 0]
    /\ active = {} /\ cancelled = {} /\ checked = {} /\ badDispatch = FALSE

\* Proposed commands open: docs/sophia-admin-files.md:125-131.
Discover == /\ live /\ discovered' = catalog
            /\ UNCHANGED <<connection, live, catalog, last, visible, pending, pin, ticketVars>>
ReplaceCatalog == /\ catalog = 1 /\ catalog' = 2
                  /\ UNCHANGED <<connection, live, discovered, last, visible, pending, pin, ticketVars>>

\* Proposed ctl acceptance: docs/sophia-admin-files.md:116-123.
\* Capacity and typed admission correspond to control/transport.rs:246-272.
Submit(r) ==
    LET t == <<connection, r>> IN
    /\ live /\ discovered # 0 /\ pending = 0
    /\ (r > last \/ AcceptReplay)
    /\ Cardinality(active) < Capacity
    /\ last' = r /\ accepted' = [accepted EXCEPT ![t] = @ + 1]
    /\ generation' = [generation EXCEPT ![t] = discovered]
    /\ IF discovered = catalog
       THEN /\ phase' = [phase EXCEPT ![t] = "queued"]
            /\ result' = [result EXCEPT ![t] = "none"]
            /\ settled' = [settled EXCEPT ![t] = FALSE]
            /\ active' = active \cup {t} /\ pending' = r
            /\ visible' = [request |-> r, outcome |-> "pending"]
       ELSE /\ phase' = [phase EXCEPT ![t] = "cancelled"]
            /\ result' = [result EXCEPT ![t] = "stale"]
            /\ settled' = [settled EXCEPT ![t] = TRUE]
            /\ visible' = [request |-> r, outcome |-> "stale"]
            /\ UNCHANGED <<active, pending>>
    /\ ending' = [ending EXCEPT ![t] = "none"]
    /\ UNCHANGED <<connection, live, catalog, discovered, pin, effects, cancelled, checked, badDispatch>>

\* ControlTicket::claim first CAS: control.rs:44-53.
RequestCheck(t) ==
    /\ phase[t] = "queued" /\ ending[t] = "none"
    /\ phase' = [phase EXCEPT ![t] = "checking"]
    /\ UNCHANGED <<publicVars, result, settled, active, ending, generation,
                    accepted, effects, cancelled, checked, badDispatch>>
\* Worker approval CAS, not an unconditional store: transport.rs:80-94.
Approve(t) ==
    /\ phase[t] = "checking" /\ ending[t] = "none"
    /\ phase' = [phase EXCEPT ![t] = "checked"] /\ checked' = checked \cup {t}
    /\ UNCHANGED <<publicVars, result, settled, active, ending, generation,
                    accepted, effects, cancelled, badDispatch>>
\* Owner generation check and successful claim: live_session/control.rs:271,318-337.
Dispatch(t) ==
    /\ phase[t] = "checked" /\ ending[t] = "none" /\ generation[t] = catalog
    /\ phase' = [phase EXCEPT ![t] = "dispatched"]
    /\ effects' = [effects EXCEPT ![t] = @ + 1]
    /\ badDispatch' = badDispatch \/ t \notin checked \/ generation[t] # catalog
    /\ UNCHANGED <<publicVars, result, settled, active, ending, generation,
                    accepted, cancelled, checked>>

\* cancel_queued CAS wins before result and settled publication: control.rs:61-104.
BeginCancel(t, why) ==
    /\ phase[t] \in PreDispatch /\ ending[t] = "none"
    /\ (why # "stale" \/ generation[t] # catalog)
    /\ (why # "denied" \/ phase[t] = "checking")
    /\ phase' = [phase EXCEPT ![t] = "cancelled"]
    /\ ending' = [ending EXCEPT ![t] = why] /\ cancelled' = cancelled \cup {t}
    /\ UNCHANGED <<publicVars, result, settled, active, generation,
                    accepted, effects, checked, badDispatch>>
\* finish on an already-dispatched command: control.rs:61-72; no rollback.
BeginComplete(t) ==
    /\ phase[t] = "dispatched" /\ ending[t] = "none" /\ ~settled[t]
    /\ ending' = [ending EXCEPT ![t] = "completed"]
    /\ UNCHANGED <<publicVars, phase, result, settled, active, generation,
                    accepted, effects, cancelled, checked, badDispatch>>
\* First outcome CAS is distinct from settlement store: control.rs:63-69.
WriteFinish(t) ==
    /\ ending[t] \notin {"none", "disconnect"} /\ result[t] = "none"
    /\ result' = [result EXCEPT ![t] = ending[t]]
    /\ UNCHANGED <<publicVars, phase, settled, active, ending, generation,
                    accepted, effects, cancelled, checked, badDispatch>>
\* Timeout after claim cannot cancel execution: control.rs:74-86.
ExpireDispatched(t) ==
    /\ phase[t] = "dispatched" /\ result[t] = "none" /\ ~settled[t]
    /\ result' = [result EXCEPT ![t] = "indeterminate"]
    /\ UNCHANGED <<publicVars, phase, settled, active, ending, generation,
                    accepted, effects, cancelled, checked, badDispatch>>
\* finish's release store; outcome may already be Indeterminate: control.rs:70.
Settle(t) ==
    /\ ending[t] # "none" /\ (result[t] # "none" \/ ending[t] = "disconnect") /\ ~settled[t]
    /\ settled' = [settled EXCEPT ![t] = TRUE]
    /\ UNCHANGED <<publicVars, phase, result, active, ending, generation,
                    accepted, effects, cancelled, checked, badDispatch>>
\* active.retain checks settled, never merely outcome: transport.rs:343.
Reclaim(t) ==
    /\ t \in active /\ (settled[t] \/ (ReleaseOnOutcome /\ result[t] # "none"))
    /\ active' = active \ {t}
    /\ UNCHANGED <<publicVars, phase, result, settled, ending, generation,
                    accepted, effects, cancelled, checked, badDispatch>>
\* Proposed status projection is fenced by connection and pending identity.
\* Counterpart: transport.rs:99-104; file draft:116-123.
Publish(t) ==
    /\ live /\ t = <<connection, pending>> /\ result[t] # "none"
    /\ visible' = [request |-> pending, outcome |-> result[t]] /\ pending' = 0
    /\ UNCHANGED <<connection, live, catalog, discovered, last, pin, ticketVars>>
\* Proposed read/flush/clunk never consume status: file draft:110-111,133-136.
ReadStatus ==
    /\ live /\ pin' = visible
    /\ visible' = IF ConsumeOnRead THEN NoStatus ELSE visible
    /\ UNCHANGED <<connection, live, catalog, discovered, last, pending, ticketVars>>
ClunkOrFlush == /\ pin' = NoStatus /\ UNCHANGED <<connection, live, catalog,
                   discovered, last, visible, pending, ticketVars>>
\* Loss stops projection; queued cancellation remains an enabled worker step.
\* Dispatched work stays charged until finish: control.rs:89-103; transport.rs:280-284.
Disconnect ==
    LET t == <<connection, pending>> IN
    /\ live /\ live' = FALSE
    /\ IF pending # 0 /\ phase[t] \in PreDispatch
       THEN /\ phase' = [phase EXCEPT ![t] = "cancelled"]
            /\ ending' = [ending EXCEPT ![t] = "disconnect"]
            /\ cancelled' = cancelled \cup {t}
       ELSE UNCHANGED <<phase, ending, cancelled>>
    /\ UNCHANGED <<connection, catalog, discovered, last, visible, pending, pin,
                    result, settled, active, generation, accepted, effects, checked, badDispatch>>
Reconnect ==
    /\ ~live /\ connection < MaxConnection
    /\ \A t \in Tickets : t[1] = connection => phase[t] \notin PreDispatch
    /\ connection' = connection + 1 /\ live' = TRUE
    /\ last' = 0 /\ pending' = 0 /\ discovered' = 0 /\ visible' = NoStatus /\ pin' = NoStatus
    /\ UNCHANGED <<catalog, ticketVars>>

Next == Discover \/ ReplaceCatalog \/ ReadStatus \/ ClunkOrFlush \/ Disconnect \/ Reconnect
        \/ (\E r \in Requests : Submit(r))
        \/ (\E t \in Tickets : RequestCheck(t) \/ Approve(t) \/ Dispatch(t)
              \/ BeginComplete(t) \/ WriteFinish(t) \/ ExpireDispatched(t)
              \/ Settle(t) \/ Reclaim(t)
              \/ (t = <<connection, pending>> /\ Publish(t))
              \/ (\E why \in {"timed-out", "denied", "stale"} : BeginCancel(t, why)))

TypeOK == /\ connection \in Connections /\ live \in BOOLEAN /\ catalog \in 1..2
          /\ last \in 0..MaxRequest /\ pending \in 0..MaxRequest /\ active \subseteq Tickets
          /\ phase \in [Tickets -> {"unused", "queued", "checking", "checked", "dispatched", "cancelled"}]
          /\ settled \in [Tickets -> BOOLEAN]
AtMostOnce == \A t \in Tickets : accepted[t] <= 1 /\ effects[t] <= 1
CancellationWins == \A t \in cancelled : effects[t] = 0
ExecutionCustody == \A t \in Tickets : (effects[t] > 0 /\ ~settled[t]) => t \in active
BoundedCustody == Cardinality(active) <= Capacity
ResultRetention == visible.request = last
CurrentDispatch == ~badDispatch
Spec == Init /\ [][Next]_vars
=============================================================================
