----------------------------- MODULE base -----------------------------
EXTENDS Naturals, FiniteSets, Sequences
CONSTANTS DropFull, AcceptStale
VARIABLES phase, pending, wire, consumed, abandoned, effect, epoch, granted, connected
vars == <<phase, pending, wire, consumed, abandoned, effect, epoch, granted, connected>>
Ids == 1..3
Reserved == {i \in Ids : phase[i] \in {"reserved", "effect"}}
Published == {i \in Ids : phase[i] = "published"}
Elems(s) == {s[n] : n \in 1..Len(s)}
Init == /\ phase = [i \in Ids |-> "new"] /\ pending = <<>> /\ wire = <<>>
        /\ consumed = {} /\ abandoned = {} /\ effect = {}
        /\ epoch = 1 /\ granted = {} /\ connected = TRUE
\* Outbox reserve: charge before registry input/grab mutation.
Reserve(i) == /\ connected /\ phase[i] = "new"
              /\ Len(pending) + Cardinality(Reserved) < 2
              /\ phase' = [phase EXCEPT ![i] = "reserved"]
              /\ UNCHANGED <<pending, wire, consumed, abandoned, effect, epoch, granted, connected>>
\* Existing registry effect is separate from committing its notification.
Effect(i) == /\ phase[i] = "reserved"
             /\ phase' = [phase EXCEPT ![i] = "effect"] /\ effect' = effect \cup {i}
             /\ UNCHANGED <<pending, wire, consumed, abandoned, epoch, granted, connected>>
Abort(i) == /\ phase[i] = "reserved" /\ phase' = [phase EXCEPT ![i] = "aborted"]
            /\ UNCHANGED <<pending, wire, consumed, abandoned, effect, epoch, granted, connected>>
\* Outbox commit retains a full-channel obligation; disconnect is explicit abandonment.
Publish(i) == /\ phase[i] = "effect" /\ phase' = [phase EXCEPT ![i] = "published"]
              /\ pending' = IF connected THEN Append(pending, i) ELSE pending
              /\ abandoned' = IF connected THEN abandoned ELSE abandoned \cup {i}
              /\ UNCHANGED <<wire, consumed, effect, epoch, granted, connected>>
\* try_send success transfers custody; Full must leave it intact.
Transfer == /\ connected /\ Len(pending) > 0 /\ Len(wire) = 0
            /\ wire' = Append(wire, Head(pending)) /\ pending' = Tail(pending)
            /\ UNCHANGED <<phase, consumed, abandoned, effect, epoch, granted, connected>>
BadFull == /\ DropFull /\ Len(pending) > 0 /\ Len(wire) = 1
           /\ pending' = Tail(pending)
           /\ UNCHANGED <<phase, wire, consumed, abandoned, effect, epoch, granted, connected>>
\* Session consumes old identities but cannot use them to confirm current authority.
Consume == /\ connected /\ Len(wire) > 0
           /\ consumed' = consumed \cup {Head(wire)}
           /\ granted' = IF epoch = 1 \/ AcceptStale THEN granted \cup {<<Head(wire), epoch>>} ELSE granted
           /\ wire' = Tail(wire)
           /\ UNCHANGED <<phase, pending, abandoned, effect, epoch, connected>>
Advance == /\ epoch = 1 /\ epoch' = 2 /\ granted' = {}
           /\ UNCHANGED <<phase, pending, wire, consumed, abandoned, effect, connected>>
Disconnect == /\ connected /\ connected' = FALSE
              /\ abandoned' = abandoned \cup Elems(pending) \cup Elems(wire)
              /\ pending' = <<>> /\ wire' = <<>>
              /\ UNCHANGED <<phase, consumed, effect, epoch, granted>>
Next == (\E i \in Ids : Reserve(i) \/ Effect(i) \/ Abort(i) \/ Publish(i))
        \/ Transfer \/ BadFull \/ Consume \/ Advance \/ Disconnect
Spec == Init /\ [][Next]_vars
Capacity == Len(pending) + Cardinality(Reserved) <= 2 /\ Len(wire) <= 1
Custody == Published = Elems(pending) \cup Elems(wire) \cup consumed \cup abandoned
Unique == Cardinality(Published) = Len(pending) + Len(wire) + Cardinality(consumed) + Cardinality(abandoned)
EffectCustody == effect = {i \in Ids : phase[i] = "effect"} \cup Published
Authority == \A g \in granted : g[2] = 1 /\ epoch = 1
=============================================================================
