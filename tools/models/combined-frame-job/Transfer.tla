---------------------------- MODULE Transfer ----------------------------
EXTENDS Naturals, FiniteSets
CONSTANT Mutation
VARIABLE x
vars == <<x>>
\* Design projection of TRANSFER.md, not implemented Rust or a channel proof.
Init == x = [phase |-> "idle", holders |-> {}, credit |-> FALSE,
 jobs |-> 0, cancelled |-> FALSE, requested |-> FALSE, withdrawn |-> FALSE,
 copied |-> FALSE, valid |-> FALSE, sourceFinished |-> TRUE,
 renderStartedCancelled |-> FALSE, busy |-> FALSE, slot |-> TRUE,
 resultRoom |-> TRUE]
Reserve == /\ x.phase = "idle" /\ x.jobs < 2 /\ ~x.withdrawn
 /\ ~x.requested /\ x.resultRoom
 /\ x' = [x EXCEPT !.phase = "validating", !.holders = {"validator"},
 !.credit = TRUE, !.jobs = @ + 1, !.copied = FALSE, !.valid = FALSE,
 !.sourceFinished = FALSE]
Capture == /\ x.phase = "validating" /\ ~x.copied
 /\ x' = [x EXCEPT !.copied = TRUE, !.sourceFinished = TRUE]
Validate == /\ x.phase = "validating" /\ x.copied /\ ~x.cancelled
 /\ x' = [x EXCEPT !.phase = "validated", !.valid = TRUE]
\* A successful try_send transfers ownership; Full/Disconnected returns payload.
Enqueue == /\ x.phase = "validated" /\ ~x.cancelled
 /\ x' = [x EXCEPT !.phase = "queued",
 !.holders = IF Mutation = "duplicate" THEN {"validator", "inbox"} ELSE {"inbox"},
 !.credit = IF Mutation = "credit" THEN FALSE ELSE @]
Take == /\ x.phase = "queued"
 /\ x' = [x EXCEPT !.phase = "waiting", !.holders = {"renderer"}]
\* No owner success wake is modeled between validation and this dispatch.
Render == /\ x.phase = "waiting" /\ ~x.busy /\ x.slot
 /\ (~x.cancelled \/ Mutation = "cancel")
 /\ x' = [x EXCEPT !.phase = "rendering", !.slot = FALSE,
 !.renderStartedCancelled = x.cancelled]
RenderDone == /\ x.phase = "rendering"
 /\ x' = [x EXCEPT !.phase = "finished"]
Return == /\ x.phase \in {"finished", "failed"} /\ x.resultRoom
 /\ x' = [x EXCEPT !.phase = "result", !.holders = {"result"}]
Consume == /\ x.phase = "result"
 /\ x' = [x EXCEPT !.phase = "idle", !.holders = {}, !.credit = FALSE,
 !.slot = TRUE]
\* Failure owns its envelope. Source drop proves no future reads here only.
Fail == /\ x.phase \in {"validating", "validated", "queued", "waiting"}
 /\ x' = [x EXCEPT !.phase = "failed", !.sourceFinished = TRUE,
 !.holders = {"failure"}]
Request == /\ ~x.requested
 /\ x' = [x EXCEPT !.requested = TRUE, !.cancelled = TRUE]
Withdraw == /\ x.requested /\ ~x.withdrawn
 /\ x' = [x EXCEPT !.withdrawn = TRUE]
\* Death does not make credit free. No unproved reclamation action exists.
Die == /\ x.phase \in {"validating", "validated", "queued", "waiting", "rendering"}
 /\ x' = [x EXCEPT !.phase = "quarantined", !.holders = {"quarantine"}]
Busy == /\ ~x.busy /\ x' = [x EXCEPT !.busy = TRUE]
Available == /\ x.busy /\ x' = [x EXCEPT !.busy = FALSE]
HoldResult == /\ x.resultRoom /\ x' = [x EXCEPT !.resultRoom = FALSE]
DrainResult == /\ ~x.resultRoom /\ x' = [x EXCEPT !.resultRoom = TRUE]
Next == Reserve \/ Capture \/ Validate \/ Enqueue \/ Take \/ Render \/ RenderDone
 \/ Return \/ Consume \/ Fail \/ Request \/ Withdraw \/ Die \/ Busy \/ Available
 \/ HoldResult \/ DrainResult
Spec == Init /\ [][Next]_vars /\ WF_vars(Withdraw)
TypeOK == /\ x.phase \in {"idle", "validating", "validated", "queued", "waiting", "rendering", "finished", "failed", "result", "quarantined"}
 /\ x.jobs \in 0..2
 /\ x.holders \subseteq {"validator", "inbox", "renderer", "failure", "result", "quarantine"}
 /\ \A k \in {"credit", "cancelled", "requested", "withdrawn", "copied", "valid", "sourceFinished", "renderStartedCancelled", "busy", "slot", "resultRoom"}: x[k] \in BOOLEAN
UniqueOwner == Cardinality(x.holders) = IF x.phase = "idle" THEN 0 ELSE 1
CreditUntilConsumed == x.credit = (x.phase # "idle")
PrivateValidatedOnly == x.phase \in {"queued", "waiting", "rendering", "finished"}
 => x.valid /\ x.copied /\ x.sourceFinished
CancelGate == ~x.renderStartedCancelled
WithdrawalProgress == x.requested ~> x.withdrawn
=============================================================================
