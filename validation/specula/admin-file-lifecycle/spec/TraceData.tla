---------------------------- MODULE TraceData ----------------------------
CONSTANT PrematureSettlement
\* Handwritten design trace, not an implementation recording.
Event(action, p, result, settled, active, pending, shown, effects, ending, pinned) ==
    [action |-> action, phase |-> p, result |-> result, settled |-> settled,
     active |-> active, pending |-> pending, shown |-> shown, effects |-> effects,
     ending |-> ending, pinned |-> pinned]
TraceLog == <<
 Event("Discover", "unused", "none", FALSE, 0, 0, "ready", 0, "none", "ready"),
 Event("Submit", "queued", "none", FALSE, 1, 1, "pending", 0, "none", "ready"),
 Event("RequestCheck", "checking", "none", FALSE, 1, 1, "pending", 0, "none", "ready"),
 Event("Approve", "checked", "none", FALSE, 1, 1, "pending", 0, "none", "ready"),
 Event("Dispatch", "dispatched", "none", FALSE, 1, 1, "pending", 1, "none", "ready"),
 Event("Expire", "dispatched", "indeterminate", FALSE, 1, 1, "pending", 1, "none", "ready"),
 Event("Publish", "dispatched", "indeterminate", FALSE, 1, 0, "indeterminate", 1, "none", "ready"),
 Event("Read", "dispatched", "indeterminate", PrematureSettlement, 1, 0, "indeterminate", 1, "none", "indeterminate"),
 Event("BeginComplete", "dispatched", "indeterminate", FALSE, 1, 0, "indeterminate", 1, "completed", "indeterminate"),
 Event("Settle", "dispatched", "indeterminate", TRUE, 1, 0, "indeterminate", 1, "completed", "indeterminate"),
 Event("Reclaim", "dispatched", "indeterminate", TRUE, 0, 0, "indeterminate", 1, "completed", "indeterminate") >>
=============================================================================
