//! Argument preflight and secondary-login admission before runtime setup.
use super::{PersistentXtermSessionConfig, development};

pub(super) struct Prepared {
    pub(super) config: PersistentXtermSessionConfig,
    pub(super) development_admission: Option<development::Admission>,
}

pub(super) fn prepare(args: &[String]) -> Result<Option<Prepared>, Box<dyn std::error::Error>> {
    // Answer whether these arguments would be accepted, and stop.
    //
    // Every check `from_args` performs runs, and nothing else does: no DRM, no
    // input seat, no display manager stopped. Three physical runs died in this
    // function's first line with the display manager already down, because
    // nothing asked the question while it was still cheap to ask.
    let validate_only = args.iter().any(|arg| arg == "--validate-session-args");
    let args = if validate_only {
        args.iter()
            .filter(|arg| *arg != "--validate-session-args")
            .cloned()
            .collect::<Vec<_>>()
    } else {
        args.to_vec()
    };
    let args = args.as_slice();
    let config = PersistentXtermSessionConfig::from_args(args)?;
    let validate_development = args.iter().any(|arg| arg == "--validate-development-login");
    if validate_development && (validate_only || config.development_seat.is_none()) {
        return Err("--validate-development-login requires --development-seat and cannot be combined with --validate-session-args".into());
    }
    crate::diagnostics::application::set_enabled(
        config.core_config_state.active().application_stderr,
    );
    if validate_only {
        crate::session_println!(
            "sophia_live_session_args schema=1 status=accepted arguments={}",
            args.len(),
        );
        return Ok(None);
    }
    // This check precedes endpoint creation and any libseat control request.
    // A secondary login cannot be selected by an output profile or XDG claim.
    let development_admission = config.development_seat.as_ref()
        .map(development::DevelopmentSeat::attest).transpose()?;
    if validate_development {
        crate::session_println!("sophia_development_login schema=1 status=accepted devices_opened=0");
        return Ok(None);
    }
    Ok(Some(Prepared { config, development_admission }))
}
