//! Physical capture uses only the currently connected native transport focus.
use super::*;
use sophia_config::ShellComponentRole;

pub(in crate::live_session) fn synchronize_native_capture(
    components: &mut ShellComponentSession,
    capture: &mut sophia_engine::LauncherCapture,
    keyboard: &mut sophia_engine::LauncherKeyboard,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut binding = None;
    for (key, role) in components.connected_roles().into_iter().flatten() {
        if role == ShellComponentRole::ApplicationLauncher {
            binding =
                components.with_service(key, |_, transport| transport.native_launcher_focus())?;
        }
    }
    keyboard.synchronize_native_focus(capture, binding);
    Ok(())
}

pub(in crate::live_session) fn dispatch_native_input(
    components: &mut ShellComponentSession,
    catalog: &mut component_catalog::ComponentCatalog,
    event: &sophia_engine::LauncherInputEvent,
) -> Result<bool, Box<dyn std::error::Error>> {
    let sophia_engine::LauncherInput::Native { binding, .. } = &event.input else {
        return Err("independent launcher received a legacy capture".into());
    };
    let Some((key, _)) = components
        .connected_roles()
        .into_iter()
        .flatten()
        .find(|(key, role)| {
            *role == ShellComponentRole::ApplicationLauncher && key.grant == binding.grant
        })
    else {
        return Ok(false);
    };
    let transaction = catalog.mint_transaction()?;
    let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let now_usec = u64::try_from(clock.tv_sec)?
        .checked_mul(1_000_000)
        .and_then(|s| s.checked_add(u64::try_from(clock.tv_nsec).ok()? / 1000))
        .ok_or("native capture clock overflow")?;
    let result = components.with_service(key, |service, transport| {
        let ShellComponentService::Launcher { content, .. } = service else {
            return Err(sophia_runtime::ShellTransportError::WrongActivation);
        };
        content.dispatch_capture(transport, event, transaction, now_usec)
    })?;
    match result {
        Ok(accepted) => Ok(accepted),
        Err(error) => {
            crate::session_eprintln!(
                "sophia_native_launcher schema=1 status=input_failed slot={} reason={error}",
                key.slot
            );
            components.stop(key)?;
            Ok(false)
        }
    }
}
