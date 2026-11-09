----------------------------- MODULE CommitOnly -----------------------------
\* Proposed commit-only carrier. Existing semantics: export/transaction.rs
\* submit_frame_record, apply_ack, read_reusable_transaction; LeanPrefix.
\* This projection deliberately keeps journal delivery outside the new carrier.
EXTENDS Naturals, TLC
CONSTANTS ReserveBeforeAck, GuardDelivery, EchoAck, ConfirmSuccessOnly,
          KeepDeadline, NoUnknownReplay
VARIABLE s
vars == <<s>>
Init == s = [live |-> TRUE, token |-> TRUE, epoch |-> 1,
  phase |-> "idle", attempt |-> 0, word |-> 0, applied |-> 0,
  confirmed |-> 0, echoed |-> 0, reserved |-> FALSE, room |-> TRUE,
  capacity |-> TRUE, accepted |-> FALSE, known |-> FALSE, result |-> "none",
  unknown |-> FALSE, expiry |-> 1, badDeadline |-> FALSE,
  badCapacity |-> FALSE, badDelivery |-> FALSE, badConfirm |-> FALSE,
  badEcho |-> FALSE, replayed |-> FALSE]
\* One outstanding attempt, two bounded attempts of the same submission.
Publish == /\ s.phase \in {"idle", "retry"} /\ s.attempt < 2
           /\ s.live /\ (~s.unknown \/ ~NoUnknownReplay)
           /\ \E w \in 0..1 :
                s' = [s EXCEPT !.phase = "queued", !.attempt = @ + 1,
                     !.word = w, !.replayed = s.replayed \/ s.unknown,
                     !.badDeadline = s.badDeadline \/ (s.phase = "retry" /\ ~KeepDeadline),
                     !.expiry = IF s.phase = "retry" /\ ~KeepDeadline THEN 2 ELSE s.expiry]
\* Capture consumes only transport storage; no ACK/admission effect yet.
Capture == /\ s.phase = "queued" /\ (s.capacity \/ ~ReserveBeforeAck)
           /\ s' = [s EXCEPT !.phase = "captured", !.reserved = s.capacity]
\* Caller-fresh entry and token expiry precede ACK application. Abstract ACK
\* 0 is no-op, 1 is the validated eligible journal frontier in this projection.
Process == /\ s.phase = "captured" /\ s.live /\ s.token /\ s.epoch = 1
           /\ s' = [s EXCEPT !.phase = "pending",
                 !.applied = IF s.word = 1 THEN 1 ELSE s.applied,
                 !.result = IF s.accepted THEN "already" ELSE IF s.room THEN "success" ELSE "again",
                 !.accepted = s.accepted \/ s.room,
                 !.badCapacity = s.badCapacity \/ ~s.reserved]
\* with_delivery-equivalent fresh gate is distinct from acceptance. Never
\* publish into a withdrawn/replacement attach, even for already accepted work.
RefuseExpired == /\ s.phase = "captured" /\ s.live /\ ~s.token /\ s.epoch = 1
                 /\ s' = [s EXCEPT !.phase = "pending", !.result = "stale"]
Deliver == /\ s.phase = "pending"
           /\ ((s.live /\ s.epoch = 1) \/ ~GuardDelivery)
           /\ s' = [s EXCEPT !.phase = "delivered",
                 !.echoed = IF EchoAck THEN s.word ELSE 1 - s.word,
                 !.badDelivery = ~(s.live /\ s.epoch = 1)]
\* Delivered bytes may be observed after withdrawal, as already-written socket
\* bytes may be. Success alone establishes custody and confirms that attempt's word.
Receive == /\ s.phase = "delivered"
           /\ LET success == s.result = "success"
                  confirms == success \/ ~ConfirmSuccessOnly
              IN s' = [s EXCEPT !.phase = IF s.result = "again" THEN "retry" ELSE "done",
                 !.known = s.known \/ success,
                 !.confirmed = IF confirms /\ s.echoed > s.confirmed THEN s.echoed ELSE s.confirmed,
                 !.badConfirm = s.badConfirm \/ (confirms /\ ~success),
                 !.badEcho = s.badEcho \/ (success /\ s.echoed # s.word),
                 !.reserved = FALSE]
\* Standalone ACK cannot overlap a published but unanswered commit.
IdleAck == /\ s.phase \in {"idle", "done", "retry"} /\ s.live /\ ~s.unknown
           /\ s' = [s EXCEPT !.applied = 1, !.confirmed = 1]
Expire == /\ s.token /\ s' = [s EXCEPT !.token = FALSE]
Withdraw == /\ s.live /\ s' = [s EXCEPT !.live = FALSE]
\* Lost reply/withdrawal abandons knowledge, not accepted work. No implicit
\* accepted-work cancellation or pixel-credit return is represented here.
Close == /\ ~s.live /\ s.phase # "closed"
         /\ s' = [s EXCEPT !.phase = "closed", !.reserved = FALSE,
                   !.unknown = s.unknown \/ (s.attempt > 0 /\ ~s.known)]
Reopen == /\ s.phase = "closed" /\ s.epoch = 1
          /\ s' = [s EXCEPT !.phase = "idle", !.epoch = 2, !.live = TRUE,
                    !.token = TRUE]
Room == s' = [s EXCEPT !.room = ~@]
Capacity == s' = [s EXCEPT !.capacity = ~@]
Next == Publish \/ Capture \/ Process \/ RefuseExpired \/ Deliver \/ Receive \/ IdleAck
        \/ Expire \/ Withdraw \/ Close \/ Reopen \/ Room \/ Capacity
Spec == Init /\ [][Next]_vars
KnownIsAccepted == ~s.known \/ s.accepted
CapacityBeforeEffects == ~s.badCapacity
FreshDelivery == ~s.badDelivery
ExactAckEcho == ~s.badEcho
SuccessConfirms == ~s.badConfirm
ConfirmedIsApplied == s.confirmed <= s.applied
OriginalDeadline == ~s.badDeadline
NoReplay == ~s.replayed
=============================================================================
