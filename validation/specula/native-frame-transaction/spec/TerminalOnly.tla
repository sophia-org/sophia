------------------------ MODULE TerminalOnly ------------------------
EXTENDS base

\* Proposed negotiated mode, NOT the implemented r13 protocol.
\* Current source: content/submission.rs:34 calls content_prepared; the proposal
\* keeps that internal handoff but releases its reserved notice without a write.
\* Admission deliberately still reserves the original slots (base.Admit).
LocalPrepared(f) ==
    /\ s.tx[f].handed /\ "prepared" \in s.tx[f].owed
    /\ s.tx[f].phase = "ready" /\ Current(f)
    /\ s' = [s EXCEPT !.tx[f].owed = @ \ {"prepared"},
             !.credits = IF Fault = "prepared-credit" THEN @ + 1 ELSE @,
             !.queue = IF Fault = "prepared-wire"
                       THEN Append(@, Event(f, "prepared")) ELSE @]

\* Fault control: exactly base.Submit with only the Prepared obligation guard
\* removed. Scene/authority checks and all state updates remain identical.
SubmitWithoutLocalPrepared(f, h) ==
    /\ Fault = "skip-local-prepared"
    /\ s.tx[f].phase = "ready" /\ h \notin s.tx[f].submitted
    /\ s.tx[f].prepared = Heads /\ Current(f) /\ s.flight[h] = 0
    /\ s' = [s EXCEPT !.flight[h] = f, !.tx[f].submitted = @ \cup {h},
             !.unsafeSubmit = @ \/ ~s.tx[f].valid \/ s.tx[f].prepared # Heads,
             !.unsafeEpoch = @ \/ ~Current(f)]

\* The existing copy, source, scene, flip, terminal and revocation actions are
\* reused unchanged. Only PublishPrepared is replaced; no completion is hidden.
TerminalOnlyNext ==
    (\E f \in Frames : Admit(f) \/ Refuse(f) \/ CopyDone(f)
        \/ (\E good \in BOOLEAN : Validate(f, good)) \/ Cancel(f)
        \/ PublishSource(f) \/ Reuse(f) \/ Handoff(f) \/ LocalPrepared(f)
        \/ Terminal(f) \/ DiscardOld(f)
        \/ (\E h \in Heads : Prepare(f, h) \/ Submit(f, h) \/ SubmitWithoutLocalPrepared(f, h)))
    \/ (\E h \in Heads : Flip(h) \/ DrainHead(h) \/ RetireBlank(h))
    \/ ChangeFacts \/ Close \/ CloseNotice \/ Reopen \/ Revoke
    \/ Deliver \/ FreePredecessor
TerminalOnlySpec == Init /\ [][TerminalOnlyNext]_vars
FairTerminalOnlySpec == TerminalOnlySpec
    /\ (\A f \in Frames : WF_vars(CopyDone(f))
        /\ WF_vars(\E good \in BOOLEAN : Validate(f, good))
        /\ WF_vars(Handoff(f)) /\ WF_vars(PublishSource(f))
        /\ WF_vars(LocalPrepared(f)) /\ WF_vars(Terminal(f))
        /\ (\A h \in Heads : WF_vars(Prepare(f, h)) /\ WF_vars(Submit(f, h))))
    /\ (\A h \in Heads : WF_vars(Flip(h))) /\ WF_vars(Deliver)
    /\ WF_vars(CloseNotice)

\* PreparedOrder in base requires a wire notice, so it is intentionally NOT
\* asserted here. Internal full-scene preparation remains mandatory instead.
PreparationSafe == \A f \in Frames :
    s.tx[f].terminal = "presented" =>
        s.tx[f].handed /\ s.tx[f].prepared = Heads
PreparedBeforeSubmit == \A f \in Frames :
    s.tx[f].submitted # {} => "prepared" \notin s.tx[f].owed
NoPreparedEvents == \A i \in 1..Len(s.queue) : s.queue[i].kind # "prepared"
=====================================================================
