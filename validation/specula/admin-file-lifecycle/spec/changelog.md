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
