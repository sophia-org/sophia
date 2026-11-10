---- MODULE FrameJob_TTrace_1794312027 ----
EXTENDS Sequences, TLCExt, Toolbox, Naturals, TLC, FrameJob

_expression ==
    LET FrameJob_TEExpression == INSTANCE FrameJob_TEExpression
    IN FrameJob_TEExpression!expression
----

_trace ==
    LET FrameJob_TETrace == INSTANCE FrameJob_TETrace
    IN FrameJob_TETrace!trace
----

_inv ==
    ~(
        TLCGet("level") = Len(_TETrace)
        /\
        x = ([phase |-> "submitted", frame |-> 2, epoch |-> 1, jobEpoch |-> 1, live |-> TRUE, requested |-> FALSE, applied |-> FALSE, workerDone |-> TRUE, valid |-> TRUE, held |-> TRUE, displayed |-> 1, retired |-> {1}, premature |-> TRUE, stale |-> FALSE])
    )
----

_init ==
    /\ x = _TETrace[1].x
----

_next ==
    /\ \E i,j \in DOMAIN _TETrace:
        /\ \/ /\ j = i + 1
              /\ i = TLCGet("level")
        /\ x  = _TETrace[i].x
        /\ x' = _TETrace[j].x

\* Uncomment the ASSUME below to write the states of the error trace
\* to the given file in Json format. Note that you can pass any tuple
\* to `JsonSerialize`. For example, a sub-sequence of _TETrace.
    \* ASSUME
    \*     LET J == INSTANCE Json
    \*         IN J!JsonSerialize("FrameJob_TTrace_1794312027.json", _TETrace)

=============================================================================

 Note that you can extract this module `FrameJob_TEExpression`
  to a dedicated file to reuse `expression` (the module in the 
  dedicated `FrameJob_TEExpression.tla` file takes precedence 
  over the module `FrameJob_TEExpression` below).

---- MODULE FrameJob_TEExpression ----
EXTENDS Sequences, TLCExt, Toolbox, Naturals, TLC, FrameJob

expression == 
    [
        \* To hide variables of the `FrameJob` spec from the error trace,
        \* remove the variables below.  The trace will be written in the order
        \* of the fields of this record.
        x |-> x
        
        \* Put additional constant-, state-, and action-level expressions here:
        \* ,_stateNumber |-> _TEPosition
        \* ,_xUnchanged |-> x = x'
        
        \* Format the `x` variable as Json value.
        \* ,_xJson |->
        \*     LET J == INSTANCE Json
        \*     IN J!ToJson(x)
        
        \* Lastly, you may build expressions over arbitrary sets of states by
        \* leveraging the _TETrace operator.  For example, this is how to
        \* count the number of times a spec variable changed up to the current
        \* state in the trace.
        \* ,_xModCount |->
        \*     LET F[s \in DOMAIN _TETrace] ==
        \*         IF s = 1 THEN 0
        \*         ELSE IF _TETrace[s].x # _TETrace[s-1].x
        \*             THEN 1 + F[s-1] ELSE F[s-1]
        \*     IN F[_TEPosition - 1]
    ]

=============================================================================



Parsing and semantic processing can take forever if the trace below is long.
 In this case, it is advised to uncomment the module below to deserialize the
 trace from a generated binary file.

\*
\*---- MODULE FrameJob_TETrace ----
\*EXTENDS IOUtils, TLC, FrameJob
\*
\*trace == IODeserialize("FrameJob_TTrace_1794312027.bin", TRUE)
\*
\*=============================================================================
\*

---- MODULE FrameJob_TETrace ----
EXTENDS TLC, FrameJob

trace == 
    <<
    ([x |-> [phase |-> "idle", frame |-> 0, epoch |-> 1, jobEpoch |-> 0, live |-> TRUE, requested |-> FALSE, applied |-> FALSE, workerDone |-> TRUE, valid |-> FALSE, held |-> FALSE, displayed |-> 0, retired |-> {}, premature |-> FALSE, stale |-> FALSE]]),
    ([x |-> [phase |-> "working", frame |-> 1, epoch |-> 1, jobEpoch |-> 1, live |-> TRUE, requested |-> FALSE, applied |-> FALSE, workerDone |-> FALSE, valid |-> FALSE, held |-> TRUE, displayed |-> 0, retired |-> {}, premature |-> FALSE, stale |-> FALSE]]),
    ([x |-> [phase |-> "working", frame |-> 1, epoch |-> 1, jobEpoch |-> 1, live |-> TRUE, requested |-> FALSE, applied |-> FALSE, workerDone |-> TRUE, valid |-> TRUE, held |-> TRUE, displayed |-> 0, retired |-> {}, premature |-> FALSE, stale |-> FALSE]]),
    ([x |-> [phase |-> "submitted", frame |-> 1, epoch |-> 1, jobEpoch |-> 1, live |-> TRUE, requested |-> FALSE, applied |-> FALSE, workerDone |-> TRUE, valid |-> TRUE, held |-> TRUE, displayed |-> 0, retired |-> {}, premature |-> FALSE, stale |-> FALSE]]),
    ([x |-> [phase |-> "idle", frame |-> 1, epoch |-> 1, jobEpoch |-> 1, live |-> TRUE, requested |-> FALSE, applied |-> FALSE, workerDone |-> TRUE, valid |-> TRUE, held |-> FALSE, displayed |-> 1, retired |-> {}, premature |-> FALSE, stale |-> FALSE]]),
    ([x |-> [phase |-> "working", frame |-> 2, epoch |-> 1, jobEpoch |-> 1, live |-> TRUE, requested |-> FALSE, applied |-> FALSE, workerDone |-> FALSE, valid |-> FALSE, held |-> TRUE, displayed |-> 1, retired |-> {}, premature |-> FALSE, stale |-> FALSE]]),
    ([x |-> [phase |-> "working", frame |-> 2, epoch |-> 1, jobEpoch |-> 1, live |-> TRUE, requested |-> FALSE, applied |-> FALSE, workerDone |-> TRUE, valid |-> TRUE, held |-> TRUE, displayed |-> 1, retired |-> {}, premature |-> FALSE, stale |-> FALSE]]),
    ([x |-> [phase |-> "submitted", frame |-> 2, epoch |-> 1, jobEpoch |-> 1, live |-> TRUE, requested |-> FALSE, applied |-> FALSE, workerDone |-> TRUE, valid |-> TRUE, held |-> TRUE, displayed |-> 1, retired |-> {}, premature |-> FALSE, stale |-> FALSE]]),
    ([x |-> [phase |-> "submitted", frame |-> 2, epoch |-> 1, jobEpoch |-> 1, live |-> TRUE, requested |-> FALSE, applied |-> FALSE, workerDone |-> TRUE, valid |-> TRUE, held |-> TRUE, displayed |-> 1, retired |-> {1}, premature |-> TRUE, stale |-> FALSE]])
    >>
----


=============================================================================

---- CONFIG FrameJob_TTrace_1794312027 ----
CONSTANTS
    Mutation = "retire"

INVARIANT
    _inv

CHECK_DEADLOCK
    \* CHECK_DEADLOCK off because of PROPERTY or INVARIANT above.
    FALSE

INIT
    _init

NEXT
    _next

CONSTANT
    _TETrace <- _trace

ALIAS
    _expression
=============================================================================
\* Generated on Tue Nov 10 07:00:27 EST 2026