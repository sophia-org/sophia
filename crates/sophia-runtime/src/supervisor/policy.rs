use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RestartPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(0),
            max_backoff: Duration::from_secs(1),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupervisedProcessKind {
    WindowManager,
    PortalBroker,
    MetadataBroker,
    Shell,
    OutputAuthority,
    SophiaXAuthority,
    LockProvider,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupervisorState {
    pub process: SupervisedProcessKind,
    pub running: bool,
    pub restart_attempts: u32,
}

impl SupervisorState {
    pub const fn new(process: SupervisedProcessKind) -> Self {
        Self {
            process,
            running: false,
            restart_attempts: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupervisorEvent {
    StartRequested,
    RestartRequested,
    ProcessExited,
    ProcessStarted,
    ProcessHealthy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SupervisorCommand {
    None,
    StartProcess {
        process: SupervisedProcessKind,
        delay: Duration,
    },
    GiveUp {
        process: SupervisedProcessKind,
    },
}

pub fn update_supervisor(
    mut state: SupervisorState,
    event: SupervisorEvent,
    policy: RestartPolicy,
) -> (SupervisorState, SupervisorCommand) {
    match event {
        SupervisorEvent::StartRequested => {
            state.running = false;
            state.restart_attempts = 0;
            let command = SupervisorCommand::StartProcess {
                process: state.process,
                delay: Duration::ZERO,
            };
            (state, command)
        }
        SupervisorEvent::RestartRequested | SupervisorEvent::ProcessExited => {
            state.running = false;
            match next_restart_delay(state.restart_attempts, policy) {
                Some(delay) => {
                    state.restart_attempts = state.restart_attempts.saturating_add(1);
                    let command = SupervisorCommand::StartProcess {
                        process: state.process,
                        delay,
                    };
                    (state, command)
                }
                None => {
                    let process = state.process;
                    (state, SupervisorCommand::GiveUp { process })
                }
            }
        }
        SupervisorEvent::ProcessStarted => {
            state.running = true;
            (state, SupervisorCommand::None)
        }
        SupervisorEvent::ProcessHealthy => {
            state.running = true;
            state.restart_attempts = 0;
            (state, SupervisorCommand::None)
        }
    }
}

fn next_restart_delay(attempts: u32, policy: RestartPolicy) -> Option<Duration> {
    if attempts >= policy.max_attempts {
        return None;
    }

    if attempts == 0 || policy.initial_backoff.is_zero() {
        return Some(Duration::ZERO);
    }

    let multiplier = 1_u32
        .checked_shl(attempts.saturating_sub(1))
        .unwrap_or(u32::MAX);
    Some(
        policy
            .initial_backoff
            .saturating_mul(multiplier)
            .min(policy.max_backoff),
    )
}
