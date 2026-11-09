----------------------------- MODULE Carrier -----------------------------
\* Proposed design projection, NOT an implemented carrier or refinement proof.
\* Correspondence: LeanPrefix Deliver/Receive; export/transaction.rs submit_frame_record/apply_ack.
EXTENDS Naturals, TLC
CONSTANTS ReserveReply, FreshAuthority, ExactReceipt, SnapshotInput, SemanticAck
VARIABLE s
vars == <<s>>
Init == s = [live |-> TRUE, epoch |-> 1, phase |-> "idle", source |-> 1,
  snapshot |-> 0, accepted |-> 0, known |-> 0, receipt |-> 0,
  room |-> TRUE, reserved |-> FALSE, consumed |-> FALSE, ack |-> FALSE,
  reuse |-> FALSE, badAuthority |-> FALSE, unreservedEffect |-> FALSE]
\* Producer publishes one bounded complete record in epoch1. Publication is
\* not custody. A hostile producer may alter bytes at any later point.
Publish == /\ s.phase = "idle" /\ s.live
           /\ s' = [s EXCEPT !.phase = "queued"]
Mutate == /\ s.phase \in {"queued", "captured", "returned", "received"}
          /\ s' = [s EXCEPT !.source = IF @ = 1 THEN 2 ELSE 1]
\* Capture before decoding; reserve response capacity before ANY effect.
Capture == /\ s.phase = "queued" /\ (s.room \/ ~ReserveReply)
           /\ s' = [s EXCEPT !.phase = "captured", !.snapshot = s.source,
                     !.reserved = s.room, !.consumed = TRUE]
\* Fresh authority remains a gate; no wake observation substitutes for it.
Accept == /\ s.phase = "captured"
          /\ ((s.live /\ s.epoch = 1) \/ ~FreshAuthority)
          /\ s' = [s EXCEPT !.phase = "returned", !.accepted = s.snapshot,
                    !.receipt = IF SnapshotInput THEN s.snapshot ELSE s.source,
                    !.badAuthority = ~(s.live /\ s.epoch = 1),
                    !.unreservedEffect = ~s.reserved]
Refuse == /\ s.phase = "captured" /\ ~(s.live /\ s.epoch = 1)
          /\ s' = [s EXCEPT !.phase = "refused"]
\* Success receipt is owner-authored and refers to the privately captured
\* record. A raw consumer head cannot establish knowledge.
Receive == /\ s.phase = "returned" /\ s.live
           /\ s' = [s EXCEPT !.phase = "received", !.known = s.receipt]
HeadAsReceipt == /\ ~ExactReceipt /\ s.consumed /\ s.known = 0
                 /\ s' = [s EXCEPT !.known = s.source]
\* Abstract lifecycle frontier: legitimate covering semantic ACK, not ring head.
Ack == /\ s.phase = "received"
       /\ s' = [s EXCEPT !.ack = TRUE]
Reuse == /\ s.consumed /\ (s.ack \/ ~SemanticAck)
         /\ s' = [s EXCEPT !.reuse = TRUE]
Revoke == /\ s.live /\ s' = [s EXCEPT !.live = FALSE, !.epoch = 2]
Room == s' = [s EXCEPT !.room = ~@]
Next == Publish \/ Mutate \/ Capture \/ Accept \/ Refuse \/ Receive
        \/ HeadAsReceipt \/ Ack \/ Reuse \/ Revoke \/ Room
Spec == Init /\ [][Next]_vars
KnownIsReal == s.known = 0 \/ s.known = s.accepted
ImmutableAcceptance == s.receipt = 0 \/ s.receipt = s.snapshot
AuthoritySafe == ~s.badAuthority
CapacitySafe == ~s.unreservedEffect
AckBeforeReuse == ~s.reuse \/ s.ack
=============================================================================
