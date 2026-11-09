-------------------------- MODULE WorkerSubmit --------------------------
EXTENDS Naturals, TLC

CONSTANT Fault
VARIABLE s
vars == <<s>>

\* Proposed one-frame/one-head handoff, NOT an implementation refinement.
\* Render remains asynchronous. Existing SFT Submit requires Current(f):
\* native-frame-transaction/spec/base.tla:83-90. A gate below is a new premise.
Init == s = [phase |-> "idle", epoch |-> 1, ticketEpoch |-> 0,
             valid |-> TRUE, withdrawRequested |-> FALSE, gate |-> FALSE,
             ledger |-> FALSE, retained |-> FALSE, cardArmed |-> FALSE,
             accepted |-> FALSE, returned |-> FALSE, outcome |-> "none",
             kernel |-> "none", observed |-> FALSE, detached |-> FALSE,
             replyOpen |-> TRUE, workerAlive |-> TRUE, stalled |-> FALSE,
             staleSubmit |-> FALSE, wrongCompletion |-> FALSE]

\* worker.rs:567-592: outbound Render/request correlation remains.
\* New premise: reserve independent custody and arm completion before sending.
Dispatch ==
    /\ s.phase = "idle" /\ s.valid /\ s.replyOpen /\ s.workerAlive
    /\ s' = [s EXCEPT !.phase = "rendering", !.ticketEpoch = s.epoch,
             !.retained = TRUE,
             !.cardArmed = Fault # "late-card"]

RenderDone ==
    /\ s.phase = "rendering" /\ s.workerAlive
    /\ s' = [s EXCEPT !.phase = "ready"]

\* New linearized submission gate. No memory ordering or OS mutex is modeled.
\* Must cover withdrawal, seat/topology generation and all competing submitters.
CheckAndReserve ==
    /\ s.phase = "ready" /\ s.workerAlive /\ ~s.gate
    /\ s.valid /\ ~s.withdrawRequested /\ s.ticketEpoch = s.epoch
    /\ s' = [s EXCEPT !.phase = "checked", !.gate = TRUE,
             !.ledger = Fault # "late-ledger"]

RequestWithdrawal ==
    /\ ~s.withdrawRequested
    /\ s' = [s EXCEPT !.withdrawRequested = TRUE]

\* Request and effective withdrawal differ; this is a critical progress limit.
ApplyWithdrawal ==
    /\ s.withdrawRequested /\ s.valid
    /\ (~s.gate \/ Fault = "unserialized")
    /\ s' = [s EXCEPT !.valid = FALSE, !.epoch = 2]

\* Kernel acceptance and userspace return are deliberately separate: a flip
\* can race the result report. prepare.rs submit seam would move to the worker.
KernelAccept ==
    /\ s.phase = "checked" /\ s.workerAlive /\ ~s.stalled
    /\ s' = [s EXCEPT !.phase = "in-ioctl", !.accepted = TRUE,
             !.kernel = "pending",
             !.staleSubmit = ~s.valid \/ s.ticketEpoch # s.epoch]

KernelReject ==
    /\ s.phase = "checked" /\ s.workerAlive /\ ~s.stalled
    /\ s' = [s EXCEPT !.phase = "rejected", !.returned = TRUE,
             !.gate = FALSE, !.outcome = "failed"]

PublishAccepted ==
    /\ s.phase = "in-ioctl" /\ s.workerAlive /\ ~s.stalled
    /\ s' = [s EXCEPT !.phase = "returned", !.returned = TRUE,
             !.gate = FALSE, !.ledger = TRUE, !.outcome = "accepted"]

\* Exact kernel callback identity is abstracted as the sole reserved ticket.
\* The prepared ledger must be enough to preserve an early event, not just
\* a channel response installed by singleton_tick.rs:124-192 after return.
KernelFlip ==
    /\ s.kernel = "pending"
    /\ s' = [s EXCEPT !.kernel = "visible"]

ObserveExactCompletion ==
    /\ s.kernel = "visible" /\ ~s.observed /\ s.cardArmed /\ s.ledger
    /\ s' = [s EXCEPT !.observed = TRUE,
             !.retained = IF Fault = "free-on-flip" THEN FALSE ELSE @]

ObserveForeignCompletion ==
    /\ s.kernel = "visible" /\ ~s.observed
    /\ Fault = "accept-foreign"
    /\ s' = [s EXCEPT !.observed = TRUE, !.wrongCompletion = TRUE]

\* Existing service.rs drops output state on failed reply delivery, detached
\* claim, Deregister and shutdown. Safe only before adding worker-side submit.
\* Proposed custody lives outside those routes; disconnect is not detach proof.
Disconnect ==
    /\ s.replyOpen
    /\ s' = [s EXCEPT !.replyOpen = FALSE,
             !.retained = IF Fault = "drop-on-disconnect" /\ s.accepted
                          THEN FALSE ELSE @]

WorkerStops ==
    /\ s.workerAlive
    /\ s' = [s EXCEPT !.workerAlive = FALSE, !.gate = FALSE,
             !.retained = IF Fault = "drop-on-worker-stop" /\ s.accepted
                          THEN FALSE ELSE @,
             !.outcome = IF s.accepted THEN "uncertain" ELSE "failed"]

\* Scheduling/driver delay is not a completed thread exit and cannot unlock.
StallWorker ==
    /\ s.workerAlive /\ ~s.stalled /\ s.gate
    /\ s' = [s EXCEPT !.stalled = TRUE]

\* Abstract existing physical retirement/device-detach proof, NOT a timeout,
\* lost reply, merely requesting shutdown, or declaring a worker dead.
PhysicalDetach ==
    /\ s.accepted /\ ~s.detached /\ ~s.gate
    /\ s' = [s EXCEPT !.detached = TRUE, !.kernel = "none"]

Release ==
    /\ s.retained /\ ~s.gate
    /\ (s.detached \/ (~s.accepted /\ (s.returned \/ ~s.workerAlive)))
    /\ s' = [s EXCEPT !.retained = FALSE]

Next == Dispatch \/ RenderDone \/ CheckAndReserve \/ RequestWithdrawal
        \/ ApplyWithdrawal \/ KernelAccept \/ KernelReject \/ PublishAccepted
        \/ KernelFlip \/ ObserveExactCompletion \/ ObserveForeignCompletion
        \/ Disconnect \/ WorkerStops \/ StallWorker \/ PhysicalDetach \/ Release
Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ s.phase \in {"idle", "rendering", "ready", "checked", "in-ioctl",
                    "rejected", "returned"}
    /\ s.epoch \in {1, 2} /\ s.ticketEpoch \in {0, 1, 2}
    /\ s.kernel \in {"none", "pending", "visible"}
    /\ s.outcome \in {"none", "accepted", "failed", "uncertain"}
    /\ \A k \in {"valid", "withdrawRequested", "gate", "ledger", "retained",
                  "cardArmed", "accepted", "returned", "observed", "detached",
                  "replyOpen", "workerAlive", "stalled", "staleSubmit", "wrongCompletion"}:
          s[k] \in BOOLEAN
FreshSubmit == ~s.staleSubmit
PhysicalCustody == s.accepted /\ ~s.detached => s.retained
SubmittedAccounted == s.accepted => s.ledger
CompletionArmed == s.accepted /\ ~s.detached => s.cardArmed
ExactCompletion == s.observed => s.accepted /\ ~s.wrongCompletion

\* Deliberately probing the remaining progress gap. Fairness cannot make a
\* stalled worker's disabled action happen. WorkerStops is not forced: a stuck
\* thread/driver has not exited merely because the owner wants to stop it.
ProgressSpec == Spec /\ WF_vars(ApplyWithdrawal)
                    /\ WF_vars(KernelAccept \/ KernelReject \/ PublishAccepted)
WithdrawalEventuallyApplied == s.withdrawRequested ~> ~s.valid

\* Native deadlines and detach proof remain outside this bounded model.
\* A fair PhysicalDetach action must not be used to assume the gap away.
=============================================================================
