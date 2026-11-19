---------------------------- MODULE Fallback ----------------------------
EXTENDS Naturals
CONSTANT Mutation
VARIABLE x
vars == <<x>>
\* Proposed owner decision only. Source and abstractions: FALLBACK.md F1-F6.
Init == x = [phase |-> "result", epoch |-> 1, jobEpoch |-> 1, live |-> TRUE,
             description |-> TRUE, scene |-> 0, plannedScene |-> 0,
             entries |-> 0, entryEpoch |-> 0, entryLive |-> FALSE, entryDescription |-> FALSE,
             installAttempted |-> FALSE, installEpoch |-> 0, installLive |-> FALSE, installDescription |-> FALSE,
             content |-> 0, claim |-> 0, installed |-> FALSE, deferred |-> FALSE, prepared |-> FALSE,
             lease |-> "result", releaseAck |-> FALSE, backing |-> "result"]
Eligible == x.live /\ x.epoch = x.jobEpoch /\ x.description
Churn == /\ x.scene < 2 /\ x' = [x EXCEPT !.scene = @ + 1]
Replace == /\ x.epoch = 1 /\ x' = [x EXCEPT !.epoch = 2]
Revoke == /\ x.live /\ x' = [x EXCEPT !.live = FALSE]
Mismatch == /\ x.description /\ x' = [x EXCEPT !.description = FALSE]
\* New routing gate; repeat current checks at the real conventional entry below.
Route == /\ x.phase = "result" /\ x.scene # x.plannedScene
         /\ (Eligible \/ Mutation = "eligibility")
         /\ x' = [x EXCEPT !.phase = "fallback", !.entries = @ + 1,
                  !.entryEpoch = x.epoch, !.entryLive = x.live, !.entryDescription = x.description,
                  !.lease = "release"]
\* Fast exit delegates to the separately specified install-already-rendered path.
Fast == /\ x.phase = "result" /\ Eligible /\ x.scene = x.plannedScene
        /\ x' = [x EXCEPT !.phase = "fast"]
Refuse == /\ x.phase \in {"result", "fallback"} /\ ~Eligible
          /\ x' = [x EXCEPT !.phase = "refused", !.lease = IF @ = "result" THEN "release" ELSE @, !.backing = "retained_failure"]
\* worker.rs flush_discarded_release: Full retains; ack abstracts actual disposition.
ReleaseAck == /\ x.lease = "release"
              /\ x' = [x EXCEPT !.lease = "freed", !.releaseAck = TRUE]
LoseRelease == /\ Mutation = "release" /\ x.lease = "release"
               /\ x' = [x EXCEPT !.lease = "freed"]
\* set_shell_component_content_on_target restores content and claim on queue Err.
\* Success, including deferred Ok(false) from queue_retained_projection, is NOT Present.
Install(ok, deferred) ==
    /\ x.phase = "fallback" /\ (Eligible \/ Mutation = "fresh")
    /\ x' = [x EXCEPT !.phase = IF ok THEN "ordinary" ELSE "queue_error",
             !.installAttempted = TRUE, !.installEpoch = x.epoch, !.installLive = x.live,
             !.installDescription = x.description, !.installed = ok, !.deferred = ok /\ deferred,
             !.content = IF ok \/ Mutation = "rollback" THEN 1 ELSE 0,
             !.claim = IF ok \/ Mutation = "rollback" THEN 1 ELSE 0,
             !.backing = IF ok THEN "ordinary" ELSE "retained_failure"]
\* submit_bundle emits Prepared only after successful setter return, even if deferred.
Prepared == /\ (x.phase = "ordinary" \/ (Mutation = "prepared" /\ x.phase = "queue_error"))
            /\ ~x.prepared /\ x' = [x EXCEPT !.prepared = TRUE]
\* Deliberately incorrect recursive fallback; normal Next has no such route.
Retry == /\ Mutation = "retry" /\ x.phase \in {"ordinary", "queue_error"}
         /\ x' = [x EXCEPT !.phase = "fallback", !.entries = @ + 1]
Next == Churn \/ Replace \/ Revoke \/ Mismatch \/ Route \/ Fast \/ Refuse
        \/ ReleaseAck \/ LoseRelease \/ Install(TRUE, TRUE) \/ Install(TRUE, FALSE)
        \/ Install(FALSE, FALSE) \/ Prepared \/ Retry
Spec == Init /\ [][Next]_vars
TypeOK == /\ x.phase \in {"result", "fallback", "fast", "refused", "ordinary", "queue_error"}
          /\ x.epoch \in 1..2 /\ x.jobEpoch = 1 /\ x.scene \in 0..2 /\ x.plannedScene = 0
          /\ x.entries \in 0..2 /\ x.entryEpoch \in 0..2 /\ x.installEpoch \in 0..2
          /\ x.content \in 0..1 /\ x.claim \in 0..1
          /\ x.lease \in {"result", "release", "freed"}
          /\ x.backing \in {"result", "ordinary", "retained_failure"}
          /\ \A k \in {"live", "description", "entryLive", "entryDescription", "installAttempted", "installLive", "installDescription", "installed", "deferred", "prepared", "releaseAck"}: x[k] \in BOOLEAN
EligibleFallback == x.entries > 0 => (x.entryEpoch = x.jobEpoch /\ x.entryLive /\ x.entryDescription)
OneFallbackEntry == x.entries <= 1
RollbackPair == x.phase = "queue_error" => (x.content = 0 /\ x.claim = 0)
LeaseHeld == x.lease = "freed" => x.releaseAck
PreparedAfterInstall == x.prepared => x.installed
FreshInstall == x.installAttempted => (x.installEpoch = x.jobEpoch /\ x.installLive /\ x.installDescription)
=============================================================================
