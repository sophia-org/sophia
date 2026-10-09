// Frontend admission, wake wiring and native client render devices.
{
    let mut frontend_config =
        XServerFrontendConfig::new_with_namespace_context(&server_path, x_namespace)?
            .with_service_wake(frontend_service_wake.clone())
            .with_owner_wake(owner_wake.notifier())
            .with_output_topology(output_topology.clone())?
            .with_xkb_config(config.xkb_config.clone())?
            .with_setup_authorization(XServerFrontendSetupAuthorization::MitMagicCookie(
                xauthority_cookie,
            ))
            // XLibre maps immediately unless a redirecting policy owner is
            // present. Deferring without a WM strands the client's toplevel
            // before MapNotify, VisibilityNotify, and Expose.
            .with_policy_map_deferred(policy_map_mode.frontend_deferred())
            .with_font_path(config.font_path.clone())
            .with_admission_policy(admission_policy);
    let mut client_render_devices = None;
    if !config.software_client_rendering
        && let Some(native_scanout) = native_scanout.as_ref()
    {
        let seat = seat_controller
            .as_ref()
            .ok_or("native client device lost its seat")?
            .device_opener()
            .name()
            .to_owned();
        let (bundle, coordinator) = render_devices::initial(native_scanout, &seat, gpu_admission.clone())?;
        frontend_config = frontend_config.with_device_bundle(bundle);
        client_render_devices = Some(coordinator);
    }
    (frontend_config, client_render_devices)
}
