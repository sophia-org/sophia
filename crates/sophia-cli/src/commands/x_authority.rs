use super::prelude::*;
use sophia_x_authority::{
    XAuthorityClientControlCommand, XAuthorityClientInputEvent, XAuthorityClientSurfaceRoutes,
    XAuthorityControlCommand, XAuthorityControlOutcome, XAuthorityInputDeliveryId,
    XAuthorityInputDeliveryOutcome, XPresentCompletionMode, XServerFrontendConfig,
    XServerFrontendRenderDeviceError, XServerFrontendRenderDeviceProvider,
    XServerFrontendRouteBroker, XServerFrontendServiceCommand,
    run_x_server_frontend_routed_until_stopped,
    run_x11_core_socket_server_once_config_traced_with_idle_timeout,
    run_x11_core_socket_server_once_session_channels,
};
use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroUsize;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::sync::Arc;
use std::time::Instant;

pub(crate) fn try_run(args: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    if args
        .iter()
        .any(|arg| arg == "x-authority-truecolor-palette-client")
    {
        truecolor_client::run()?;
        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "x-authority-kitty-input-smoke")
    {
        let report = run_x_authority_kitty_input_smoke()?;
        println!(
            "x-authority-kitty-input-smoke display={} routed_keys={} present_before_input={} present_after_input={} text_match={}",
            report.display,
            report.routed_keys,
            report.present_before_input,
            report.present_after_input,
            report.text_match,
        );
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-browser-smoke") {
        let report = run_x_authority_browser_smoke()?;
        print_external_probe_smoke_report("x-authority-browser-smoke", &report);
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-kitty-smoke") {
        let report = run_x_authority_kitty_smoke()?;
        print_external_probe_smoke_report("x-authority-kitty-smoke", &report);
        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "x-authority-glx-pbuffer-smoke")
    {
        let report = run_x_authority_glx_pbuffer_smoke()?;
        print_external_probe_smoke_report("x-authority-glx-pbuffer-smoke", &report);
        return Ok(true);
    }
    if args.iter().any(|arg| arg == "x-authority-glxgears-smoke") {
        let report = run_x_authority_glxgears_smoke()?;
        print_external_probe_smoke_report("x-authority-glxgears-smoke", &report);
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-xmobar-smoke") {
        let report = run_x_authority_xmobar_smoke()?;
        print_external_probe_smoke_report("x-authority-xmobar-smoke", &report);
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-quickshell-smoke") {
        let report = run_x_authority_quickshell_smoke()?;
        print_external_probe_smoke_report("x-authority-quickshell-smoke", &report);
        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "x-authority-quickshell-software-smoke")
    {
        let report = run_x_authority_quickshell_software_smoke()?;
        print_external_probe_smoke_report("x-authority-quickshell-software-smoke", &report);
        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "x-authority-zenity-render-smoke")
    {
        let report = run_x_authority_zenity_render_smoke()?;
        print_external_probe_smoke_report("x-authority-zenity-render-smoke", &report);
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-vkcube-smoke") {
        let report = run_x_authority_vkcube_smoke()?;
        print_external_probe_smoke_report("x-authority-vkcube-smoke", &report);
        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "x-authority-vkcube-admission-smoke")
    {
        let report = run_x_authority_vkcube_admission_smoke()?;
        println!(
            "x-authority-vkcube-admission-smoke display={} intent={} admission_delivered={} dma_bufs={} presents={} feedback={}",
            report.display,
            report.intent_observed,
            report.admission_delivered,
            report.dma_bufs,
            report.presents,
            report.feedback,
        );
        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "x-authority-xterm-two-client-smoke")
    {
        let report = run_x_authority_xterm_two_client_smoke()?;
        println!(
            "x-authority-xterm-two-client-smoke display={} clients={} routed_keys={} initial_generation={} final_generation={} initial_checksum={} final_checksum={} pixel_change={}",
            report.display,
            report.clients,
            report.routed_keys,
            report.initial_generation,
            report.final_generation,
            report.initial_checksum,
            report.final_checksum,
            report.initial_checksum != report.final_checksum,
        );
        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "x-authority-xterm-input-smoke")
    {
        let report = run_x_authority_xterm_input_smoke()?;
        println!(
            "x-authority-xterm-input-smoke display={} keys={} initial_generation={} final_generation={} initial_checksum={} final_checksum={} pixel_change={} text_match={}",
            report.display,
            report.keys,
            report.initial_generation,
            report.final_generation,
            report.initial_checksum,
            report.final_checksum,
            report.initial_checksum != report.final_checksum,
            report.text_match,
        );
        return Ok(true);
    }

    if let Some(spec) = EXTERNAL_PROBE_SMOKES
        .iter()
        .find(|spec| args.iter().any(|arg| arg == spec.command_name))
    {
        let report = run_x_authority_external_probe_smoke_spec(spec)?;
        print_external_probe_smoke_report(spec.command_name, &report);
        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "x-authority-present-pixmap-smoke")
    {
        let report = run_x_authority_present_pixmap_smoke()?;
        println!(
            "x-authority-present-pixmap-smoke display={} extension_opcode={} transactions={} runtime_committed={} runtime_surfaces={}",
            report.display,
            report.extension_opcode,
            report.transactions,
            report.runtime_committed,
            report.runtime_surfaces
        );
        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "x-authority-xlib-put-image-smoke")
    {
        let report = run_x_authority_xlib_put_image_smoke()?;
        println!(
            "x-authority-xlib-put-image-smoke display={} status={} stdout_bytes={} stderr_bytes={} image_ops={} transactions={} runtime_committed={} runtime_surfaces={}",
            report.display,
            report.status,
            report.stdout_bytes,
            report.stderr_bytes,
            report.image_ops,
            report.transactions,
            report.runtime_committed,
            report.runtime_surfaces
        );
        return Ok(true);
    }

    if args
        .iter()
        .any(|arg| arg == "x-authority-xlib-drawing-smoke")
    {
        let report = run_x_authority_xlib_drawing_smoke()?;
        println!(
            "x-authority-xlib-drawing-smoke display={} status={} stdout_bytes={} stderr_bytes={} draw_ops={} transactions={} runtime_committed={} runtime_surfaces={}",
            report.display,
            report.status,
            report.stdout_bytes,
            report.stderr_bytes,
            report.draw_ops,
            report.transactions,
            report.runtime_committed,
            report.runtime_surfaces
        );
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-xlib-smoke") {
        let report = run_x_authority_xlib_smoke()?;
        println!(
            "x-authority-xlib-smoke display={} status={} stdout_bytes={} stderr_bytes={} title_bytes={} title_match={}",
            report.display,
            report.status,
            report.stdout_bytes,
            report.stderr_bytes,
            report.title_bytes,
            report.title_match
        );
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-xdpyinfo-smoke") {
        let report = run_x_authority_xdpyinfo_smoke()?;
        println!(
            "x-authority-xdpyinfo-smoke display={} status={} stdout_bytes={} stderr_bytes={} mentions_sophia={} mentions_root={}",
            report.display,
            report.status,
            report.stdout_bytes,
            report.stderr_bytes,
            report.mentions_sophia,
            report.mentions_root
        );
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-shm-fd-smoke") {
        let report = run_x_authority_shm_fd_smoke()?;
        println!(
            "x-authority-shm-fd-smoke display={} shm_version={}.{} created_bytes={} written={} read_back={} attached_fd_segments={} granted_xids={} oversize_refused={} errors={}",
            report.display,
            report.major_version,
            report.minor_version,
            report.created_bytes,
            report.written,
            report.read_back,
            report.attached_fd_segments,
            report.granted_xids,
            report.oversize_refused,
            report.errors,
        );
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-render-smoke") {
        let report = run_x_authority_render_smoke()?;
        println!(
            "x-authority-render-smoke display={} version={}.{} formats={} composited_pixel={:?} glyph_pixel={:?} errors={}",
            report.display,
            report.major_version,
            report.minor_version,
            report.formats,
            report.composited_pixel,
            report.glyph_pixel,
            report.errors
        );
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-x11rb-smoke") {
        let report = run_x_authority_x11rb_smoke()?;
        println!(
            "x-authority-x11rb-smoke display={} window={:#x} title_bytes={} configure_notify={} map_notify={} errors={}",
            report.display,
            report.window,
            report.title_bytes,
            report.configure_notify,
            report.map_notify,
            report.errors
        );
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-x11-smoke") {
        let report = run_x_authority_x11_smoke()?;
        println!(
            "x-authority-x11-smoke setup=ok configure_notify={} map_notify={} property_bytes={} errors={}",
            report.configure_notify, report.map_notify, report.property_bytes, report.errors
        );
        return Ok(true);
    }

    if args.iter().any(|arg| arg == "x-authority-runtime-smoke") {
        let report = run_x_authority_runtime_smoke()?;
        println!(
            "x-authority-runtime-smoke socket={} surfaces={} transactions={} portal_prompts={} selection_artifacts={}",
            report.socket_path.display(),
            report.surfaces,
            report.transactions,
            report.portal_prompts,
            report.selection_artifacts
        );
        return Ok(true);
    }

    Ok(false)
}

include!("x_authority/reports.rs");
include!("x_authority/render_device_provider.rs");
include!("x_authority/basic_smokes.rs");
include!("x_authority/render_smoke.rs");
mod truecolor_client;
include!("x_authority/kitty_input_smoke.rs");
include!("x_authority/vkcube_admission_smoke.rs");
include!("x_authority/terminal_probe.rs");
include!("x_authority/xterm.rs");
include!("x_authority/external_probe.rs");
include!("x_authority/runtime_proofs.rs");
include!("x_authority/wire_helpers.rs");
