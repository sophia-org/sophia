--------------------------- MODULE LeanPrefix ---------------------------
\* Lean SFT commits (bit 20): the header sequence word of a complete
\* frame-submit FrameCommit is a cumulative ACK applied before admission, no
\* Submitted record is published, the exact Rwrite is custody and nothing is
\* retained for the commit. The first acceptance (an Open on the paired path,
\* sequence 1) stays retained until a covering ACK, as before. Derived from
\* the AckPrefix fold analysis; AlterRetry and SettleObserved are gone because
\* no bytes are retained and no Submitted can be observed.
EXTENDS Naturals, FiniteSets, TLC

CONSTANTS ValidateEpoch, ValidateAck, RequireWriter, RequireCoveringAck,
          Standalone, KnownOnAnyReply, ConfirmOnError, EmitSubmitted
VARIABLE s
vars == <<s>>
Identity(id, bytes) == [id |-> id, bytes |-> bytes]
First == Identity(1, 1)
Second(bytes) == Identity(2, bytes)
\* The held announcement is sequence 2; a lean commit publishes no record.
Limit == IF s.hold = 0 THEN s.handled
         ELSE IF s.handled < s.hold THEN s.handled ELSE s.hold - 1
EmptyAttempt == [epoch |-> 1, bytes |-> 1, ack |-> 0, valid |-> TRUE]

Init == s = [live |-> TRUE, writer |-> TRUE, tail |-> 2, acked |-> 0,
             confirmed |-> 0, handled |-> 1, hold |-> 2, accepted |-> 1,
             sequence |-> 1, history |-> {First}, known |-> {First},
             phase |-> "idle", attempt |-> EmptyAttempt, reply |-> "none",
             room |-> TRUE, unsafeReuse |-> FALSE, badEpochEffect |-> FALSE,
             badAckEffect |-> FALSE, staleWriterSuccess |-> FALSE]

\* Handling events never grows known: only the exact Rwrite does.
Observe == /\ s.live /\ s.handled < s.tail
           /\ s' = [s EXCEPT !.handled = @ + 1]
ReleaseHold == /\ s.hold = 2 /\ s.handled >= 2
               /\ s' = [s EXCEPT !.hold = 0]

\* One complete envelope; the word is 0 (nothing new, or manual ACK policy),
\* the ACK bound, or an invalid probe (4).
Queue == /\ s.live /\ s.phase = "idle"
         /\ ~\E x \in s.known : x.id = 2
         /\ \E ep \in {1, 2}, b \in {1, 2}, a \in {0, Limit, 4}, v \in BOOLEAN :
              s' = [s EXCEPT !.phase = "queued",
                     !.attempt = [epoch |-> ep, bytes |-> b, ack |-> a, valid |-> v]]

AckValid(a) == a = 0 \/ (a >= s.acked /\ a <= s.tail)
PrefixAck == IF s.attempt.ack = 0 THEN s.acked ELSE s.attempt.ack
RetainedAfterAck == IF s.accepted # 0 /\ s.sequence <= PrefixAck
                   THEN 0 ELSE s.accepted

\* Host: envelope, epoch and ACK validation, ACK applied, then admission.
\* An admitted lean commit is recycled at once: accepted' = 0, sequence' = 0,
\* and no record is appended (tail unchanged) unless EmitSubmitted (control).
Deliver ==
    /\ s.phase = "queued"
    /\ LET tokenOK == s.live /\ (s.writer \/ ~RequireWriter)
           epochOK == s.attempt.epoch = 1 \/ ~ValidateEpoch
           ackOK == AckValid(s.attempt.ack) \/ ~ValidateAck
           retained == RetainedAfterAck
           prior == \E x \in s.history : x.id = 2
           new == retained = 0 /\ ~prior /\ s.attempt.valid /\ s.room
           unchecked == ~RequireCoveringAck /\ ~prior /\ s.attempt.valid /\ s.room
           accept == new \/ unchecked
           result == IF retained # 0 /\ ~unchecked THEN "busy"
                     ELSE IF prior THEN "already"
                     ELSE IF ~s.attempt.valid THEN "refused"
                     ELSE IF ~s.room THEN "backpressure"
                     ELSE "success"
       IN IF ~tokenOK \/ ~epochOK \/ ~ackOK
          THEN s' = [s EXCEPT !.phase = "reply", !.reply = "invalid"]
          ELSE s' = [s EXCEPT
                 !.phase = "reply", !.reply = result,
                 !.acked = PrefixAck,
                 !.accepted = IF accept THEN 0 ELSE retained,
                 !.sequence = IF accept THEN 0 ELSE s.sequence,
                 !.tail = IF accept /\ EmitSubmitted THEN 3 ELSE s.tail,
                 !.history = IF accept THEN s.history \cup {Second(s.attempt.bytes)} ELSE s.history,
                 !.writer = IF result = "refused" THEN FALSE ELSE s.writer,
                 !.unsafeReuse = s.unsafeReuse \/
                    (accept /\ s.accepted # 0 /\ PrefixAck < s.sequence),
                 !.badEpochEffect = s.badEpochEffect \/
                    (s.attempt.epoch # 1 /\ (accept \/ PrefixAck # s.acked)),
                 !.badAckEffect = s.badAckEffect \/
                    (~AckValid(s.attempt.ack) /\ (accept \/ PrefixAck # s.acked)),
                 !.staleWriterSuccess = s.staleWriterSuccess \/
                    (~s.writer /\ result = "success")]

\* Client: the exact Rwrite alone is custody and confirms the word; any other
\* reply confirms nothing and the commit is unknown only through disconnect.
Receive == /\ s.phase = "reply"
           /\ LET success == s.reply = "success" IN
              s' = [s EXCEPT !.phase = IF s.reply = "backpressure" THEN "retry" ELSE "idle",
                     !.known = IF success \/ KnownOnAnyReply THEN
                                   s.known \cup {Second(s.attempt.bytes)} ELSE s.known,
                     !.confirmed = IF (success \/ ConfirmOnError) /\ s.attempt.ack # 0
                                   THEN s.attempt.ack ELSE s.confirmed,
                     !.reply = "none"]
\* A reply is lost only with the connection: no retry, nothing learned.
LoseReply == /\ s.phase = "reply"
             /\ s' = [s EXCEPT !.phase = "idle", !.reply = "none",
                       !.live = FALSE, !.writer = FALSE]
\* Backpressure retry resends the same identity; the word may be refreshed.
Retry == /\ s.phase = "retry" /\ s.live
         /\ \E a \in {0, Limit, 4} :
              s' = [s EXCEPT !.phase = "queued", !.attempt.ack = a]

\* The standalone ack node stays available without a frame.
AckIdle == /\ Standalone /\ s.live /\ s.phase = "idle" /\ Limit > s.acked
           /\ s' = [s EXCEPT !.acked = Limit, !.confirmed = Limit,
                     !.accepted = IF s.sequence <= Limit THEN 0 ELSE s.accepted]
RoomChanges == s' = [s EXCEPT !.room = ~@]
Clunk == /\ s.writer /\ s' = [s EXCEPT !.writer = FALSE]
Disconnect == /\ s.live /\ s' = [s EXCEPT !.live = FALSE, !.writer = FALSE]

Next == Queue \/ Deliver \/ Receive \/ LoseReply \/ Retry
        \/ Observe \/ ReleaseHold \/ AckIdle \/ RoomChanges \/ Clunk \/ Disconnect
Spec == Init /\ [][Next]_vars

TypeOK == /\ s.acked \in 0..4 /\ s.confirmed \in 0..4 /\ s.handled \in 1..3
          /\ s.tail \in 2..3 /\ s.accepted \in 0..1 /\ s.sequence \in 0..1
          /\ s.hold \in {0, 2}
          /\ s.phase \in {"idle", "queued", "reply", "retry"}
          /\ s.known \subseteq {First, Second(1), Second(2)}
KnownIsReal == s.known \subseteq s.history
OneAcceptance == Cardinality({x \in s.history : x.id = 2}) <= 1
AckBeforeReuse == ~s.unsafeReuse
EpochBinding == ~s.badEpochEffect
AckValidation == ~s.badAckEffect
WriterBinding == ~s.staleWriterSuccess
HeldFrontier == s.acked <= Limit
\* The client never believes an ACK the host did not apply.
ConfirmedIsApplied == s.confirmed <= s.acked
\* A lean commit publishes no Submitted record.
NoSubmitted == s.tail = 2

IdleNext == AckIdle
IdleSpec == Init /\ [][IdleNext]_vars /\ WF_vars(AckIdle)
IdleProgress == <> (s.acked = 1)
=============================================================================
