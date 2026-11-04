------------------------------ MODULE Intake ------------------------------
EXTENDS Naturals, FiniteSets, Sequences
CONSTANTS Capacity, Fault
VARIABLE s, request
vars == <<s, request>>
Ids == {1, 2}
Kinds == {"source", "prepared", "terminal"}
Empty == [active |-> FALSE, reading |-> FALSE, parts |-> 0, owed |-> {},
          disposition |-> "none", terminal |-> FALSE]
Init == /\ request \in [Ids -> {"valid", "stale", "budget"}]
        /\ s = [head |-> 1, fragments |-> 0, open |-> TRUE, closeDue |-> TRUE,
                 tx |-> [f \in Ids |-> Empty], queue |-> <<"other", "other">>,
                 refusals |-> {}, unsafeAdmission |-> FALSE]
Held == {f \in Ids : s.tx[f].active}
Owed == {p \in (Ids \X Kinds) : p[2] \in s.tx[p[1]].owed}
Used == Cardinality(Owed) + Len(s.queue) + IF s.closeDue THEN 1 ELSE 0
Complete == s.head \in Ids /\ s.fragments = 2
Eligible == s.open /\ request[s.head] = "valid" /\ Held = {}

\* Proposed bounded complete FrameCommit: transport fragments are not new
\* requests. Existing resource intake peeks before taking (content_resources.rs:156).
DecodeFragment ==
    /\ s.head \in Ids /\ s.fragments < 2
    /\ (Fault # "fragment-credit" \/ Used < Capacity)
    /\ s' = [s EXCEPT !.fragments = @ + 1]
    /\ UNCHANGED request
Admit ==
    /\ Complete /\ (Eligible \/ (Fault = "stale-peek" /\ ~s.open
                       /\ request[s.head] = "valid" /\ Held = {}))
    /\ Used + 3 <= Capacity
    /\ s' = [s EXCEPT !.head = @ + 1, !.fragments = 0,
             !.unsafeAdmission = @ \/ ~s.open,
             !.tx[s.head] = [Empty EXCEPT !.active = TRUE, !.reading = TRUE,
                             !.parts = 2, !.owed = Kinds, !.disposition = "accepted"]]
    /\ UNCHANGED request
\* Budget/closed-stream refusals own one reply before consuming input. Unlike
\* today's unreported candidate error (content_candidates.rs:224), this is nonfatal.
Refuse ==
    /\ Complete /\ ~Eligible
    /\ (Used < Capacity \/ Fault = "unreserved-refusal")
    /\ s' = [s EXCEPT !.head = @ + 1, !.fragments = 0,
             !.queue = Append(@, s.head), !.refusals = @ \cup {s.head},
             !.tx[s.head].disposition = "refused"]
    /\ UNCHANGED request
CopyDone(f) ==
    /\ s.tx[f].reading /\ s' = [s EXCEPT !.tx[f].reading = FALSE]
    /\ UNCHANGED request
\* Internal Begin/Chunk/End expansion uses the already-owned reservation.
\* A blanket +1 here can deadlock at full budget (content_candidates.rs:123).
Expand(f) ==
    /\ s.tx[f].active /\ ~s.tx[f].reading /\ s.tx[f].parts > 0
    /\ (Fault # "continuation-credit" \/ Used < Capacity)
    /\ s' = [s EXCEPT !.tx[f].parts = @ - 1]
    /\ UNCHANGED request
\* A rejected internal Begin discards its own rest, without taking the next
\* client's record. Accepted copy ownership survives (owned_copy/admission.rs:55).
Reject(f) ==
    /\ s.tx[f].active /\ s.tx[f].disposition = "accepted"
    /\ "prepared" \in s.tx[f].owed
    /\ s' = [s EXCEPT !.tx[f].disposition = "rejected",
             !.tx[f].owed = @ \ {"prepared"}]
    /\ UNCHANGED request
Publish(f, kind) ==
    /\ kind \in s.tx[f].owed /\ ~s.tx[f].reading
    /\ (kind = "source" \/ s.tx[f].parts = 0)
    /\ (kind # "terminal" \/ "prepared" \notin s.tx[f].owed)
    /\ s' = [s EXCEPT !.tx[f].owed = @ \ {kind},
             !.queue = Append(@, kind),
             !.tx[f].active = IF kind = "terminal" THEN FALSE ELSE @,
             !.tx[f].terminal = IF kind = "terminal" THEN TRUE ELSE @]
    /\ UNCHANGED request
Close == /\ s.open /\ s' = [s EXCEPT !.open = FALSE] /\ UNCHANGED request
PublishClose ==
    /\ ~s.open /\ s.closeDue
    /\ s' = [s EXCEPT !.closeDue = FALSE, !.queue = Append(@, "closed")]
    /\ UNCHANGED request
Deliver == /\ Len(s.queue) > 0 /\ s' = [s EXCEPT !.queue = Tail(@)]
           /\ UNCHANGED request
Next == DecodeFragment \/ Admit \/ Refuse \/ Close \/ PublishClose \/ Deliver
     \/ (\E f \in Ids : CopyDone(f) \/ Expand(f) \/ Reject(f)
                         \/ (\E k \in Kinds : Publish(f, k)))
Spec == Init /\ [][Next]_vars
BudgetSafe == Used <= Capacity
CreditSafe == Cardinality(Held) <= 1
ReaderSafe == \A f \in Ids : s.tx[f].reading => s.tx[f].active
AdmissionSafe == ~s.unsafeAdmission
RefusalSafe == \A f \in Ids :
    (s.tx[f].disposition = "refused") = (f \in s.refusals)
SettlementSafe == \A f \in Ids :
    s.tx[f].terminal => ~s.tx[f].reading /\ s.tx[f].parts = 0
FragmentsEnabled == s.head \in Ids /\ s.fragments < 2 => ENABLED DecodeFragment
ContinuationEnabled == \A f \in Ids :
    s.tx[f].active /\ ~s.tx[f].reading /\ s.tx[f].parts > 0 => ENABLED Expand(f)
FairSpec == Spec /\ WF_vars(DecodeFragment) /\ WF_vars(Admit) /\ WF_vars(Refuse)
    /\ WF_vars(Deliver) /\ WF_vars(PublishClose)
    /\ (\A f \in Ids : WF_vars(CopyDone(f)) /\ WF_vars(Expand(f))
                      /\ (\A k \in Kinds : WF_vars(Publish(f, k))))
InputDrains == <> (s.head = 3)
AcceptedSettles == \A f \in Ids : s.tx[f].active ~> s.tx[f].terminal
=============================================================================
