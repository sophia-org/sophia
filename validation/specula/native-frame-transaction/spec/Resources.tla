-------------------------- MODULE Resources --------------------------
EXTENDS Naturals, FiniteSets
CONSTANT Fault
VARIABLE r, epoch, unsafePublication
vars == <<r, epoch, unsafePublication>>
Ids == {1, 2}
Consumers == {1, 2}
Empty == [phase |-> "unused", owner |-> 0, bytes |-> 0, charged |-> FALSE,
          reading |-> FALSE, stopped |-> FALSE, accepted |-> FALSE,
          leases |-> {}, queued |-> FALSE, published |-> FALSE]
Init == /\ r = [i \in Ids |-> Empty] /\ epoch = 1
        /\ unsafePublication = FALSE
Charges == {p \in (Ids \X {1, 2}) : r[p[1]].charged /\ p[2] <= r[p[1]].bytes}

\* Reservation includes old epochs still holding physical storage.
Reserve(i, bytes) ==
    /\ r[i].phase = "unused" /\ Cardinality(Charges) + bytes <= 3
    /\ r' = [r EXCEPT ![i] = [Empty EXCEPT !.phase = "copy", !.owner = epoch,
                   !.bytes = bytes, !.charged = TRUE, !.reading = TRUE]]
    /\ UNCHANGED <<epoch, unsafePublication>>
\* resources.rs:270-306: cancellation retains the worker's storage charge.
Cancel(i) ==
    /\ r[i].phase = "copy" /\ ~r[i].stopped
    /\ r' = [r EXCEPT ![i].stopped = TRUE,
             ![i].charged = IF Fault = "cancel-frees" THEN FALSE ELSE @]
    /\ UNCHANGED <<epoch, unsafePublication>>
Finish(i, valid) ==
    /\ r[i].reading
    /\ LET accepted == valid /\ ~r[i].stopped /\ r[i].owner = epoch IN
       r' = [r EXCEPT ![i].reading = FALSE, ![i].accepted = accepted,
             ![i].phase = IF accepted THEN "accepted" ELSE "failed"]
    /\ UNCHANGED <<epoch, unsafePublication>>
\* resources.rs:436-456: retirement prevents new leases, not existing readers.
Lease(i, c) ==
    /\ r[i].phase = "accepted" /\ r[i].owner = epoch
    /\ ~r[i].stopped /\ c \notin r[i].leases
    /\ r' = [r EXCEPT ![i].leases = @ \cup {c}]
    /\ UNCHANGED <<epoch, unsafePublication>>
DropLease(i, c) ==
    /\ c \in r[i].leases /\ r' = [r EXCEPT ![i].leases = @ \ {c}]
    /\ UNCHANGED <<epoch, unsafePublication>>
Retire(i) ==
    /\ r[i].phase = "accepted" /\ ~r[i].stopped
    /\ r' = [r EXCEPT ![i].stopped = TRUE]
    /\ UNCHANGED <<epoch, unsafePublication>>
QueueRelease(i) ==
    /\ r[i].phase = "accepted" /\ r[i].owner = epoch
    /\ r[i].stopped /\ r[i].leases = {} /\ ~r[i].queued
    /\ r' = [r EXCEPT ![i].queued = TRUE]
    /\ UNCHANGED <<epoch, unsafePublication>>
\* FIFO transfer alone is not journal publication (resources.rs:484-560).
PublishRelease(i) ==
    /\ r[i].queued /\ ~r[i].published /\ r[i].phase # "freed"
    /\ (r[i].owner = epoch \/ Fault = "stale-publication")
    /\ r' = [r EXCEPT ![i].published = TRUE]
    /\ unsafePublication' = (unsafePublication \/ r[i].owner # epoch)
    /\ UNCHANGED epoch
Free(i) ==
    /\ r[i].phase \in {"accepted", "failed"} /\ ~r[i].reading
    /\ (r[i].leases = {} \/ Fault = "free-leased")
    /\ (r[i].phase = "failed" \/ r[i].owner # epoch
         \/ (r[i].stopped /\ (r[i].published \/ (Fault = "queue-is-publication" /\ r[i].queued))))
    /\ r' = [r EXCEPT ![i].phase = "freed", ![i].charged = FALSE]
    /\ UNCHANGED <<epoch, unsafePublication>>
Revoke == /\ epoch = 1 /\ epoch' = 2 /\ UNCHANGED <<r, unsafePublication>>
Next == Revoke \/ (\E i \in Ids : Cancel(i) \/ Retire(i) \/ QueueRelease(i)
     \/ PublishRelease(i) \/ Free(i)
     \/ (\E b \in {1, 2} : Reserve(i, b))
     \/ (\E valid \in BOOLEAN : Finish(i, valid))
     \/ (\E c \in Consumers : Lease(i, c) \/ DropLease(i, c)))
Spec == Init /\ [][Next]_vars
BudgetSafe == Cardinality(Charges) <= 3
ChargedUse == \A i \in Ids : (r[i].reading \/ r[i].leases # {}) => r[i].charged
FreeSafe == \A i \in Ids : r[i].phase = "freed" => ~r[i].reading /\ r[i].leases = {}
PublicationSafe == \A i \in Ids :
    r[i].accepted /\ r[i].owner = epoch /\ ~r[i].charged => r[i].published
EpochSafe == ~unsafePublication
FairSpec == Spec /\ (\A i \in Ids : WF_vars(\E v \in BOOLEAN : Finish(i, v))
    /\ WF_vars(QueueRelease(i)) /\ WF_vars(PublishRelease(i)) /\ WF_vars(Free(i))
    /\ (\A c \in Consumers : WF_vars(DropLease(i, c))))
RetiredDrains == \A i \in Ids :
    (r[i].phase # "unused" /\ (r[i].stopped \/ r[i].owner # epoch))
        ~> (r[i].phase = "freed")
=============================================================================
