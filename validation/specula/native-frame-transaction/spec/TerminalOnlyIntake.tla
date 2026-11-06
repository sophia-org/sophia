-------------------- MODULE TerminalOnlyIntake --------------------
EXTENDS Intake, TLC

\* Proposed replacement for Intake.Publish(f, "prepared"): retain exactly
\* its prerequisites and remove the reserved obligation without a FIFO entry.
\* No complete-input, refusal, fragment, copy or continuation rule changes.
LocalPrepared(f) ==
    /\ "prepared" \in s.tx[f].owed /\ ~s.tx[f].reading
    /\ s.tx[f].parts = 0
    /\ s' = [s EXCEPT !.tx[f].owed = @ \ {"prepared"}]
    /\ UNCHANGED request
TerminalOnlyNext == DecodeFragment \/ Admit \/ Refuse \/ Close \/ PublishClose \/ Deliver
    \/ (\E f \in Ids : CopyDone(f) \/ Expand(f) \/ Reject(f) \/ LocalPrepared(f)
        \/ (\E k \in Kinds \ {"prepared"} : Publish(f, k)))
TerminalOnlySpec == Init /\ [][TerminalOnlyNext]_vars
FairTerminalOnlySpec == TerminalOnlySpec
    /\ WF_vars(DecodeFragment) /\ WF_vars(Admit) /\ WF_vars(Refuse)
    /\ WF_vars(Deliver) /\ WF_vars(PublishClose)
    /\ (\A f \in Ids : WF_vars(CopyDone(f)) /\ WF_vars(Expand(f))
        /\ WF_vars(LocalPrepared(f))
        /\ (\A k \in Kinds \ {"prepared"} : WF_vars(Publish(f, k))))
NoPreparedEvents == \A i \in 1..Len(s.queue) : ToString(s.queue[i]) # ToString("prepared")
===================================================================
