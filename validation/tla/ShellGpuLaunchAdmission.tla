---------------------- MODULE ShellGpuLaunchAdmission ----------------------
EXTENDS Naturals

(***************************************************************************
 * Startup-scoped GPU authority for one admitted shell process. A content   *
 * grant does not imply this authority: implementation support, an explicit *
 * client request, operator policy, and one current render-device identity  *
 * must all meet before launch. Device loss and policy revocation terminate  *
 * the process; a later launch receives a fresh connection/grant epoch.      *
 *************************************************************************)

CONSTANTS MaxDeviceGeneration, MaxGrantEpoch,
          RevokeOnDeviceLoss, AdvanceGrantEpoch

ASSUME /\ MaxDeviceGeneration \in Nat \ {0}
       /\ MaxGrantEpoch \in Nat \ {0}
       /\ RevokeOnDeviceLoss \in BOOLEAN
       /\ AdvanceGrantEpoch \in BOOLEAN

VARIABLES requested, allowed, deviceAvailable, deviceGeneration,
          process, connectionEpoch, grantEpoch, boundDevice,
          nextEpoch, lastIssuedEpoch, freshnessOk

vars == <<requested, allowed, deviceAvailable, deviceGeneration,
          process, connectionEpoch, grantEpoch, boundDevice,
          nextEpoch, lastIssuedEpoch, freshnessOk>>

Init ==
    /\ requested = TRUE
    /\ allowed = TRUE
    /\ deviceAvailable = TRUE
    /\ deviceGeneration = 1
    /\ process = "stopped"
    /\ connectionEpoch = 0
    /\ grantEpoch = 0
    /\ boundDevice = 0
    /\ nextEpoch = 1
    /\ lastIssuedEpoch = 0
    /\ freshnessOk = TRUE

Start ==
    /\ process = "stopped"
    /\ nextEpoch <= MaxGrantEpoch
    /\ requested
    /\ allowed
    /\ deviceAvailable
    /\ process' = "running"
    /\ connectionEpoch' = nextEpoch
    /\ grantEpoch' = nextEpoch
    /\ boundDevice' = deviceGeneration
    /\ freshnessOk' = freshnessOk /\ nextEpoch > lastIssuedEpoch
    /\ lastIssuedEpoch' = nextEpoch
    /\ nextEpoch' = IF AdvanceGrantEpoch THEN nextEpoch + 1 ELSE nextEpoch
    /\ UNCHANGED <<requested, allowed, deviceAvailable, deviceGeneration>>

Stop ==
    /\ process = "running"
    /\ process' = "stopped"
    /\ grantEpoch' = 0
    /\ boundDevice' = 0
    /\ UNCHANGED <<requested, allowed, deviceAvailable, deviceGeneration,
                   connectionEpoch, nextEpoch, lastIssuedEpoch, freshnessOk>>

LoseDevice ==
    /\ deviceAvailable
    /\ deviceGeneration < MaxDeviceGeneration
    /\ deviceAvailable' = FALSE
    /\ deviceGeneration' = deviceGeneration + 1
    /\ IF RevokeOnDeviceLoss
          THEN /\ process' = "stopped"
               /\ grantEpoch' = 0
               /\ boundDevice' = 0
          ELSE /\ UNCHANGED <<process, grantEpoch, boundDevice>>
    /\ UNCHANGED <<requested, allowed, connectionEpoch, nextEpoch,
                   lastIssuedEpoch, freshnessOk>>

RestoreDevice ==
    /\ ~deviceAvailable
    /\ deviceAvailable' = TRUE
    /\ UNCHANGED <<requested, allowed, deviceGeneration, process,
                   connectionEpoch, grantEpoch, boundDevice, nextEpoch,
                   lastIssuedEpoch, freshnessOk>>

RevokePolicy ==
    /\ allowed
    /\ allowed' = FALSE
    /\ process' = "stopped"
    /\ grantEpoch' = 0
    /\ boundDevice' = 0
    /\ UNCHANGED <<requested, deviceAvailable, deviceGeneration,
                   connectionEpoch, nextEpoch, lastIssuedEpoch, freshnessOk>>

GrantPolicy ==
    /\ ~allowed
    /\ allowed' = TRUE
    /\ UNCHANGED <<requested, deviceAvailable, deviceGeneration, process,
                   connectionEpoch, grantEpoch, boundDevice, nextEpoch,
                   lastIssuedEpoch, freshnessOk>>

WithdrawRequest ==
    /\ requested
    /\ requested' = FALSE
    /\ process' = "stopped"
    /\ grantEpoch' = 0
    /\ boundDevice' = 0
    /\ UNCHANGED <<allowed, deviceAvailable, deviceGeneration,
                   connectionEpoch, nextEpoch, lastIssuedEpoch, freshnessOk>>

Request ==
    /\ ~requested
    /\ requested' = TRUE
    /\ UNCHANGED <<allowed, deviceAvailable, deviceGeneration, process,
                   connectionEpoch, grantEpoch, boundDevice, nextEpoch,
                   lastIssuedEpoch, freshnessOk>>

Next == Start \/ Stop \/ LoseDevice \/ RestoreDevice \/
        RevokePolicy \/ GrantPolicy \/ WithdrawRequest \/ Request

NoAmbientGrant ==
    (process = "running") \/ (grantEpoch = 0 /\ boundDevice = 0)

ActiveGrantNamesCurrentAuthority ==
    process = "running" =>
        requested /\ allowed /\ deviceAvailable /\
        grantEpoch = connectionEpoch /\ grantEpoch # 0 /\
        boundDevice = deviceGeneration

FreshGrantEpoch == freshnessOk /\ nextEpoch > lastIssuedEpoch

TypeInvariant ==
    /\ requested \in BOOLEAN
    /\ allowed \in BOOLEAN
    /\ deviceAvailable \in BOOLEAN
    /\ deviceGeneration \in 1..MaxDeviceGeneration
    /\ process \in {"stopped", "running"}
    /\ connectionEpoch \in 0..MaxGrantEpoch
    /\ grantEpoch \in 0..MaxGrantEpoch
    /\ boundDevice \in 0..MaxDeviceGeneration
    /\ nextEpoch \in 1..(MaxGrantEpoch + 1)
    /\ lastIssuedEpoch \in 0..MaxGrantEpoch
    /\ freshnessOk \in BOOLEAN

Spec == Init /\ [][Next]_vars

=============================================================================
