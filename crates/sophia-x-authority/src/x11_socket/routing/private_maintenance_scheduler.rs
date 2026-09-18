/// Which bounded post-collection visit the original executor selected.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrivateMaintenancePhase {
    #[default]
    Output,
    Terminal,
}

#[cfg(unix)]
#[derive(Default)]
struct PrivateMaintenanceCursor {
    next: PrivateMaintenancePhase,
    output: PrivateRetainedDriveCursor,
    terminal: PrivateTerminalDriveCursor,
}

/// Status of one visit, never an aggregate settlement result.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivateMaintenanceStatus {
    Unavailable,
    Refused,
    Yielded,
    Visited,
    SupervisionFailed,
    AccountingFailed,
}

#[cfg(unix)]
#[derive(Debug)]
enum PrivateMaintenanceOutcome {
    Unavailable,
    Output(PrivateRetainedDriveStep),
    Terminal(PrivateTerminalDriveStep),
}

/// The exact internal visit remains available in Debug diagnostics. Public
/// observations distinguish admission, accounting and supervision without
/// turning a local completed visit into a claim that the invocation settled.
#[cfg(unix)]
#[derive(Debug)]
pub struct PrivateMaintenanceReport {
    phase: PrivateMaintenancePhase,
    outcome: PrivateMaintenanceOutcome,
}

#[cfg(unix)]
impl PrivateMaintenanceReport {
    pub fn phase(&self) -> PrivateMaintenancePhase {
        self.phase
    }

    pub fn charge(
        &self,
    ) -> Option<
        &Result<
            sophia_input_authority::ServiceCharge,
            sophia_input_authority::ServiceAccountingError,
        >,
    > {
        match &self.outcome {
            PrivateMaintenanceOutcome::Output(PrivateRetainedDriveStep::Charged {
                charge, ..
            })
            | PrivateMaintenanceOutcome::Terminal(PrivateTerminalDriveStep::Charged {
                charge,
                ..
            }) => Some(charge),
            _ => None,
        }
    }

    pub fn allowance_refusal(&self) -> Option<sophia_input_authority::ServiceStartRefusal> {
        match self.outcome {
            PrivateMaintenanceOutcome::Output(PrivateRetainedDriveStep::Yield(cause))
            | PrivateMaintenanceOutcome::Terminal(PrivateTerminalDriveStep::Yield(cause)) => {
                Some(cause)
            }
            _ => None,
        }
    }

    /// A diagnostic of a refusal before a charged visit. Charged refusals and
    /// late failures remain in this report's full Debug observation.
    pub fn entry_refusal(&self) -> Option<String> {
        match &self.outcome {
            PrivateMaintenanceOutcome::Output(PrivateRetainedDriveStep::Refused(cause)) => {
                Some(format!("{cause:?}"))
            }
            PrivateMaintenanceOutcome::Terminal(PrivateTerminalDriveStep::Refused(cause)) => {
                Some(format!("{cause:?}"))
            }
            _ => None,
        }
    }

    pub fn status(&self) -> PrivateMaintenanceStatus {
        use PrivateMaintenanceStatus as Status;
        match &self.outcome {
            PrivateMaintenanceOutcome::Unavailable => Status::Unavailable,
            PrivateMaintenanceOutcome::Output(PrivateRetainedDriveStep::Refused(_))
            | PrivateMaintenanceOutcome::Terminal(PrivateTerminalDriveStep::Refused(_)) => {
                Status::Refused
            }
            PrivateMaintenanceOutcome::Output(PrivateRetainedDriveStep::Yield(_))
            | PrivateMaintenanceOutcome::Terminal(PrivateTerminalDriveStep::Yield(_)) => {
                Status::Yielded
            }
            PrivateMaintenanceOutcome::Output(PrivateRetainedDriveStep::Charged {
                outcome,
                charge,
                supervision,
            }) => Self::charged_status(outcome.is_ok(), charge.is_ok(), supervision.is_ok()),
            PrivateMaintenanceOutcome::Terminal(PrivateTerminalDriveStep::Charged {
                outcome,
                charge,
                supervision,
            }) => Self::charged_status(outcome.is_ok(), charge.is_ok(), supervision.is_ok()),
        }
    }

    fn charged_status(
        visited: bool,
        accounted: bool,
        supervised: bool,
    ) -> PrivateMaintenanceStatus {
        use PrivateMaintenanceStatus as Status;
        if !accounted {
            Status::AccountingFailed
        } else if !supervised {
            Status::SupervisionFailed
        } else if visited {
            Status::Visited
        } else {
            Status::Refused
        }
    }

    /// Only this selected ordered home, as established by its source's
    /// settled predicate. Other homes and terminal debts may remain owed.
    /// This remains readable even when the visit's finish reports a failure.
    pub fn output_settled(&self) -> Option<bool> {
        match self.outcome {
            PrivateMaintenanceOutcome::Output(PrivateRetainedDriveStep::Charged {
                outcome: Ok(PrivateRetainedVisit::Driven { settled }),
                ..
            }) => Some(settled),
            _ => None,
        }
    }
}

#[cfg(unix)]
impl PrivateServiceExecutionKeeper {
    /// Perform one finite visit after the service has returned or its unwind
    /// has been caught by this keeper's original executing thread.
    ///
    /// First drop the returned settlement into the independent durable owner;
    /// terminal visits can only borrow inventory that owner actually holds.
    /// This method does no waiting for a new budget interval, starts no actor,
    /// and cannot rebuild unavailable keyboard history or supervision.
    ///
    /// Output and terminal visits alternate, including after refusal. Each
    /// uses the same original budget and watchdog. The cursor is advanced
    /// before entering the visit so an unwind cannot restart that selected
    /// operation; its original accounting guard also latches interruption.
    ///
    /// A visited or settled output home is not whole-invocation completion.
    /// This scheduler never retires the external custody which may still own
    /// exact termination evidence needed by terminal/native dependents.
    pub fn maintain_step(&mut self, service: &PrivateServiceLease<'_>) -> PrivateMaintenanceReport {
        let phase = self.maintenance.next;
        let Some(resources) = self.resources.as_mut() else {
            return PrivateMaintenanceReport {
                phase,
                outcome: PrivateMaintenanceOutcome::Unavailable,
            };
        };
        self.maintenance.next = match phase {
            PrivateMaintenancePhase::Output => PrivateMaintenancePhase::Terminal,
            PrivateMaintenancePhase::Terminal => PrivateMaintenancePhase::Output,
        };
        let outcome = match phase {
            PrivateMaintenancePhase::Output => PrivateMaintenanceOutcome::Output(
                resources.drive_retained_output_step(service, &mut self.maintenance.output),
            ),
            PrivateMaintenancePhase::Terminal => PrivateMaintenanceOutcome::Terminal(
                resources.drive_terminal_step(service, &mut self.maintenance.terminal),
            ),
        };
        PrivateMaintenanceReport { phase, outcome }
    }
}
