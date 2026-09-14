------------------ MODULE ShellPresentedContentAction ------------------
EXTENDS Naturals, Sequences

(***************************************************************************
 * The discrete-action seam for one content-shell target. A target becomes *
 * authoritative only with native presentation, and pointer capture names   *
 * that exact presented identity. Revocation or replacement invalidates the *
 * capture, but the later physical release remains consumed rather than     *
 * falling through to an application.                                      *
 *                                                                         *
 * ContentActionAck settlement is local to the shell action exchange. The   *
 * separately authorized WM activation may be admitted, refused, accepted, *
 * or rejected later; only WM admission permits an Accepted result.         *
 *************************************************************************)

CONSTANTS
    MaxEpoch, MaxGeneration,
    CaptureFromPresented,
    SuppressRevokedRelease,
    SuppressReplacedRelease,
    ConsumeSuppressedRelease,
    SettleAckAtReceipt,
    RequireWmAdmissionForAccepted

ASSUME /\ MaxEpoch \in (Nat \ {0})
       /\ MaxGeneration \in (Nat \ {0})
       /\ CaptureFromPresented \in BOOLEAN
       /\ SuppressRevokedRelease \in BOOLEAN
       /\ SuppressReplacedRelease \in BOOLEAN
       /\ ConsumeSuppressedRelease \in BOOLEAN
       /\ SettleAckAtReceipt \in BOOLEAN
       /\ RequireWmAdmissionForAccepted \in BOOLEAN

Epochs == 1..MaxEpoch
Generations == 1..MaxGeneration

NoIdentity ==
    [shell |-> 0, output |-> 0, allocation |-> 0, candidate |-> 0,
     presentation |-> 0, interaction |-> 0, target |-> 0]

ContentIdentity(epoch, generation) ==
    [shell |-> epoch, output |-> 1, allocation |-> generation,
     candidate |-> generation, presentation |-> generation,
     interaction |-> generation, target |-> generation]

Identities ==
    {NoIdentity} \cup
    {ContentIdentity(epoch, generation) :
        epoch \in Epochs, generation \in Generations}

CaptureRecords == [captured : Identities, presented : Identities]
ActionRecords ==
    [identity : Identities, presented : Identities, live : BOOLEAN,
     valid : BOOLEAN, cause : {"none", "revoked", "replaced"}]

VARIABLES
    shellEpoch, nextGeneration, prepared, presented, interactionLive,
    contactDown, capture, pressedIdentity, invalidCause,
    captureHistory, actionHistory, applicationReleases,
    pendingEvent, eventIdentity, ackReceived, ackSettled,
    wmRequested, wmAdmitted, wmTerminal, accepted

vars ==
    <<shellEpoch, nextGeneration, prepared, presented, interactionLive,
      contactDown, capture, pressedIdentity, invalidCause,
      captureHistory, actionHistory, applicationReleases,
      pendingEvent, eventIdentity, ackReceived, ackSettled,
      wmRequested, wmAdmitted, wmTerminal, accepted>>

Init ==
    /\ shellEpoch = 1
    /\ nextGeneration = 1
    /\ prepared = NoIdentity
    /\ presented = NoIdentity
    /\ interactionLive = FALSE
    /\ contactDown = FALSE
    /\ capture = NoIdentity
    /\ pressedIdentity = NoIdentity
    /\ invalidCause = "none"
    /\ captureHistory = {}
    /\ actionHistory = {}
    /\ applicationReleases = 0
    /\ pendingEvent = FALSE
    /\ eventIdentity = NoIdentity
    /\ ackReceived = FALSE
    /\ ackSettled = FALSE
    /\ wmRequested = FALSE
    /\ wmAdmitted = FALSE
    /\ wmTerminal = FALSE
    /\ accepted = FALSE

Prepare ==
    /\ nextGeneration <= MaxGeneration
    /\ prepared' = ContentIdentity(shellEpoch, nextGeneration)
    /\ nextGeneration' = nextGeneration + 1
    /\ UNCHANGED
        <<shellEpoch, presented, interactionLive, contactDown, capture,
          pressedIdentity, invalidCause, captureHistory, actionHistory,
          applicationReleases, pendingEvent, eventIdentity, ackReceived,
          ackSettled, wmRequested, wmAdmitted, wmTerminal, accepted>>

Present ==
    /\ prepared # NoIdentity
    /\ prepared # presented
    /\ presented' = prepared
    /\ interactionLive' = TRUE
    /\ capture' = NoIdentity
    /\ invalidCause' = IF contactDown THEN "replaced" ELSE "none"
    /\ UNCHANGED
        <<shellEpoch, nextGeneration, prepared, contactDown, pressedIdentity,
          captureHistory, actionHistory, applicationReleases, pendingEvent,
          eventIdentity, ackReceived, ackSettled, wmRequested, wmAdmitted,
          wmTerminal, accepted>>

Press ==
    LET selected == IF CaptureFromPresented THEN presented ELSE prepared IN
    /\ ~contactDown
    /\ interactionLive
    /\ selected # NoIdentity
    /\ contactDown' = TRUE
    /\ capture' = selected
    /\ pressedIdentity' = selected
    /\ invalidCause' = "none"
    /\ captureHistory' =
        captureHistory \cup {[captured |-> selected, presented |-> presented]}
    /\ UNCHANGED
        <<shellEpoch, nextGeneration, prepared, presented, interactionLive,
          actionHistory, applicationReleases, pendingEvent, eventIdentity,
          ackReceived, ackSettled, wmRequested, wmAdmitted, wmTerminal,
          accepted>>

RevokeInteraction ==
    /\ interactionLive
    /\ interactionLive' = FALSE
    /\ capture' = NoIdentity
    /\ invalidCause' = IF contactDown THEN "revoked" ELSE "none"
    /\ UNCHANGED
        <<shellEpoch, nextGeneration, prepared, presented, contactDown,
          pressedIdentity, captureHistory, actionHistory,
          applicationReleases, pendingEvent, eventIdentity, ackReceived,
          ackSettled, wmRequested, wmAdmitted, wmTerminal, accepted>>

ReplaceShell ==
    /\ shellEpoch < MaxEpoch
    /\ shellEpoch' = shellEpoch + 1
    /\ nextGeneration' = 1
    /\ prepared' = NoIdentity
    /\ presented' = NoIdentity
    /\ interactionLive' = FALSE
    /\ capture' = NoIdentity
    /\ invalidCause' = IF contactDown THEN "replaced" ELSE "none"
    /\ pendingEvent' = FALSE
    /\ eventIdentity' = NoIdentity
    /\ ackReceived' = FALSE
    /\ ackSettled' = FALSE
    /\ wmRequested' = FALSE
    /\ wmAdmitted' = FALSE
    /\ wmTerminal' = FALSE
    /\ accepted' = FALSE
    /\ UNCHANGED
        <<contactDown, pressedIdentity, captureHistory, actionHistory,
          applicationReleases>>

CaptureIsCurrent ==
    /\ capture # NoIdentity
    /\ capture = presented
    /\ interactionLive
    /\ capture.shell = shellEpoch

ReleaseSuppressed ==
    \/ invalidCause = "revoked" /\ SuppressRevokedRelease
    \/ invalidCause = "replaced" /\ SuppressReplacedRelease
    \/ invalidCause = "none"

Release ==
    LET valid == CaptureIsCurrent IN
    LET emit == valid \/ ~ReleaseSuppressed IN
    /\ contactDown
    /\ ~pendingEvent
    /\ contactDown' = FALSE
    /\ capture' = NoIdentity
    /\ invalidCause' = "none"
    /\ actionHistory' =
        IF emit
        THEN actionHistory \cup
             {[identity |-> pressedIdentity, presented |-> presented,
               live |-> interactionLive, valid |-> valid,
               cause |-> invalidCause]}
        ELSE actionHistory
    /\ pendingEvent' = emit
    /\ eventIdentity' = IF emit THEN pressedIdentity ELSE NoIdentity
    /\ applicationReleases' =
        IF valid \/ emit \/ ConsumeSuppressedRelease
        THEN applicationReleases
        ELSE applicationReleases + 1
    /\ UNCHANGED
        <<shellEpoch, nextGeneration, prepared, presented, interactionLive,
          pressedIdentity, captureHistory, ackReceived, ackSettled,
          wmRequested, wmAdmitted, wmTerminal, accepted>>

ReceiveShellAck ==
    /\ pendingEvent
    /\ ~ackReceived
    /\ ackReceived' = TRUE
    /\ ackSettled' = SettleAckAtReceipt
    /\ wmRequested' = TRUE
    /\ UNCHANGED
        <<shellEpoch, nextGeneration, prepared, presented, interactionLive,
          contactDown, capture, pressedIdentity, invalidCause,
          captureHistory, actionHistory, applicationReleases, pendingEvent,
          eventIdentity, wmAdmitted, wmTerminal, accepted>>

AdmitWmActivation ==
    /\ wmRequested
    /\ ~wmAdmitted
    /\ ~wmTerminal
    /\ wmAdmitted' = TRUE
    /\ UNCHANGED
        <<shellEpoch, nextGeneration, prepared, presented, interactionLive,
          contactDown, capture, pressedIdentity, invalidCause,
          captureHistory, actionHistory, applicationReleases, pendingEvent,
          eventIdentity, ackReceived, ackSettled, wmRequested, wmTerminal,
          accepted>>

RefuseWmAdmission ==
    /\ wmRequested
    /\ ~wmAdmitted
    /\ ~wmTerminal
    /\ wmTerminal' = TRUE
    /\ accepted' = FALSE
    /\ ackSettled' = IF SettleAckAtReceipt THEN ackSettled ELSE ackReceived
    /\ UNCHANGED
        <<shellEpoch, nextGeneration, prepared, presented, interactionLive,
          contactDown, capture, pressedIdentity, invalidCause,
          captureHistory, actionHistory, applicationReleases, pendingEvent,
          eventIdentity, ackReceived, wmRequested, wmAdmitted>>

FinishWmActivation(outcome) ==
    /\ wmRequested
    /\ ~wmTerminal
    /\ outcome \in {"accepted", "rejected"}
    /\ (outcome = "accepted" =>
        (wmAdmitted \/ ~RequireWmAdmissionForAccepted))
    /\ wmTerminal' = TRUE
    /\ accepted' = (outcome = "accepted")
    /\ ackSettled' = IF SettleAckAtReceipt THEN ackSettled ELSE ackReceived
    /\ UNCHANGED
        <<shellEpoch, nextGeneration, prepared, presented, interactionLive,
          contactDown, capture, pressedIdentity, invalidCause,
          captureHistory, actionHistory, applicationReleases, pendingEvent,
          eventIdentity, ackReceived, wmRequested, wmAdmitted>>

Next ==
    \/ Prepare
    \/ Present
    \/ Press
    \/ RevokeInteraction
    \/ ReplaceShell
    \/ Release
    \/ ReceiveShellAck
    \/ AdmitWmActivation
    \/ RefuseWmAdmission
    \/ \E outcome \in {"accepted", "rejected"} :
         FinishWmActivation(outcome)

Spec == Init /\ [][Next]_vars

TypeOK ==
    /\ shellEpoch \in Epochs
    /\ nextGeneration \in 1..(MaxGeneration + 1)
    /\ prepared \in Identities
    /\ presented \in Identities
    /\ interactionLive \in BOOLEAN
    /\ contactDown \in BOOLEAN
    /\ capture \in Identities
    /\ pressedIdentity \in Identities
    /\ invalidCause \in {"none", "revoked", "replaced"}
    /\ captureHistory \subseteq CaptureRecords
    /\ actionHistory \subseteq ActionRecords
    /\ applicationReleases \in 0..1
    /\ pendingEvent \in BOOLEAN
    /\ eventIdentity \in Identities
    /\ ackReceived \in BOOLEAN
    /\ ackSettled \in BOOLEAN
    /\ wmRequested \in BOOLEAN
    /\ wmAdmitted \in BOOLEAN
    /\ wmTerminal \in BOOLEAN
    /\ accepted \in BOOLEAN

CapturesNameExactPresentedContent ==
    \A record \in captureHistory :
        /\ record.presented # NoIdentity
        /\ record.captured = record.presented

ReleasedActionsNameExactPresentedContent ==
    \A record \in actionHistory :
        /\ record.valid
        /\ record.live
        /\ record.presented # NoIdentity
        /\ record.identity = record.presented

RevokedReleaseIsSuppressed ==
    \A record \in actionHistory : record.cause # "revoked"

ReplacedReleaseIsSuppressed ==
    \A record \in actionHistory : record.cause # "replaced"

NoClickThrough == applicationReleases = 0

AckIndependentFromWmOutcome == ackReceived => ackSettled

AcceptedOnlyAfterWmAdmission == accepted => wmAdmitted

=============================================================================
