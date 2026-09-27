pub(crate) fn print(verbose: bool) {
    println!("sophia {}", env!("CARGO_PKG_VERSION"));
    println!("components: engine, x-authority, protocol, wm-demo");
    println!("commands: inspect wm [--socket PATH] [--json] ls|stat PATH|status|snapshot|watch");
    println!(
        "commands: client-launch --adapter=chromium --argv-style=direct|wrapper [--check-only] -- PROGRAM ARGS..."
    );
    println!(
        "commands: msg [--socket PATH] [--json] commands|policy NAME|session restart-wm|reload-profile|logout"
    );
    println!("commands: config check [--config=/absolute/path]");
    println!("commands: config check --wm [--wm-config=/absolute/path]");
    println!("commands: config check --desktop-profile=/absolute/path");
    println!(
        "commands: config check-session-profile --desktop-profile=/absolute/path [--default-wm=/path/to/wm] (--policy-checker=/path/to/adapter | --allow-deferred-policy)"
    );
    println!("commands: config print-effective --desktop-profile=/absolute/path");
    println!("commands: config print-policy --desktop-profile=/absolute/path");
    println!(
        "commands: config print-component --desktop-profile=/absolute/path --component=window-manager|shell-client"
    );
    println!("commands: config print-effective [--wm]");
    println!("commands: runtime-damage-epoch-smoke");
    println!("commands: headless-session-driver-smoke");
    println!("commands: runtime-brokers-smoke [--portal=/usr/bin/true] [--metadata=/usr/bin/true]");
    println!("commands: portal-broker-health-smoke");
    println!("commands: metadata-broker-health-smoke");
    println!("commands: wm-supervisor-smoke [--wm=target/debug/sophia-wm-demo]");
    println!("commands: x-authority-runtime-smoke");
    println!("commands: x-authority-x11-smoke");
    println!("commands: x-authority-x11rb-smoke");
    println!("commands: x-authority-shm-fd-smoke");
    println!("commands: x-authority-xdpyinfo-smoke");
    println!("commands: x-authority-xlib-smoke");
    println!("commands: x-authority-xlib-drawing-smoke");
    println!("commands: x-authority-xlib-put-image-smoke");
    println!("commands: x-authority-xclock-smoke");
    println!("commands: x-authority-xeyes-smoke");
    println!("commands: x-authority-xwininfo-root-smoke");
    println!("commands: x-authority-xprop-root-smoke");
    println!("commands: x-authority-xsetroot-name-smoke");
    println!("commands: x-authority-xlogo-smoke");
    println!("commands: x-authority-xmessage-smoke");
    println!("commands: x-authority-xrandr-query-smoke");
    println!("commands: x-authority-xcalc-smoke");
    println!("commands: x-authority-xterm-smoke");
    println!("commands: x-authority-xterm-render-smoke");
    println!("commands: x-authority-xterm-input-smoke");
    println!("commands: x-authority-xterm-two-client-smoke");
    println!("commands: x-authority-zenity-smoke");
    println!("commands: x-authority-kitty-smoke");
    println!("commands: x-authority-glx-pbuffer-smoke");
    println!("commands: x-authority-glxgears-smoke");
    println!("commands: x-authority-kitty-input-smoke");
    println!("commands: x-authority-vkcube-admission-smoke");
    println!("commands: x-authority-xmobar-smoke");
    println!("commands: x-authority-present-pixmap-smoke");
    #[cfg(feature = "native-session")]
    println!(
        "commands: session run [--desktop-profile=/absolute/path] [--session-mode=normal --session-app=ID=/PATH --session-app-default=ID=EXECUTABLE --session-action-default=terminal|browser=ID --session-app-arg=ID=ARG ... --session-start=ID ... --session-start-default=ID --session-action-app=terminal|launcher|browser=ID --exit-when-startup-exits --startup-ready-timeout-ms=8000] [--client-backend=sophia-x] [--client=PATH] [--client-arg=ARG ...] [--display=:77] [--terminal=xterm] [--terminal-exec=PATH] [--terminal-exec-arg=ARG ...] [--secondary-terminal] [--namespace-profile=classic|confined] [--no-input|--input-seat=seat0|--input-devices=/dev/input/eventN,...] [--native-scanout] [--wm-process=PATH --wm-interface=sophia_wm_v1 --wm-transport=current-ipc|9p2000.L] [--shell-process=PATH --shell-transport=current-ipc|9p2000.L] [--wm-process-arg=ARG ...] [--wm-process-executable-grant=/absolute/path ...] [--max-runtime-ms=N] [--max-ticks=N] [--inject-text=lowercase|--expect-physical-text=lowercase] [--expect-physical-pointer] [--admit-xtest] [--exit-after-input-proof] [--proof]"
    );
    #[cfg(feature = "native-session")]
    println!(
        "diagnostics: session mark [--session=ID|latest] [LABEL] | session inspect ID|latest [--marker=ID] | session keep ID|latest [--include-application-stderr] | session launches ID|latest | session stderr ID|latest --launch=ID [--raw] | session list"
    );
    println!(
        "session prepare-environment --tty=PATH: prepare NUL-delimited bus mode and environment entries without starting a session or bus"
    );
    println!(
        "session prepare-controls --profile=LABEL: prepare generic guard/watchdog controls with an opaque state/log label"
    );
    println!(
        "session check-host --tty=/dev/ttyN [--allow-active=true|false]: run the explicit SOPHIA_SESSION_PREFLIGHT checker before device takeover; never discovers a desktop client"
    );
    println!("compatibility aliases: sophia-live-session, sophia-session-input-guard");
    #[cfg(feature = "native-session")]
    println!(
        "commands: native-egl-vkcube-mixed-smoke [--display=:184] [--terminal=xterm] [--max-runtime-ms=6000]"
    );
    #[cfg(feature = "native-session")]
    println!("commands: live-session-composition-smoke");
    #[cfg(feature = "native-session")]
    println!(
        "commands: shell-gpu-content-proof --transport=current-ipc|9p2000.L --client=/absolute/client [--client-arg=ARG]... [--config=/absolute/file] [--seat=seat0] [--render-node=/dev/dri/renderD128] --output=WxH --surface=WxH --edge=top|bottom|left|right --outcomes=presented|renderer-failed[,...] --end=client-exits|stop-client --pixels=contract|full-surface-raster --discrete-input=granted|denied [--timeout-ms=15000] (needs SOPHIA_SHELL_GPU_PROOF_ARM=1; optional SOPHIA_SHELL_GPU_EXPECTED_DEVICE=MAJOR:MINOR@PCI)"
    );
    #[cfg(feature = "native-session")]
    println!("commands: atomic-scanout-preflight");
    #[cfg(feature = "native-session")]
    println!("commands: native-topology-probe (read-only; needs DRM master)");
    println!("          native-mirror-probe (validation-only; needs DRM master)");
    println!("          native-mirror-page-flip (real commit; opt-in; needs DRM master)");
    println!("commands: native-topology-validate (read-only; needs DRM master)");
    println!(
        "commands: native-topology-apply (MUTATES outputs; needs SOPHIA_NATIVE_OUTPUT_APPLY=1)"
    );
    #[cfg(feature = "atomic-scanout-smoke-live")]
    println!("commands: atomic-vrr-inspect");
    #[cfg(feature = "atomic-scanout-smoke-live")]
    println!(
        "commands: sophia-live-session-content-hardware-proof [--terminal=xterm] [--slot=1] [--output=1] [--authority=1] [--page-flip-timeout-ms=8000]"
    );
    #[cfg(feature = "atomic-scanout-smoke-live")]
    println!(
        "commands: atomic-scanout-smoke [--slot=1] [--output=1] [--authority=1] [--page-flip-timeout-ms=8000] [--child-timeout-ms=30000]"
    );
    #[cfg(feature = "atomic-scanout-smoke-live")]
    println!(
        "commands: atomic-vrr-smoke [--slot=1] [--output=1] [--authority=1] [--page-flip-timeout-ms=8000] [--child-timeout-ms=30000]"
    );
    #[cfg(feature = "atomic-scanout-smoke-live")]
    println!(
        "commands: atomic-scanout-runtime-evidence [--slot=1] [--output=1] [--authority=1] [--page-flip-timeout-ms=8000]"
    );

    if verbose {
        tracing::debug!("verbose tracing enabled");
        println!("logging: tracing subscriber initialized");
    }
}
