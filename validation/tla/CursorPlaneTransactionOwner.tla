------------------- MODULE CursorPlaneTransactionOwner -------------------
EXTENDS Naturals, FiniteSets

(***************************************************************************
 * One owner deciding what goes into a CRTC's next atomic commit.           *
 *                                                                          *
 * Sophia drives primary planes atomically and the cursor through the       *
 * legacy ioctl, which is why the two never contend: an ioctl can move a    *
 * cursor while a page flip is outstanding, and archive 0004 counted that   *
 * happening fifteen times. An atomic commit cannot. The kernel serializes  *
 * commits per CRTC, so bringing the cursor into the request means the      *
 * cursor joins the queue the primary already waits in.                     *
 *                                                                          *
 * That is the whole subject here: what happens to a cursor move that       *
 * arrives at a busy CRTC. It must not be lost, must not wait for a client  *
 * that may not draw again for most of a second, and must not accumulate    *
 * one commit per pointer event.                                            *
 *                                                                          *
 * A cursor-only commit is taken only while the client is quiet. The real   *
 * commit blocks until the kernel applies it at a vblank, so one issued      *
 * between a frame retiring and the next arriving spends the vblank that     *
 * frame needed; a measured run paid 234 of them for a fifth of its frame    *
 * rate. A drawing client has a frame along within a refresh and that frame  *
 * carries the cursor for nothing, so the position waits for it. `quiet`     *
 * carries that, and `Quiesce` is a client falling silent -- which is what   *
 * keeps the cursor from freezing on a desktop nobody is drawing to.         *
 *                                                                          *
 * The model also has to keep two things a working system already has. A    *
 * cursor commit must not disturb a directly scanned client buffer -- the   *
 * primary keeps scanning what it was scanning. The eligibility episode     *
 * itself stays with `PresentFlipOwnership`, which owns it; nothing here    *
 * advances one, and an invariant over a variable this model never changes  *
 * would pass without meaning anything. And a refused                       *
 * combined commit must never cost the frame: the retry drops the cursor,   *
 * not the primary.                                                         *
 *                                                                          *
 * Heads of one card session share the request, which is what mirroring is: *
 * one commit carries every head's contribution. A cursor is not on every    *
 * head at once, so a commit also *hides* it on the heads it left -- which   *
 * is an ioctl today and a property in the request tomorrow, and either way  *
 * must not disturb what those heads are scanning.                           *
 *                                                                          *
 * Deliberately not modelled: cursor image content, hotspot, formats and    *
 * sizes (a startup capability probe answers those once, the way the atomic *
 * test answers format questions for direct scanout); cursor framebuffer    *
 * allocation, which is the existing resource-bundle discipline rather than *
 * a new temporal property; mirror cohort pacing, which `MirrorHeadPacing`  *
 * owns; and every duration.                                                *
 *************************************************************************)

CONSTANTS Heads, MaxMoves, MaxFrames

ASSUME Heads # {}
ASSUME MaxMoves \in Nat /\ MaxMoves >= 1
ASSUME MaxFrames \in Nat /\ MaxFrames >= 1

(***************************************************************************
 * Which heads the cursor projects onto at a given position. Modelled as a  *
 * function of the position rather than as free choice, because the real    *
 * projection is deterministic given the pointer and the head layouts       *
 * (`project_mirror_coordinates`). Alternating parity is enough to make     *
 * every head both covered and uncovered across a run without inventing     *
 * geometry the model has no business having.                               *
 *************************************************************************)
CoveredBy(position) ==
    IF position % 2 = 1 THEN Heads ELSE {h \in Heads : h = CHOOSE any \in Heads : TRUE}

(***************************************************************************
 * outstanding  : which commit kind the CRTC is currently busy with, if     *
 *                any. "none" means the CRTC is free. The kernel allows one *
 *                at a time and this is that one.                           *
 * pendingCursor: the newest cursor position not yet committed, or 0. A     *
 *                cell rather than a queue: a newer move overwrites an      *
 *                uncommitted one, because a backlog that grows per pointer *
 *                event is unbounded by construction.                       *
 * committed    : per head, the cursor position that head is showing, or 0  *
 *                when the cursor is not on it. A head the cursor left is   *
 *                hidden by the same commit that moves it elsewhere.        *
 * pendingFrame : a client frame waiting to be committed.                   *
 * scanned      : per head, which client buffer that head's primary plane   *
 *                is scanning. This is what a cursor commit must leave      *
 *                alone -- on every head, including the ones it hides on.   *
 * doubleCommit : set if a commit is ever issued while one is outstanding.  *
 *                The single `outstanding` variable makes that structurally *
 *                impossible, so stating it as an invariant over that       *
 *                variable would pass without meaning anything; each commit *
 *                re-evaluates the guard instead, the way PresentFlipOwner- *
 *                ship records a bad flip.                                  *
 * lostFrame    : set if a frame is ever dropped because of the cursor.     *
 * disturbed    : set if a cursor-only commit ever changes what any head is *
 *                scanning.                                                 *
 * moves        : pointer motions the environment has produced.             *
 * frames       : client frames the environment has produced.               *
 * commits      : atomic commits issued.                                    *
 * freed        : how many times the CRTC became free.                      *
 * quiet        : the client has stopped drawing, so a blocking cursor-only *
 *                commit may take a vblank without taking one a frame       *
 *                needed. Drawing clears it; `Quiesce` sets it.             *
 *************************************************************************)
VARIABLES outstanding, pendingCursor, committed, pendingFrame, scanned,
          doubleCommit, lostFrame, disturbed, moves, frames, commits, freed,
          quiet

vars == <<outstanding, pendingCursor, committed, pendingFrame, scanned,
          doubleCommit, lostFrame, disturbed, moves, frames, commits, freed,
          quiet>>

Kinds == {"none", "primary", "cursorOnly"}

Init ==
    /\ outstanding = "none"
    /\ pendingCursor = 0
    /\ committed = [h \in Heads |-> 0]
    /\ pendingFrame = 0
    /\ scanned = [h \in Heads |-> 0]
    /\ doubleCommit = FALSE
    /\ lostFrame = FALSE
    /\ disturbed = FALSE
    /\ moves = 0
    /\ frames = 0
    /\ commits = 0
    /\ freed = 0
    /\ quiet = TRUE

(***************************************************************************
 * Environment: the pointer moves. Unfair and bounded -- nothing obliges a  *
 * hand to move a mouse. A move while one is already pending supersedes it  *
 * in place, which is the latest-wins cell the implementation needs.        *
 *************************************************************************)
PointerMoves ==
    /\ moves < MaxMoves
    /\ moves' = moves + 1
    /\ pendingCursor' = moves + 1
    /\ UNCHANGED <<outstanding, committed, pendingFrame, scanned,
         doubleCommit, lostFrame, disturbed, frames, commits, freed, quiet>>

(***************************************************************************
 * Environment: the client draws. Also unfair: a client repainting on a     *
 * cursor blink may not draw again for most of a second, which is exactly   *
 * why a cursor cannot be made to wait for the next frame.                  *
 *************************************************************************)
ClientDraws ==
    /\ frames < MaxFrames
    /\ pendingFrame = 0
    /\ frames' = frames + 1
    /\ pendingFrame' = frames + 1
    /\ quiet' = FALSE
    /\ UNCHANGED <<outstanding, pendingCursor, committed, scanned,
         doubleCommit, lostFrame, disturbed, moves, commits, freed>>

(***************************************************************************
 * Environment: the client falls silent. This is the implementation's two   *
 * refreshes without a retirement, with the duration abstracted away. Fair, *
 * unlike drawing: a client that has stopped stays stopped until it draws   *
 * again, which is what lets a pending cursor eventually find its commit.   *
 *************************************************************************)
Quiesce ==
    /\ pendingFrame = 0
    /\ quiet = FALSE
    /\ quiet' = TRUE
    /\ UNCHANGED <<outstanding, pendingCursor, committed, pendingFrame,
         scanned, doubleCommit, lostFrame, disturbed, moves, frames, commits,
         freed>>

(***************************************************************************
 * A commit carrying the primary, and the cursor too when one is pending.   *
 * This is the cheap case: the cursor rides a request that was going out    *
 * anyway.                                                                  *
 *************************************************************************)
CommitPrimary ==
    /\ outstanding = "none"
    /\ pendingFrame # 0
    /\ outstanding' = "primary"
    /\ scanned' = [h \in Heads |-> pendingFrame]
    /\ pendingFrame' = 0
    /\ committed' =
           IF pendingCursor = 0
           THEN committed
           ELSE [h \in Heads |->
                    IF h \in CoveredBy(pendingCursor) THEN pendingCursor ELSE 0]
    /\ pendingCursor' = 0
    /\ commits' = commits + 1
    /\ doubleCommit' = (doubleCommit \/ outstanding # "none")
    /\ UNCHANGED <<lostFrame, disturbed, moves, frames, freed, quiet>>

(***************************************************************************
 * A commit carrying only the cursor. Atomic requests are sparse, so a      *
 * request naming only cursor properties leaves the primary's framebuffer   *
 * bound -- which is what lets a directly scanned client buffer stay on the *
 * plane while the pointer moves over it. `scanned` is therefore carried    *
 * forward explicitly rather than by UNCHANGED, and `disturbed` compares    *
 * the new value against the old, so a weakened version shows up as a       *
 * recorded violation instead of a quietly different model.                 *
 *************************************************************************)
CommitCursorOnly ==
    /\ outstanding = "none"
    /\ pendingCursor # 0
    /\ pendingFrame = 0
    /\ quiet
    /\ outstanding' = "cursorOnly"
    /\ committed' = [h \in Heads |->
                        IF h \in CoveredBy(pendingCursor) THEN pendingCursor ELSE 0]
    /\ pendingCursor' = 0
    /\ commits' = commits + 1
    /\ scanned' = scanned
    /\ disturbed' = (disturbed \/ \E h \in Heads : scanned'[h] # scanned[h])
    /\ doubleCommit' = (doubleCommit \/ outstanding # "none")
    /\ UNCHANGED <<pendingFrame, lostFrame, moves, frames, freed, quiet>>

(***************************************************************************
 * The commit completes and the CRTC frees. This is the page-flip event for *
 * a primary commit; a cursor-only commit settles the same way as far as    *
 * the CRTC is concerned, which is the point -- it occupies the same slot.  *
 *************************************************************************)
CommitCompletes ==
    /\ outstanding # "none"
    /\ outstanding' = "none"
    /\ freed' = freed + 1
    /\ UNCHANGED <<pendingCursor, committed, pendingFrame, scanned,
         doubleCommit, lostFrame, disturbed, moves, frames, commits, quiet>>

(***************************************************************************
 * The driver refuses a combined commit. The retry drops the cursor and     *
 * commits the primary alone: the frame survives, and the cursor stays      *
 * pending for a later commit rather than being discarded with the request  *
 * that carried it. A cursor must never cost a frame.                       *
 *                                                                          *
 * `lostFrame` re-evaluates whether the frame reached the plane, so a retry *
 * that dropped the primary instead would be a recorded violation.          *
 *************************************************************************)
CombinedCommitRefused ==
    /\ outstanding = "none"
    /\ pendingFrame # 0
    /\ pendingCursor # 0
    /\ outstanding' = "primary"
    /\ scanned' = [h \in Heads |-> pendingFrame]
    /\ pendingFrame' = 0
    /\ lostFrame' = (lostFrame \/ \E h \in Heads : scanned'[h] # pendingFrame)
    /\ commits' = commits + 1
    /\ doubleCommit' = (doubleCommit \/ outstanding # "none")
    /\ UNCHANGED <<pendingCursor, committed, disturbed, moves,
         frames, freed, quiet>>

Owner ==
    \/ CommitPrimary
    \/ CommitCursorOnly
    \/ CombinedCommitRefused
    \/ CommitCompletes

Next ==
    \/ PointerMoves
    \/ ClientDraws
    \/ Quiesce
    \/ Owner

Spec == Init /\ [][Next]_vars

(***************************************************************************
 * `Quiesce` is fair and drawing is not, which is what keeps the liveness   *
 * property true under the gate: a client that keeps drawing carries the    *
 * cursor on its own commits, and one that stops eventually goes quiet and  *
 * releases the cursor-only commit.                                          *
 *************************************************************************)
FairSpec == Spec /\ WF_vars(Owner) /\ WF_vars(Quiesce)

TypeOK ==
    /\ outstanding \in Kinds
    /\ pendingCursor \in 0..MaxMoves
    /\ committed \in [Heads -> 0..MaxMoves]
    /\ pendingFrame \in 0..MaxFrames
    /\ scanned \in [Heads -> 0..MaxFrames]
    /\ doubleCommit \in BOOLEAN
    /\ lostFrame \in BOOLEAN
    /\ disturbed \in BOOLEAN
    /\ quiet \in BOOLEAN
    /\ commits \in 0..(MaxMoves + 2 * MaxFrames + 1)

(***************************************************************************
 * The kernel's rule, which is the reason this model exists. Two commits    *
 * outstanding on one CRTC is not a race to be tuned; it is a request the   *
 * driver refuses.                                                          *
 *************************************************************************)
OneOutstandingCommitPerCrtc == ~doubleCommit

(***************************************************************************
 * A cursor-only commit leaves the primary scanning what it was scanning.   *
 * Without this a cursor move over a directly scanned frame could evict the *
 * client's buffer, which is the interaction the whole row has to preserve. *
 *************************************************************************)
CursorOnlyCommitPreservesPrimary == ~disturbed

(***************************************************************************
 * No head keeps showing a cursor the pointer has left.                     *
 *                                                                          *
 * This is the group-specific one. A commit carries every head of the       *
 * session, so moving the cursor onto one head is also the moment to take   *
 * it off the others -- the legacy path does exactly that, hiding on every  *
 * CRTC no longer named by a target. An implementation that updated only    *
 * the heads it landed on would leave a cursor frozen on the monitor the    *
 * pointer walked off, which looks like a stuck pointer and is not caught   *
 * by any of the checks above.                                              *
 *                                                                          *
 * Stated as heads agreeing rather than as each head matching its own       *
 * position's coverage: the first version of this passed its own controls,  *
 * because a head left showing an old position is still consistent with     *
 * where that position was covered. What a ghost actually looks like is two *
 * heads showing different cursors at once.                                 *
 *************************************************************************)
CursorLeavesNoGhost ==
    \A h1, h2 \in Heads :
        (committed[h1] # 0 /\ committed[h2] # 0) => committed[h1] = committed[h2]

(***************************************************************************
 * A cursor never costs a frame. A refused combined commit retries with the *
 * primary alone; the cursor waits, which is invisible next to a dropped    *
 * frame.                                                                   *
 *************************************************************************)
NoFrameLostToCursor == ~lostFrame

(***************************************************************************
 * Work is bounded by how often the CRTC becomes free, not by how often the *
 * pointer moves. This is what the roadmap means by bounded cursor-only     *
 * idle work: an implementation that queued a commit per pointer event      *
 * would break it, because a hand moving a mouse produces motion far faster *
 * than a display retires frames.                                           *
 *                                                                          *
 * In this model it is implied rather than independent: exceeding the bound  *
 * requires committing while a commit is outstanding, which sets            *
 * `doubleCommit` first. Its control fires on that invariant instead, and   *
 * this is kept as a stated consequence rather than deleted -- the same     *
 * reasoning `PresentFlipOwnership` gives for keeping a conjunct its        *
 * neighbours already make unreachable. An implementation is free to drift  *
 * the two apart; the property should be here when it does.                 *
 *************************************************************************)
CursorWorkBoundedByAvailability == commits <= freed + 1

(***************************************************************************
 * A moved cursor eventually reaches a plane. Under service fairness only:  *
 * nothing here says when, and nothing obliges the client to draw -- which  *
 * is the point, since a cursor that waited for the next client frame would *
 * freeze on an idle desktop.                                               *
 *************************************************************************)
PendingCursorEventuallyCommits ==
    (pendingCursor # 0) ~> (pendingCursor = 0)

=============================================================================
