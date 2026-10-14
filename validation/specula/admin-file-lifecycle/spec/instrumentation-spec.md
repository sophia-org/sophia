# Action map and future instrumentation

Source is `5cea0350b`. The file adapter does not yet exist. `TraceData.tla` is
explicitly a design example; `Trace.tla` verifies its selected post-state fields
and complete consumption. There are no runtime trace or refinement claims.
The `JsonFile` name reserves the later NDJSON input; it is not read today.

| Action | Boundary to record |
| --- | --- |
| Discover, Submit, ReadStatus, ClunkOrFlush | Future file adapter after snapshot pin, accepted write, read or fid release; draft contract lines 100–139. |
| ReplaceCatalog | Owner publishes a new catalog; `live_session/control.rs` generation changes. |
| RequestCheck | Successful queued→checking CAS, `control.rs:44–53`. |
| Approve | Successful checking→checked CAS, `control/transport.rs:85–90`. |
| Dispatch | Successful checked→dispatched CAS, `control.rs:54–58`; owner generation check at `live_session/control.rs:271`. |
| BeginCancel | Successful cancellation CAS, `control.rs:95–103`; denial, timeout or stale-owner branch selects the reason. |
| BeginComplete, WriteFinish, Settle | Owner finish, first-outcome CAS and release store, `control.rs:61–72`. |
| ExpireDispatched | Indeterminate CAS after cancellation loses, `control.rs:74–86`. |
| Reclaim | Removal after the settled acquire load, `control/transport.rs:343`. |
| Publish | Result projected to its exact connection/request, `control/transport.rs:99–104`; future file status adapter. |
| Disconnect, Reconnect | Peer loss and queued cancellation, `control.rs:89–103`; fresh identity at `control/transport.rs:352–382`. |

A future hook must record action, session/connection/request identities,
catalog and discovered generations, phase, first result, settlement, execution
charge, watermark, pending identity and visible status. Include claim/cancel
success rather than assuming it from entry to a function. Correlate worker and
owner events with operation intervals; do not infer a global order from unsynchronized
timestamps or take multiple atomic loads as a coherent snapshot. The abstract
BeginComplete marker is a local program-counter observation, not shared state.

The design trace captures phase, result, settlement, active count, pending,
shown outcome, effects, ending and pinned outcome; every captured field is
checked in `ValidatePostState`. Identity/generation and acceptance counts are
also checked for its single ticket. The exhaustive hunt covers cancellation,
catalog replacement and reconnect beyond this one example. Runtime validation
will need additional traces and a timebox adapter; this fixture does not supply them.
