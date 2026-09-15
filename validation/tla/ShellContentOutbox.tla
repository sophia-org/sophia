------------------------ MODULE ShellContentOutbox ------------------------
EXTENDS Naturals, Sequences, FiniteSets

(***************************************************************************
 * Bounded ownership abstraction of the single-owner content outbox.          *
 * Two output identities, one grant, fixed control envelopes and one bulk    *
 * frame. Pixels/driver progress and WM admission live in separate models.   *
 *                                                                         *
 * Source mapping (2026-09-14 working implementation):                       *
 * Accept: shell_transport/content_candidates.rs grant_content_permit;       *
 * Native: content_prepared/content_presented + Session pending custody.     *
 * Transfer: prepare_content_frame, prevalidated producer, ShellOutbox::push,*
 * then infallible producer pop_front. Allocation failure BEFORE push is a  *
 * stutter; no fallible step follows ownership transfer. Process loss is     *
 * Disconnect, never producer replay.                                      *
 * Write: shell_transport/outbox.rs written; charge includes full frame      *
 * through the last byte. Actions: content_actions.rs two-credit admission. *
 * Release: real-consumer retirement; holding it cannot gate action enqueue.*
 * The actual rendering/reference graph is tested separately, not modeled  *
 * as satisfied merely by this nondeterministic external completion.         *
 *************************************************************************)

CONSTANTS DuplicateTransfer, ReleaseCreditEarly, BypassPresented,
          IgnoreStoreCredit, ActionWaitsForRelease
Outputs == {1, 2}
P(i) == <<"presented", i>>
R(i) == <<"released", i>>
A(i) == <<"action", i>>
C(i) == <<"cancel", i>>
Bulk == <<"bulk", 0>>
Records == {Bulk} \cup {P(i): i \in Outputs} \cup {R(i): i \in Outputs}
           \cup {A(i): i \in Outputs} \cup {C(i): i \in Outputs}
Weight(r) == IF r = Bulk THEN 3 ELSE 2
Bytes(set) == 2 * Cardinality(set \ {Bulk}) + IF Bulk \in set THEN 3 ELSE 0
Queued(q) == {q[n] : n \in 1..Len(q)}

VARIABLES live, accepted, native, held, acted, store, fifo, offset,
          charged, sent, lost, bulkUsed
vars == <<live, accepted, native, held, acted, store, fifo, offset,
          charged, sent, lost, bulkUsed>>

Fits(extra) == Cardinality(charged \cup extra) <= 5 /\ Bytes(charged \cup extra) <= 12
Published(i) == P(i) \in (Queued(fifo) \cup Queued(sent))

Init == /\ live = TRUE /\ accepted = {} /\ native = {} /\ held = {}
        /\ acted = {} /\ store = {} /\ fifo = <<>> /\ offset = 0
        /\ charged = {} /\ sent = <<>> /\ lost = {} /\ bulkUsed = FALSE

Accept(i) ==
    /\ live /\ i \notin accepted /\ Fits({P(i), R(i)})
    /\ accepted' = accepted \cup {i}
    /\ held' = held \cup {i}
    /\ store' = store \cup {P(i), R(i)}
    /\ charged' = charged \cup {P(i), R(i)}
    /\ UNCHANGED <<live, native, acted, fifo, offset, sent, lost, bulkUsed>>

Native(i) ==
    /\ live /\ i \in accepted \ native
    /\ native' = native \cup {i}
    /\ UNCHANGED <<live, accepted, held, acted, store, fifo, offset, charged, sent, lost, bulkUsed>>

ConsumerRetires(i) ==
    /\ i \in native \cap held
    /\ held' = held \ {i}
    /\ UNCHANGED <<live, accepted, native, acted, store, fifo, offset, charged, sent, lost, bulkUsed>>

AdmitAction(i) ==
    /\ live /\ i \in native \ acted
    /\ Published(i) \/ BypassPresented
    /\ ~ActionWaitsForRelease \/ i \notin held
    /\ Fits({A(i), C(i)})
    /\ acted' = acted \cup {i}
    /\ store' = store \cup {A(i), C(i)}
    /\ charged' = charged \cup {A(i), C(i)}
    /\ UNCHANGED <<live, accepted, native, held, fifo, offset, sent, lost, bulkUsed>>

Ready(r) ==
    \/ \E i \in Outputs : r = P(i) /\ i \in native
    \/ \E i \in Outputs : r = R(i) /\ i \in native \ held
    \/ \E i \in Outputs : r = A(i) /\ (Published(i) \/ BypassPresented)
    \/ \E i \in Outputs : r = C(i) /\ A(i) \in (Queued(fifo) \cup Queued(sent))

Transfer(r) ==
    /\ live /\ r \in store /\ Ready(r)
    /\ store' = IF DuplicateTransfer THEN store ELSE store \ {r}
    /\ fifo' = Append(fifo, r)
    /\ UNCHANGED <<live, accepted, native, held, acted, offset, charged, sent, lost, bulkUsed>>

EnqueueBulk ==
    /\ live /\ ~bulkUsed
    /\ IF IgnoreStoreCredit
          THEN Cardinality(Queued(fifo) \cup {Bulk}) <= 5 /\ Bytes(Queued(fifo) \cup {Bulk}) <= 12
          ELSE Fits({Bulk})
    /\ fifo' = Append(fifo, Bulk)
    /\ charged' = charged \cup {Bulk}
    /\ bulkUsed' = TRUE
    /\ UNCHANGED <<live, accepted, native, held, acted, store, offset, sent, lost>>

Write ==
    /\ live /\ Len(fifo) > 0
    /\ IF offset = 0
          THEN /\ offset' = 1 /\ fifo' = fifo /\ sent' = sent
               /\ charged' = IF ReleaseCreditEarly THEN charged \ {Head(fifo)} ELSE charged
          ELSE /\ offset' = 0 /\ fifo' = Tail(fifo)
               /\ sent' = Append(sent, Head(fifo))
               /\ charged' = charged \ {Head(fifo)}
    /\ UNCHANGED <<live, accepted, native, held, acted, store, lost, bulkUsed>>

Disconnect ==
    /\ live /\ live' = FALSE
    /\ lost' = lost \cup store \cup Queued(fifo)
    /\ store' = {} /\ fifo' = <<>> /\ offset' = 0 /\ charged' = {}
    /\ UNCHANGED <<accepted, native, held, acted, sent, bulkUsed>>

Next == (\E i \in Outputs : Accept(i) \/ Native(i) \/ ConsumerRetires(i) \/ AdmitAction(i))
        \/ (\E r \in Records : Transfer(r)) \/ EnqueueBulk \/ Write \/ Disconnect
Spec == Init /\ [][Next]_vars

TypeOK == /\ live \in BOOLEAN /\ accepted \subseteq Outputs /\ native \subseteq accepted
          /\ held \subseteq accepted /\ acted \subseteq native
          /\ store \subseteq Records /\ charged \subseteq Records /\ lost \subseteq Records
          /\ Queued(fifo) \subseteq Records /\ Queued(sent) \subseteq Records
          /\ offset \in 0..1 /\ bulkUsed \in BOOLEAN
OneCustodian == /\ store \cap Queued(fifo) = {}
                /\ Len(fifo) = Cardinality(Queued(fifo))
                /\ (store \cup Queued(fifo)) \cap Queued(sent) = {}
ChargedUntilLastByte == charged = store \cup Queued(fifo)
AggregateBudget == Cardinality(charged) <= 5 /\ Bytes(charged) <= 12
NoTerminalReplay == Len(sent) = Cardinality(Queued(sent))
NoObligationLost ==
    \A i \in accepted : {P(i), R(i)} \subseteq (store \cup Queued(fifo) \cup Queued(sent) \cup lost)
PresentedBeforeAction ==
    \A n \in 1..Len(sent) : \A i \in Outputs : sent[n] = A(i) =>
        \E m \in 1..(n-1) : sent[m] = P(i)
ReleaseDoesNotGateAction ==
    \A i \in native \ acted :
        (live /\ Published(i) /\ Fits({A(i), C(i)})) => ENABLED AdmitAction(i)
=============================================================================
