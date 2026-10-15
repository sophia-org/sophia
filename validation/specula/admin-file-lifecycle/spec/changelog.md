# Model review

- Source inspection kept claim request, worker approval, dispatch, first result,
  settlement and capacity reclamation separate. The model does not assert an
  instantaneous revocation check after the checked claim handoff.
- Review removed a synthetic result publication after queued disconnect:
  `ControlTicket::disconnect` sets settlement without publishing an outcome.
  Final searches use that corrected transition. This was a modeling issue,
  not a production finding.
- Base and merged safety hunt passed. Deliberate negative configurations each
  violated their named property. The valid design trace matched; the premature
  settlement trace failed its temporal completion property. No production bug
  was established by these runs.

- Added the source-grounded service-instance boundary model at `bfd65d84f`.
  Its base and hunt pass (352 distinct states, depth 23). Disabling queued
  cancellation violates ShutdownCancels; projecting another instance's result
  violates ResultIsolation. The corresponding implementation retains cancellation
  CASes and separate ticket Arcs, so these are sensitivity controls, not bugs.
  The valid design trace passes and the false settlement trace fails as intended.
- Concurrent JVM runs failed while parsing a shared `/tmp` standard module.
  Separate reruns passed unchanged; the final cancellation control used a private
  Java temp directory. This was tool invocation interference, not an invariant
  violation. Future parallel runs need private Java temp directories.
