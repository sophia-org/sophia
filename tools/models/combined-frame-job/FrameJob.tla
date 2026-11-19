---------------------------- MODULE FrameJob ----------------------------
EXTENDS Naturals, TLC
CONSTANT Mutation
VARIABLE x
vars == <<x>>
\* Proposed contract projection, NOT a model of an existing Rust execution.
\* All source associations and deliberate abstractions are in README.md.
Init == x = [phase |-> "idle", frame |-> 0, epoch |-> 1, jobEpoch |-> 0,
 live |-> TRUE, requested |-> FALSE, applied |-> FALSE,
 workerDone |-> TRUE, valid |-> FALSE, held |-> FALSE,
 displayed |-> 0, retired |-> {}, premature |-> FALSE, stale |-> FALSE]
\* Service 1: authority and bounded reservation; no unvalidated content install.
Admit == /\ x.phase = "idle" /\ x.frame < 2 /\ x.live /\ ~x.requested
 /\ x' = [x EXCEPT !.phase = "working", !.frame = @ + 1,
 !.jobEpoch = x.epoch, !.workerDone = FALSE, !.valid = FALSE, !.held = TRUE]
\* Combined worker returns once, with success or failure; it has no authority.
WorkerReturn(ok) == /\ x.phase = "working" /\ ~x.workerDone
 /\ x' = [x EXCEPT !.workerDone = TRUE, !.valid = ok]
\* Replacement is not withdrawal: it changes what current owner checks observe.
Replace == /\ x.epoch = 1 /\ x' = [x EXCEPT !.epoch = 2]
RequestWithdrawal == /\ ~x.requested /\ x' = [x EXCEPT !.requested = TRUE]
\* This action remains enabled even if the worker never returns.
ApplyWithdrawal == /\ x.requested /\ ~x.applied
 /\ x' = [x EXCEPT !.applied = TRUE, !.live = FALSE]
Current == x.live /\ ~x.requested /\ x.jobEpoch = x.epoch
\* Service 2 projected as the authorization linearization point only.
\* Real check / ioctl / known return are NOT collapsed by implementation claim.
Submit == /\ x.phase = "working" /\ x.workerDone /\ x.valid /\ x.held
 /\ (Current \/ Mutation = "stale")
 /\ x' = [x EXCEPT !.phase = "submitted", !.stale = ~Current]
Discard == /\ x.phase = "working" /\ x.workerDone
 /\ (~x.valid \/ ~Current)
 /\ x' = [x EXCEPT !.phase = "idle", !.held = FALSE]
\* Existing exact-completion/last-use rules are abstracted by this action.
\* It represents completion with the required last-use proof, not mere page flip.
Present == /\ x.phase = "submitted"
 /\ x' = [x EXCEPT !.phase = "idle", !.held = FALSE,
 !.retired = IF x.displayed = 0 THEN @ ELSE @ \cup {x.displayed},
 !.displayed = x.frame]
\* Mutation controls remove real modeled guards, not invariant oracle constants.
EarlyRelease == /\ Mutation = "release" /\ x.phase = "working"
 /\ ~x.workerDone /\ x.held /\ x' = [x EXCEPT !.held = FALSE]
EarlyRetire == /\ Mutation = "retire" /\ x.phase = "submitted"
 /\ x.displayed # 0 /\ ~x.premature
 /\ x' = [x EXCEPT !.retired = @ \cup {x.displayed}, !.premature = TRUE]
Next == Admit \/ WorkerReturn(TRUE) \/ WorkerReturn(FALSE) \/ Replace
 \/ RequestWithdrawal \/ ApplyWithdrawal \/ Submit \/ Discard \/ Present
 \/ EarlyRelease \/ EarlyRetire
Spec == Init /\ [][Next]_vars /\ WF_vars(ApplyWithdrawal)
TypeOK == /\ x.phase \in {"idle", "working", "submitted"}
 /\ x.frame \in 0..2 /\ x.epoch \in 1..2 /\ x.jobEpoch \in 0..2
 /\ x.displayed \in 0..2 /\ x.retired \subseteq 1..2
 /\ \A key \in {"live", "requested", "applied", "workerDone", "valid",
                "held", "premature", "stale"}: x[key] \in BOOLEAN
FreshSubmission == ~x.stale
WorkerOwnership == ~x.workerDone => x.held
SubmittedOwnership == x.phase = "submitted" => x.held
PredecessorLifetime == ~x.premature /\ x.displayed \notin x.retired
WithdrawalProgress == x.requested ~> x.applied
=============================================================================
