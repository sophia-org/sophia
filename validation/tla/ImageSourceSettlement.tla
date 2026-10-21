----------------------- MODULE ImageSourceSettlement -----------------------
EXTENDS Naturals, Sequences, FiniteSets

(***************************************************************************
 * Bounded model of image-source settlement: which ResourceStatus may spend *
 * an admitted offer's held terminal-status reservation, and when the       *
 * source (sealed: SourceReleased; ring: SlotReleased) may be released.     *
 *                                                                         *
 * Two offers, one grant, at most one channel loss. Each offer may receive  *
 * at most one client ResourceCancel and one ResourceRetire, under a       *
 * client-chosen transaction: the offer's own id or another id. Pixels,    *
 * budgets, credits and allocation are modeled elsewhere or in Rust tests. *
 *                                                                         *
 * Source mapping (sophia-d4-design, 2026-10-21):                           *
 *  Offer/Admit/RefusePending: export image_offers.rs or owned_ring.rs     *
 *    write_owned_ring; owned_copy/admission.rs propose_offer;             *
 *    owned_refuse_offer is the direct pre-read status.                    *
 *  StartRead/FinishRead: isb_worker read ticket held, then dropped.        *
 *  Cancel: content_resources.rs -> store.cancel -> abort: first stop wins, *
 *    keeping the cancel's own transaction (resources.rs abort).           *
 *  Expire: stop_isb keeps transfer.transaction (the offer id).            *
 *  Retire on an open transfer: store.retire Err(Stale), no store event;   *
 *    content_resources.rs synthesises a status-3 reply under the retire's *
 *    transaction (a non-final, unflagged record).                         *
 *  Settle: complete_isb / abort_finished push the transfer's single final *
 *    ResourceStatus into the store event queue (flagged by                *
 *    flush_content_resource_events for both source types).                *
 *  AppendStatus: FIFO head reaches the export. MatchMode selects how the  *
 *    export decides that a status spends the offer's held status:          *
 *      Provenance: only a flagged store settlement (B and sealed).         *
 *      TransactionKeyed: transaction == offer id, offer still live        *
 *        (sealed append_image_status before the fix); a spent hold makes  *
 *        `offer.status.ok_or(EINVAL)` fault.                              *
 *      ResourceKeyed: any status for the resource while owed (the review- *
 *        03 interim B matcher).                                           *
 *  Release: image_owner / OwnedCopyOwner publish the release only after   *
 *    settlement and once the held status is spent (EAGAIN otherwise), and *
 *    only for the current identity (journal).                             *
 *  Lose: channel/journal loss; reset releases every held reservation and  *
 *    clears the outbound FIFO. Old transfers still settle; their events   *
 *    go nowhere.                                                          *
 ***************************************************************************)

CONSTANTS Ring, MatchMode, ClientReusesOfferId, LossAllowed,
          ReleaseBeforeRead, ReleaseIgnoresEpoch

Provenance == 0
TransactionKeyed == 1
ResourceKeyed == 2

Offers == {1, 2}
States == {"none", "pending", "open", "reading", "read", "settled"}
StopTxs == {"none", "offer", "other"}
ClientTxs == IF ClientReusesOfferId THEN {"offer", "other"} ELSE {"other"}
Transfer == {"open", "reading", "read"}

VARIABLES st, stopTx, hold, accepted, released, cancelled, retired,
          offEpoch, epoch, queue, journal, fault, writeDuringRead
vars == <<st, stopTx, hold, accepted, released, cancelled, retired,
          offEpoch, epoch, queue, journal, fault, writeDuringRead>>

Current(o) == offEpoch[o] = epoch
SlotFree == \A p \in Offers : st[p] = "none" \/ released[p] \/ ~Current(p)
Entry(k, o, tx, final) ==
    [k |-> k, o |-> o, tx |-> tx, final |-> final, ep |-> epoch]

Init ==
    /\ st = [o \in Offers |-> "none"]
    /\ stopTx = [o \in Offers |-> "none"]
    /\ hold = [o \in Offers |-> FALSE]
    /\ accepted = [o \in Offers |-> FALSE]
    /\ released = [o \in Offers |-> FALSE]
    /\ cancelled = [o \in Offers |-> FALSE]
    /\ retired = [o \in Offers |-> FALSE]
    /\ offEpoch = [o \in Offers |-> 0]
    /\ epoch = 1
    /\ queue = <<>>
    /\ journal = <<>>
    /\ fault = FALSE
    /\ writeDuringRead = FALSE

\* Client proposes an offer. The terminal-status and release reservations
\* are held from here. A ring client fills its slot first, and may do so
\* only when every earlier offer on that ring has been released.
Offer(o) ==
    /\ st[o] = "none"
    /\ o = 2 => st[1] # "none"
    /\ Ring => SlotFree
    /\ st' = [st EXCEPT ![o] = "pending"]
    /\ hold' = [hold EXCEPT ![o] = TRUE]
    /\ offEpoch' = [offEpoch EXCEPT ![o] = epoch]
    /\ writeDuringRead' = (writeDuringRead
           \/ (Ring /\ \E p \in Offers : st[p] = "reading" /\ Current(p)))
    /\ UNCHANGED <<stopTx, accepted, released, cancelled, retired, epoch,
                   queue, journal, fault>>

Admit(o) ==
    /\ st[o] = "pending" /\ Current(o)
    /\ st' = [st EXCEPT ![o] = "open"]
    /\ UNCHANGED <<stopTx, hold, accepted, released, cancelled, retired,
                   offEpoch, epoch, queue, journal, fault, writeDuringRead>>

\* Direct pre-read refusal: the owner's own helper writes the final status
\* under the offer id and spends the hold itself.
RefusePending(o) ==
    /\ st[o] = "pending" /\ Current(o)
    /\ st' = [st EXCEPT ![o] = "settled"]
    /\ hold' = [hold EXCEPT ![o] = FALSE]
    /\ journal' = Append(journal, Entry("status", o, "offer", TRUE))
    /\ UNCHANGED <<stopTx, accepted, released, cancelled, retired,
                   offEpoch, epoch, queue, fault, writeDuringRead>>

StartRead(o) ==
    /\ st[o] = "open" /\ stopTx[o] = "none"
    /\ st' = [st EXCEPT ![o] = "reading"]
    /\ UNCHANGED <<stopTx, hold, accepted, released, cancelled, retired,
                   offEpoch, epoch, queue, journal, fault, writeDuringRead>>

FinishRead(o) ==
    /\ st[o] = "reading"
    /\ st' = [st EXCEPT ![o] = "read"]
    /\ UNCHANGED <<stopTx, hold, accepted, released, cancelled, retired,
                   offEpoch, epoch, queue, journal, fault, writeDuringRead>>

Expire(o) ==
    /\ st[o] \in Transfer /\ stopTx[o] = "none"
    /\ stopTx' = [stopTx EXCEPT ![o] = "offer"]
    /\ UNCHANGED <<st, hold, accepted, released, cancelled, retired,
                   offEpoch, epoch, queue, journal, fault, writeDuringRead>>

\* The store's single final status for this transfer, under the offer id
\* unless a stop recorded another transaction first.
Settle(o) ==
    /\ \/ st[o] = "read"
       \/ st[o] = "open" /\ stopTx[o] # "none"
    /\ st' = [st EXCEPT ![o] = "settled"]
    /\ accepted' = [accepted EXCEPT ![o] = (stopTx[o] = "none")]
    /\ queue' = IF Current(o)
                   THEN Append(queue, [o |-> o, final |-> TRUE,
                            tx |-> IF stopTx[o] = "none" THEN "offer"
                                   ELSE stopTx[o]])
                   ELSE queue
    /\ UNCHANGED <<stopTx, hold, released, cancelled, retired, offEpoch,
                   epoch, journal, fault, writeDuringRead>>

\* An open transfer records the first stop; a settled one draws a Stale
\* reply under the cancel's transaction. Pending cancels are held inbound.
Cancel(o, tx) ==
    /\ ~cancelled[o] /\ Current(o)
    /\ st[o] \in Transfer \cup {"settled"}
    /\ cancelled' = [cancelled EXCEPT ![o] = TRUE]
    /\ IF st[o] \in Transfer
          THEN /\ stopTx' = [stopTx EXCEPT ![o] =
                       IF stopTx[o] = "none" THEN tx ELSE stopTx[o]]
               /\ queue' = queue
          ELSE /\ stopTx' = stopTx
               /\ queue' = Append(queue, [o |-> o, tx |-> tx, final |-> FALSE])
    /\ UNCHANGED <<st, hold, accepted, released, retired, offEpoch, epoch,
                   journal, fault, writeDuringRead>>

\* Retire of anything not yet accepted is refused with a synthetic reply.
Retire(o, tx) ==
    /\ ~retired[o] /\ Current(o)
    /\ st[o] \in {"pending"} \cup Transfer \cup {"settled"}
    /\ ~(st[o] = "settled" /\ accepted[o])
    /\ retired' = [retired EXCEPT ![o] = TRUE]
    /\ queue' = Append(queue, [o |-> o, tx |-> tx, final |-> FALSE])
    /\ UNCHANGED <<st, stopTx, hold, accepted, released, cancelled,
                   offEpoch, epoch, journal, fault, writeDuringRead>>

Spends(r) ==
    CASE MatchMode = Provenance -> r.final /\ hold[r.o]
      [] MatchMode = ResourceKeyed -> hold[r.o]
      [] OTHER -> r.tx = "offer" /\ hold[r.o]

AppendFault(r) ==
    /\ MatchMode = TransactionKeyed
    /\ r.tx = "offer" /\ ~hold[r.o] /\ ~released[r.o]

AppendStatus ==
    /\ queue # <<>> /\ ~fault
    /\ LET r == Head(queue) IN
         IF AppendFault(r)
            THEN /\ fault' = TRUE
                 /\ UNCHANGED <<queue, hold, journal>>
            ELSE /\ fault' = FALSE
                 /\ queue' = Tail(queue)
                 /\ hold' = IF Spends(r) THEN [hold EXCEPT ![r.o] = FALSE]
                            ELSE hold
                 /\ journal' = Append(journal, Entry("status", r.o, r.tx, r.final))
    /\ UNCHANGED <<st, stopTx, accepted, released, cancelled, retired,
                   offEpoch, epoch, writeDuringRead>>

Release(o) ==
    /\ st[o] # "none" /\ ~released[o]
    /\ Current(o) \/ ReleaseIgnoresEpoch
    /\ IF ReleaseBeforeRead
          THEN st[o] \in {"reading", "read", "settled"}
          ELSE st[o] = "settled" /\ ~hold[o]
    /\ released' = [released EXCEPT ![o] = TRUE]
    /\ journal' = Append(journal, Entry("release", o, "offer", FALSE))
    /\ UNCHANGED <<st, stopTx, hold, accepted, cancelled, retired, offEpoch,
                   epoch, queue, fault, writeDuringRead>>

Lose ==
    /\ LossAllowed /\ epoch = 1
    /\ epoch' = 2
    /\ queue' = <<>>
    /\ hold' = [o \in Offers |-> FALSE]
    /\ UNCHANGED <<st, stopTx, accepted, released, cancelled, retired,
                   offEpoch, journal, fault, writeDuringRead>>

Next ==
    \/ \E o \in Offers :
          \/ Offer(o) \/ Admit(o) \/ RefusePending(o)
          \/ StartRead(o) \/ FinishRead(o) \/ Expire(o) \/ Settle(o)
          \/ Release(o)
          \/ \E tx \in ClientTxs : Cancel(o, tx) \/ Retire(o, tx)
    \/ AppendStatus
    \/ Lose

Fairness ==
    /\ \A o \in Offers :
          /\ WF_vars(Admit(o) \/ RefusePending(o))
          /\ WF_vars(StartRead(o))
          /\ WF_vars(FinishRead(o))
          /\ WF_vars(Settle(o))
          /\ WF_vars(Release(o))
    /\ WF_vars(AppendStatus)

Spec == Init /\ [][Next]_vars /\ Fairness

---------------------------------------------------------------------------
Entries == {journal[i] : i \in 1..Len(journal)}

TypeOK ==
    /\ st \in [Offers -> States]
    /\ stopTx \in [Offers -> StopTxs]
    /\ hold \in [Offers -> BOOLEAN]
    /\ accepted \in [Offers -> BOOLEAN]
    /\ released \in [Offers -> BOOLEAN]
    /\ cancelled \in [Offers -> BOOLEAN]
    /\ retired \in [Offers -> BOOLEAN]
    /\ offEpoch \in [Offers -> {0, 1, 2}]
    /\ epoch \in {1, 2}
    /\ fault \in BOOLEAN
    /\ writeDuringRead \in BOOLEAN

\* The export never faults on a legitimate final status.
NoAppendFault == ~fault

\* A release follows that offer's own final status in the same journal.
StatusBeforeRelease ==
    \A i \in 1..Len(journal) :
        journal[i].k = "release" =>
            \E j \in 1..(i - 1) :
                /\ journal[j].k = "status" /\ journal[j].final
                /\ journal[j].o = journal[i].o /\ journal[j].ep = journal[i].ep

\* Exactly-once settlement (at most once here; liveness gives at least once).
OneFinalPerOffer ==
    \A o \in Offers :
        Cardinality({i \in 1..Len(journal) :
            journal[i].k = "status" /\ journal[i].final /\ journal[i].o = o}) <= 1
OneReleasePerOffer ==
    \A o \in Offers :
        Cardinality({i \in 1..Len(journal) :
            journal[i].k = "release" /\ journal[i].o = o}) <= 1

\* A release never lands on a replacement connection's journal.
ReleaseInOwnEpoch ==
    \A e \in Entries : e.k = "release" => e.ep = offEpoch[e.o]

\* Ring: an honest client never writes a slot Sophia is still reading.
NoWriteDuringRead == ~writeDuringRead

\* Every offer made on a live journal is eventually released, unless that
\* journal is lost first.
EveryOfferReleased ==
    \A o \in Offers :
        (st[o] # "none" /\ Current(o)) ~> (released[o] \/ ~Current(o))
=============================================================================
