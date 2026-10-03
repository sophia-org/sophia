---------------------------- MODULE SessionLock ----------------------------
EXTENDS Naturals, FiniteSets

(***************************************************************************
 * Session's lock (t034): the SessionLockState reducer                     *
 * (crates/sophia-session/src/session_lock.rs), the X frontend's requested *
 * and applied input epochs, Engine's cover proof, and a lock provider     *
 * whose connection can be replaced (t294).                                 *
 *                                                                          *
 * A lock is named by its epoch and an attempt by a session-wide serial.    *
 * The authenticator may answer any attempt it was ever given, in any       *
 * order and at any time, including attempts of earlier locks. Any lock     *
 * request voids the attempt in flight, and only the current attempt of the *
 * current lock, if it began after the latest lock request, may unlock.     *
 * Input returns to applications only after the frontend has applied the    *
 * epoch that ends the lock. A provider image is shown only when it names   *
 * the current provider connection and the current lock epoch.              *
 *                                                                          *
 * The Boolean constants are the rules; each negative-control              *
 * configuration drops one, and its invariant must then fail.                *
 *************************************************************************)

CONSTANTS MaxEpoch, MaxSerial, MaxConnection,
          VerdictChecksCurrent, RelockVoidsAttempt,
          PresentChecksCurrent, UnlockWaitsForApplied, CoveredChecksPresented

ASSUME /\ MaxEpoch \in Nat \ {0}
       /\ MaxSerial \in Nat \ {0}
       /\ MaxConnection \in Nat \ {0}
       /\ VerdictChecksCurrent \in BOOLEAN
       /\ RelockVoidsAttempt \in BOOLEAN
       /\ PresentChecksCurrent \in BOOLEAN
       /\ UnlockWaitsForApplied \in BOOLEAN
       /\ CoveredChecksPresented \in BOOLEAN

Phases == {"Unlocked", "Locking", "Locked", "Unlocking"}
Holding == {"Locking", "Locked"}
Verdicts == {"Accepted", "Rejected", "Unavailable"}
NoImage == <<0, 0>>

VARIABLES phase, epoch, lastEpoch, attempt, lastSerial, attemptEpoch,
          pending, voided, requestedInput, appliedInput, coverPresented,
          connection, lastConnection, candidates, shown, badUnlock

vars == <<phase, epoch, lastEpoch, attempt, lastSerial, attemptEpoch,
          pending, voided, requestedInput, appliedInput, coverPresented,
          connection, lastConnection, candidates, shown, badUnlock>>

providerVars == <<connection, lastConnection, candidates>>

Init ==
    /\ phase = "Unlocked"
    /\ epoch = 0
    /\ lastEpoch = 0
    /\ attempt = 0
    /\ lastSerial = 0
    /\ attemptEpoch = [s \in 1..MaxSerial |-> 0]
    /\ pending = {}
    /\ voided = {}
    /\ requestedInput = 0
    /\ appliedInput = 0
    /\ coverPresented = 0
    /\ connection = 0
    /\ lastConnection = 0
    /\ candidates = {}
    /\ shown = NoImage
    /\ badUnlock = FALSE

\* A lock request. With nothing being verified, a lock in force stays as it
\* is; otherwise a new lock begins under a fresh epoch, and the new input
\* epoch revokes every grab and lease of the desktop. Every request voids the
\* attempt in flight.
Lock ==
    LET keep == /\ phase \in Holding
                /\ (attempt = 0 \/ ~RelockVoidsAttempt)
    IN /\ \/ /\ keep
             /\ UNCHANGED <<phase, epoch, lastEpoch, attempt, requestedInput,
                            shown>>
          \/ /\ ~keep
             /\ lastEpoch < MaxEpoch
             /\ epoch' = lastEpoch + 1
             /\ lastEpoch' = lastEpoch + 1
             /\ phase' = "Locking"
             /\ attempt' = 0
             /\ requestedInput' = requestedInput + 1
             /\ shown' = NoImage
       /\ voided' = IF attempt # 0 THEN voided \cup {attempt} ELSE voided
       /\ UNCHANGED <<lastSerial, attemptEpoch, pending, appliedInput,
                      coverPresented, providerVars, badUnlock>>

\* Every head retires a frame carrying the current lock's cover.
PresentCover ==
    /\ phase # "Unlocked"
    /\ coverPresented' = epoch
    /\ UNCHANGED <<phase, epoch, lastEpoch, attempt, lastSerial, attemptEpoch,
                   pending, voided, requestedInput, appliedInput,
                   providerVars, shown, badUnlock>>

\* The X frontend clears grabs and frozen input and applies the epoch.
FrontendApply ==
    /\ appliedInput # requestedInput
    /\ appliedInput' = requestedInput
    /\ UNCHANGED <<phase, epoch, lastEpoch, attempt, lastSerial, attemptEpoch,
                   pending, voided, requestedInput, coverPresented,
                   providerVars, shown, badUnlock>>

ObserveCovered ==
    /\ phase = "Locking"
    /\ CoveredChecksPresented => coverPresented = epoch
    /\ appliedInput = requestedInput
    /\ phase' = "Locked"
    /\ UNCHANGED <<epoch, lastEpoch, attempt, lastSerial, attemptEpoch,
                   pending, voided, requestedInput, appliedInput,
                   coverPresented, providerVars, shown, badUnlock>>

\* A submitted secret opens an attempt, one at a time, while the lock holds
\* input. Serials are never reused.
BeginAttempt ==
    /\ phase \in Holding
    /\ attempt = 0
    /\ lastSerial < MaxSerial
    /\ attempt' = lastSerial + 1
    /\ lastSerial' = lastSerial + 1
    /\ attemptEpoch' = [attemptEpoch EXCEPT ![lastSerial + 1] = epoch]
    /\ pending' = pending \cup {lastSerial + 1}
    /\ UNCHANGED <<phase, epoch, lastEpoch, voided, requestedInput,
                   appliedInput, coverPresented, providerVars, shown,
                   badUnlock>>

\* The authenticator answers any attempt it was given, once.
Settle(s, v) ==
    /\ s \in pending
    /\ pending' = pending \ {s}
    /\ LET current ==
               IF VerdictChecksCurrent
                  THEN phase \in Holding /\ attempt = s /\ attemptEpoch[s] = epoch
                  ELSE phase \in Holding
       IN IF current
             THEN IF v = "Accepted"
                     THEN /\ phase' = "Unlocking"
                          /\ attempt' = 0
                          /\ requestedInput' = requestedInput + 1
                          /\ badUnlock' = (badUnlock \/ s \in voided
                                            \/ attempt # s
                                            \/ attemptEpoch[s] # epoch)
                     ELSE /\ attempt' = 0
                          /\ UNCHANGED <<phase, requestedInput, badUnlock>>
             ELSE UNCHANGED <<phase, attempt, requestedInput, badUnlock>>
    /\ UNCHANGED <<epoch, lastEpoch, lastSerial, attemptEpoch, voided,
                   appliedInput, coverPresented, providerVars, shown>>

\* The cover goes, and input returns, once the epoch that ends the lock is
\* applied.
ObserveUnlocked ==
    /\ phase = "Unlocking"
    /\ UnlockWaitsForApplied => appliedInput = requestedInput
    /\ phase' = "Unlocked"
    /\ epoch' = 0
    /\ shown' = NoImage
    /\ UNCHANGED <<lastEpoch, attempt, lastSerial, attemptEpoch, pending,
                   voided, requestedInput, appliedInput, coverPresented,
                   providerVars, badUnlock>>

\* A provider starts or is replaced: a fresh connection epoch, and the old
\* connection's image is withdrawn.
ProviderConnect ==
    /\ lastConnection < MaxConnection
    /\ connection' = lastConnection + 1
    /\ lastConnection' = lastConnection + 1
    /\ shown' = NoImage
    /\ UNCHANGED <<phase, epoch, lastEpoch, attempt, lastSerial, attemptEpoch,
                   pending, voided, requestedInput, appliedInput,
                   coverPresented, candidates, badUnlock>>

\* A provider offers a candidate for any lock epoch it has heard of, current
\* or not.
ProviderSubmit(e) ==
    /\ connection # 0
    /\ e \in 1..lastEpoch
    /\ candidates' = candidates \cup {<<connection, e>>}
    /\ UNCHANGED <<phase, epoch, lastEpoch, attempt, lastSerial, attemptEpoch,
                   pending, voided, requestedInput, appliedInput,
                   coverPresented, connection, lastConnection, shown,
                   badUnlock>>

\* Engine shows a candidate over the fill.
Present(c) ==
    /\ c \in candidates
    /\ phase \in Holding
    /\ PresentChecksCurrent => c[1] = connection /\ c[2] = epoch
    /\ shown' = c
    /\ UNCHANGED <<phase, epoch, lastEpoch, attempt, lastSerial, attemptEpoch,
                   pending, voided, requestedInput, appliedInput,
                   coverPresented, providerVars, badUnlock>>

Next ==
    \/ Lock
    \/ PresentCover
    \/ FrontendApply
    \/ ObserveCovered
    \/ BeginAttempt
    \/ \E s \in 1..MaxSerial, v \in Verdicts : Settle(s, v)
    \/ ObserveUnlocked
    \/ ProviderConnect
    \/ \E e \in 1..MaxEpoch : ProviderSubmit(e)
    \/ \E c \in candidates : Present(c)

\* Only the current attempt of the current lock, begun after the latest lock
\* request, ever unlocks.
NoStaleUnlock == ~badUnlock

\* A shown provider image names the current connection and lock epoch.
ProviderImageIsCurrent ==
    shown # NoImage =>
        /\ phase # "Unlocked"
        /\ shown[1] = connection
        /\ shown[2] = epoch

\* Input reaches applications only after the frontend applied the epoch
\* that ended the lock.
InputReturnsOnlyAfterApplied ==
    phase = "Unlocked" => appliedInput = requestedInput

\* The session is reported locked only once every head showed its cover.
LockedOnlyWhenCovered ==
    phase = "Locked" => coverPresented = epoch

\* The lock holds input exactly while some lock's cover is drawn.
CoverWhileHeld ==
    (phase # "Unlocked") <=> (epoch # 0)

FreshEpochs ==
    /\ epoch <= lastEpoch
    /\ \A s \in 1..lastSerial : attemptEpoch[s] <= lastEpoch

TypeInvariant ==
    /\ phase \in Phases
    /\ epoch \in 0..MaxEpoch
    /\ lastEpoch \in 0..MaxEpoch
    /\ attempt \in 0..MaxSerial
    /\ lastSerial \in 0..MaxSerial
    /\ attemptEpoch \in [1..MaxSerial -> 0..MaxEpoch]
    /\ pending \subseteq 1..MaxSerial
    /\ voided \subseteq 1..MaxSerial
    /\ requestedInput \in 0..(2 * MaxEpoch)
    /\ appliedInput \in 0..(2 * MaxEpoch)
    /\ coverPresented \in 0..MaxEpoch
    /\ connection \in 0..MaxConnection
    /\ lastConnection \in 0..MaxConnection
    /\ candidates \subseteq (1..MaxConnection) \X (1..MaxEpoch)
    /\ shown \in ((1..MaxConnection) \X (1..MaxEpoch)) \cup {NoImage}
    /\ badUnlock \in BOOLEAN

Spec == Init /\ [][Next]_vars

=============================================================================
