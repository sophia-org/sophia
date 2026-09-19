fn run_x_authority_x11_smoke() -> Result<XAuthorityX11SmokeReport, Box<dyn std::error::Error>> {
    use std::io::Write;

    let socket_path = std::env::temp_dir().join(format!(
        "sophia-x-authority-x11-{}-{}.sock",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    let server_path = socket_path.clone();
    let server = std::thread::spawn(move || {
        run_x11_core_socket_server_once(&server_path, NamespaceId::from_raw(41))
    });

    wait_for_socket_path(&socket_path)?;
    let mut stream = UnixStream::connect(&socket_path)?;
    stream.write_all(&x11_setup_request(XByteOrder::LittleEndian))?;
    read_x11_setup_success(&mut stream, XByteOrder::LittleEndian)?;

    stream.write_all(&x11_intern_atom_request(
        XByteOrder::LittleEndian,
        false,
        "_NET_WM_NAME",
    ))?;
    let net_wm_name = read_x11_record(&mut stream)?;
    let net_wm_name = read_x11_u32(XByteOrder::LittleEndian, &net_wm_name[8..12]);

    stream.write_all(&x11_intern_atom_request(
        XByteOrder::LittleEndian,
        false,
        "UTF8_STRING",
    ))?;
    let utf8 = read_x11_record(&mut stream)?;
    let utf8 = read_x11_u32(XByteOrder::LittleEndian, &utf8[8..12]);

    stream.write_all(&x11_create_window_request(
        XByteOrder::LittleEndian,
        0x0020_0001,
        20,
        30,
        640,
        480,
    ))?;
    let configure = read_x11_record(&mut stream)?;

    stream.write_all(&x11_resource_request(
        XByteOrder::LittleEndian,
        8,
        0x0020_0001,
    ))?;
    let map = read_x11_record(&mut stream)?;

    stream.write_all(&x11_change_property_request(
        XByteOrder::LittleEndian,
        0x0020_0001,
        net_wm_name,
        utf8,
        b"Sophia Socket",
    ))?;
    let property_notify = read_x11_record(&mut stream)?;

    stream.write_all(&x11_get_property_request(
        XByteOrder::LittleEndian,
        0x0020_0001,
        net_wm_name,
        0,
        0,
        64,
    ))?;
    let property = read_x11_reply(&mut stream, XByteOrder::LittleEndian)?;

    let records = [configure, map, property_notify];
    let configure_notify = records.iter().filter(|record| record[0] == 22).count();
    let map_notify = records.iter().filter(|record| record[0] == 19).count();
    let errors = records.iter().filter(|record| record[0] == 0).count();

    drop(stream);
    let _ = std::fs::remove_file(&socket_path);
    server
        .join()
        .map_err(|_| "X authority X11 socket server thread panicked")??;

    Ok(XAuthorityX11SmokeReport {
        configure_notify,
        map_notify,
        property_bytes: usize::try_from(read_x11_u32(XByteOrder::LittleEndian, &property[16..20]))?,
        errors,
    })
}

/// Drives MIT-SHM 1.2 over a real socket, both directions of descriptor.
///
/// The wire tests cover decode and dispatch, and cannot cover the part that
/// actually carries a descriptor: `CreateSegment` puts one in a reply and
/// `AttachFd` takes one from a request, and both of those live in the socket
/// layer. This is the only test that crosses that boundary.
///
/// The proof is not that the requests are accepted. It is that memory written
/// through the descriptor the server returned is memory the server holds --
/// otherwise a server could satisfy every check here by handing back any
/// descriptor at all.
fn run_x_authority_shm_fd_smoke() -> Result<XAuthorityShmFdSmokeReport, Box<dyn std::error::Error>> {
    use x11rb::connection::Connection;
    use std::os::fd::AsFd as _;
    use x11rb::protocol::shm::ConnectionExt as _;

    let display_number = 7990 + (std::process::id() % 200);
    let display = format!(":{display_number}");
    let socket_path = std::path::PathBuf::from(format!("/tmp/.X11-unix/X{display_number}"));
    std::fs::create_dir_all("/tmp/.X11-unix")?;
    let server_path = socket_path.clone();
    let server = std::thread::spawn(move || {
        run_x11_core_socket_server_once(&server_path, NamespaceId::from_raw(66))
    });
    wait_for_socket_path(&socket_path)?;

    let (connection, _screen) = x11rb::connect(Some(&display))?;
    let version = connection.shm_query_version()?.reply()?;
    let mut errors = 0usize;

    // CreateSegment: the server allocates and hands the descriptor back.
    const SEGMENT_BYTES: u32 = 4096;
    let segment = connection.generate_id()?;
    let created = connection
        .shm_create_segment(segment, SEGMENT_BYTES, false)?
        .reply()?;
    let returned = created.shm_fd;
    let mapping = sophia_sysv_shm::DescriptorMapping::map(returned.as_fd(), false)?;
    let pattern: Vec<u8> = (0..64u16).map(|value| value as u8).collect();
    mapping.write_bytes(128, &pattern)?;
    let read_back = mapping.copy_bytes(128, pattern.len())?;
    if read_back != pattern {
        errors += 1;
    }

    // XC-MISC, over the same connection: the grant path lives in the socket
    // layer and the dispatch layer cannot reach it, so this is the only place
    // that proves a real range comes back rather than the honest zero.
    let xid_range = {
        use x11rb::protocol::xc_misc::ConnectionExt as _;
        let version = connection.xc_misc_get_version(1, 1)?.reply()?;
        assert_eq!(version.server_major_version, 1);
        let range = connection.xc_misc_get_xid_range()?.reply()?;
        // The block must not overlap the range this client was given at
        // connection setup, or the identifiers it hands out would collide with
        // resources it already owns.
        let setup = connection.setup();
        let own_end = setup.resource_id_base + setup.resource_id_mask;
        assert!(
            range.count > 0,
            "the server granted no identifiers: {range:?}"
        );
        assert!(
            range.start_id > own_end || range.start_id + range.count <= setup.resource_id_base,
            "granted range {}..{} overlaps this client's own {}..{}",
            range.start_id,
            range.start_id + range.count,
            setup.resource_id_base,
            own_end
        );
        range
    };

    // AttachFd: the client passes one the other way.
    let (ours, descriptor) = sophia_sysv_shm::DescriptorMapping::create_sealed(4096)?;
    drop(ours);
    let attached = connection.generate_id()?;
    connection
        .shm_attach_fd(attached, descriptor, false)?
        .check()?;
    connection.shm_detach(attached)?.check()?;

    // A size beyond what the adapter maps is refused rather than allocated.
    let oversize = connection.generate_id()?;
    let oversize_refused = connection
        .shm_create_segment(oversize, u32::MAX, false)?
        .reply()
        .is_err();

    connection.shm_detach(segment)?.check()?;
    drop(connection);
    let _ = server.join();
    let _ = std::fs::remove_file(&socket_path);

    Ok(XAuthorityShmFdSmokeReport {
        display,
        major_version: version.major_version,
        minor_version: version.minor_version,
        created_bytes: usize::try_from(SEGMENT_BYTES).unwrap_or(0),
        written: pattern.len(),
        read_back: read_back.len(),
        attached_fd_segments: 1,
        granted_xids: xid_range.count,
        oversize_refused,
        errors,
    })
}

fn run_x_authority_x11rb_smoke() -> Result<XAuthorityX11rbSmokeReport, Box<dyn std::error::Error>> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        AtomEnum, ConnectionExt, CreateWindowAux, PropMode, WindowClass,
    };
    use x11rb::wrapper::ConnectionExt as _;

    let display_number = 600 + (std::process::id() % 1000);
    let display = format!(":{display_number}");
    let socket_path = std::path::PathBuf::from(format!("/tmp/.X11-unix/X{display_number}"));
    std::fs::create_dir_all("/tmp/.X11-unix")?;
    let server_path = socket_path.clone();
    let server = std::thread::spawn(move || {
        run_x11_core_socket_server_once(&server_path, NamespaceId::from_raw(42))
    });

    wait_for_socket_path(&socket_path)?;
    let (connection, screen_index) = x11rb::connect(Some(&display))?;
    let screen = &connection.setup().roots[screen_index];
    let net_wm_name = connection
        .intern_atom(false, b"_NET_WM_NAME")?
        .reply()?
        .atom;
    let utf8 = connection.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
    let window = connection.generate_id()?;
    connection.create_window(
        screen.root_depth,
        window,
        screen.root,
        20,
        30,
        320,
        200,
        0,
        WindowClass::INPUT_OUTPUT,
        screen.root_visual,
        &CreateWindowAux::new(),
    )?;
    let title = b"Sophia x11rb";
    connection.change_property8(PropMode::REPLACE, window, net_wm_name, utf8, title)?;
    let property = connection
        .get_property(false, window, net_wm_name, AtomEnum::ANY, 0, 64)?
        .reply()?;
    connection.map_window(window)?;
    connection.flush()?;

    let mut configure_notify = 0usize;
    let mut map_notify = 0usize;
    let mut errors = 0usize;
    for _ in 0..8 {
        match connection.poll_for_event()? {
            Some(Event::ConfigureNotify(_)) => configure_notify += 1,
            Some(Event::MapNotify(_)) => map_notify += 1,
            Some(Event::Error(_)) => errors += 1,
            Some(_) => {}
            None => std::thread::sleep(Duration::from_millis(10)),
        }
    }

    drop(connection);
    let _ = std::fs::remove_file(&socket_path);
    server
        .join()
        .map_err(|_| "X authority X11 socket server thread panicked")??;

    Ok(XAuthorityX11rbSmokeReport {
        display,
        window,
        title_bytes: property.value.len(),
        configure_notify,
        map_notify,
        errors,
    })
}

fn run_x_authority_xdpyinfo_smoke()
-> Result<XAuthorityXdpyinfoSmokeReport, Box<dyn std::error::Error>> {
    let (display, socket_path) = temp_xauthority_display(1600)?;
    let server_path = socket_path.clone();
    let server = std::thread::spawn(move || {
        run_x11_core_socket_server_once(&server_path, NamespaceId::from_raw(43))
    });

    wait_for_socket_path(&socket_path)?;
    let output = std::process::Command::new("xdpyinfo")
        .arg("-display")
        .arg(&display)
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let status = output.status.code().unwrap_or(-1);
    let report = XAuthorityXdpyinfoSmokeReport {
        display: display.clone(),
        status,
        stdout_bytes: output.stdout.len(),
        stderr_bytes: output.stderr.len(),
        mentions_sophia: stdout.contains("Sophia") || stderr.contains("Sophia"),
        mentions_root: stdout.contains("root window id") || stderr.contains("root window id"),
    };

    let _ = std::fs::remove_file(&socket_path);
    server
        .join()
        .map_err(|_| "X authority X11 socket server thread panicked")??;

    if !output.status.success() {
        return Err(format!(
            "xdpyinfo failed for {display}: status={status} stderr={}",
            stderr.trim()
        )
        .into());
    }

    Ok(report)
}

fn run_x_authority_xlib_smoke() -> Result<XAuthorityXlibSmokeReport, Box<dyn std::error::Error>> {
    let (display, socket_path) = temp_xauthority_display(2600)?;
    let server_path = socket_path.clone();
    let server = std::thread::spawn(move || {
        run_x11_core_socket_server_once(&server_path, NamespaceId::from_raw(44))
    });
    wait_for_socket_path(&socket_path)?;
    let output = run_compiled_xlib_probe(&display, "xlib", XLIB_SMOKE_SOURCE)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let status = output.status.code().unwrap_or(-1);
    let title_bytes = xlib_smoke_title_bytes(&stdout).unwrap_or(0);
    let title_match = stdout.contains("title_match=1");

    let _ = std::fs::remove_file(&socket_path);
    server
        .join()
        .map_err(|_| "X authority X11 socket server thread panicked")??;

    if !output.status.success() {
        return Err(format!(
            "Xlib smoke failed for {display}: status={status} stdout={} stderr={}",
            stdout.trim(),
            stderr.trim()
        )
        .into());
    }

    Ok(XAuthorityXlibSmokeReport {
        display,
        status,
        stdout_bytes: output.stdout.len(),
        stderr_bytes: output.stderr.len(),
        title_bytes,
        title_match,
    })
}

fn run_x_authority_xlib_drawing_smoke()
-> Result<XAuthorityXlibDrawingSmokeReport, Box<dyn std::error::Error>> {
    let (display, socket_path) = temp_xauthority_display(3600)?;
    let server_path = socket_path.clone();
    let server = std::thread::spawn(move || -> Result<Vec<SurfaceTransaction>, String> {
        let mut transactions = Vec::new();
        run_x11_core_socket_server_once_observed(
            &server_path,
            NamespaceId::from_raw(45),
            |result| {
                if let Some(response) = &result.response {
                    transactions.extend(response.transactions.iter().cloned());
                }
            },
        )
        .map_err(|error| error.to_string())?;
        Ok(transactions)
    });
    wait_for_socket_path(&socket_path)?;
    let output = run_compiled_xlib_probe(&display, "xlib-drawing", XLIB_DRAWING_SMOKE_SOURCE)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let status = output.status.code().unwrap_or(-1);
    let draw_ops = xlib_smoke_field(&stdout, "draw_ops").unwrap_or(0);

    let _ = std::fs::remove_file(&socket_path);
    let transactions = server
        .join()
        .map_err(|_| "X authority X11 socket server thread panicked")?
        .map_err(|error| format!("X authority X11 socket server failed: {error}"))?;
    let runtime_state = runtime_state_from_observed_transactions(&transactions)?;

    if !output.status.success() {
        return Err(format!(
            "Xlib drawing smoke failed for {display}: status={status} stdout={} stderr={}",
            stdout.trim(),
            stderr.trim()
        )
        .into());
    }

    Ok(XAuthorityXlibDrawingSmokeReport {
        display,
        status,
        stdout_bytes: output.stdout.len(),
        stderr_bytes: output.stderr.len(),
        draw_ops,
        transactions: transactions.len(),
        runtime_committed: runtime_state.authority_transactions_committed,
        runtime_surfaces: runtime_state.authority_surfaces_applied,
    })
}

fn run_x_authority_xlib_put_image_smoke()
-> Result<XAuthorityXlibPutImageSmokeReport, Box<dyn std::error::Error>> {
    let (display, socket_path) = temp_xauthority_display(4600)?;
    let server_path = socket_path.clone();
    let server = std::thread::spawn(move || -> Result<Vec<SurfaceTransaction>, String> {
        let mut transactions = Vec::new();
        run_x11_core_socket_server_once_observed(
            &server_path,
            NamespaceId::from_raw(46),
            |result| {
                if let Some(response) = &result.response {
                    transactions.extend(response.transactions.iter().cloned());
                }
            },
        )
        .map_err(|error| error.to_string())?;
        Ok(transactions)
    });
    wait_for_socket_path(&socket_path)?;
    let output = run_compiled_xlib_probe(&display, "xlib-put-image", XLIB_PUT_IMAGE_SMOKE_SOURCE)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let status = output.status.code().unwrap_or(-1);
    let image_ops = xlib_smoke_field(&stdout, "image_ops").unwrap_or(0);

    let _ = std::fs::remove_file(&socket_path);
    let transactions = server
        .join()
        .map_err(|_| "X authority X11 socket server thread panicked")?
        .map_err(|error| format!("X authority X11 socket server failed: {error}"))?;
    let runtime_state = runtime_state_from_observed_transactions(&transactions)?;

    if !output.status.success() || image_ops != 2 {
        return Err(format!(
            "Xlib PutImage/GetImage smoke failed for {display}: status={status} image_ops={image_ops} stdout={} stderr={}",
            stdout.trim(),
            stderr.trim()
        )
        .into());
    }

    Ok(XAuthorityXlibPutImageSmokeReport {
        display,
        status,
        stdout_bytes: output.stdout.len(),
        stderr_bytes: output.stderr.len(),
        image_ops,
        transactions: transactions.len(),
        runtime_committed: runtime_state.authority_transactions_committed,
        runtime_surfaces: runtime_state.authority_surfaces_applied,
    })
}

fn run_x_authority_external_probe_smoke_spec(
    spec: &ExternalProbeSmokeSpec,
) -> Result<XAuthorityExternalProbeSmokeReport, Box<dyn std::error::Error>> {
    let command = resolve_external_probe_binary(spec.label, spec.binary)?;
    let (display, socket_path) = temp_xauthority_display(spec.display_base)?;
    run_x_authority_external_probe_smoke(ExternalProbeInvocation {
        label: spec.label,
        command: &command,
        display_mode: spec.display_mode,
        command_args: spec.args,
        extra_env: &[],
        display,
        socket_path,
        namespace: NamespaceId::from_raw(spec.namespace),
        require_transactions: spec.require_transactions,
        pixel_proof: spec.pixel_proof,
        allow_proof_kill_without_transactions: spec.allow_proof_kill_without_transactions,
        allow_client_failure_without_x_error: spec.allow_client_failure_without_x_error,
        render_device_provider: None,
        pixmap_allocator: None,
        proof_timeout: Duration::from_secs(spec.proof_timeout_secs),
        isolate_session_bus: false,
    })
}

fn run_x_authority_xmobar_smoke()
-> Result<XAuthorityExternalProbeSmokeReport, Box<dyn std::error::Error>> {
    let command = match std::env::var_os("SOPHIA_XMOBAR_BIN") {
        Some(path) => std::path::PathBuf::from(path),
        None => resolve_external_probe_binary("xmobar", "xmobar")?,
    };
    if !command.is_file() {
        return Err(format!("xmobar smoke executable does not exist: {}", command.display()).into());
    }
    let config = std::env::var_os("SOPHIA_XMOBAR_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or(std::env::current_dir()?.join("tools/fixtures/xmobar_sophia.config"));
    if !config.is_file() {
        return Err(format!("xmobar smoke config does not exist: {}", config.display()).into());
    }
    let config = config
        .to_str()
        .ok_or("xmobar smoke config path is not valid UTF-8")?;
    let (display, socket_path) = temp_xauthority_display(7_850)?;
    run_x_authority_external_probe_smoke(ExternalProbeInvocation {
        label: "xmobar",
        command: &command,
        display_mode: ExternalProbeDisplayMode::Environment,
        command_args: &[config],
        extra_env: &[],
        display,
        socket_path,
        namespace: NamespaceId::from_raw(62),
        require_transactions: true,
        pixel_proof: ExternalProbePixelProof::Nonzero,
        allow_proof_kill_without_transactions: false,
        allow_client_failure_without_x_error: false,
        render_device_provider: None,
        pixmap_allocator: None,
        proof_timeout: Duration::from_secs(8),
        isolate_session_bus: false,
    })
}

/// A shell panel, which is a class of client the matrix does not otherwise
/// prove.
///
/// xmobar already covers a bar, but covers a different one: it is
/// override-redirect and draws through CPU buffers. This is a dock -- it takes
/// `_NET_WM_WINDOW_TYPE_DOCK`, reserves work area with `_NET_WM_STRUT_PARTIAL`,
/// and renders through Qt's GL path, so it reaches DRI3 and Present against a
/// surface that outlives the frame. A live run of exactly this configuration
/// produced nine Present refusals, and this exists so the next one can be read
/// without spending a desktop to see it.
fn run_x_authority_quickshell_smoke()
-> Result<XAuthorityExternalProbeSmokeReport, Box<dyn std::error::Error>> {
    let command = match std::env::var_os("SOPHIA_QUICKSHELL_BIN") {
        Some(path) => std::path::PathBuf::from(path),
        None => resolve_external_probe_binary("quickshell", "quickshell")?,
    };
    if !command.is_file() {
        return Err(format!(
            "quickshell smoke executable does not exist: {}",
            command.display()
        )
        .into());
    }
    let config = std::env::var_os("SOPHIA_QUICKSHELL_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or(std::env::current_dir()?.join("tools/fixtures/quickshell_sophia/shell.qml"));
    if !config.is_file() {
        return Err(
            format!("quickshell smoke config does not exist: {}", config.display()).into(),
        );
    }
    let config = config
        .to_str()
        .ok_or("quickshell smoke config path is not valid UTF-8")?;
    // Both halves of DRI3, because the point of this probe is the GL path: Qt
    // renders through it, and without a device the client falls back long
    // before it reaches Present -- which is the request this exists to watch.
    let provider = Arc::new(ExternalProbeRenderDeviceProvider::measured(
        first_openable_render_node()?,
    )?);
    let (display, socket_path) = temp_xauthority_display(7_900)?;
    run_x_authority_external_probe_smoke(ExternalProbeInvocation {
        label: "quickshell",
        command: &command,
        display_mode: ExternalProbeDisplayMode::Environment,
        command_args: &["--path", config],
        extra_env: &[],
        display,
        socket_path,
        namespace: NamespaceId::from_raw(63),
        // No transaction is required, and the reason is the point of the probe
        // rather than a weakness in it: a dock stays unmapped until a policy
        // client admits it, and there is no window manager here. What this
        // proves is the protocol trace -- that Qt reaches GLX and DRI3 against
        // Sophia and that no refusal in it is the server's. Admission is the
        // session gate's to prove, as it is for vkcube.
        require_transactions: false,
        pixel_proof: ExternalProbePixelProof::None,
        // A shell does not exit; it is stopped at the deadline.
        allow_proof_kill_without_transactions: true,
        allow_client_failure_without_x_error: false,
        render_device_provider: Some(provider),
        pixmap_allocator: external_probe_pixmap_allocator()?,
        // A Qt cold start loads QML and builds a scene graph; eight seconds is
        // a terminal's budget, not a toolkit's.
        proof_timeout: Duration::from_secs(20),
        // Quickshell reads the session bus for tray and notification services.
        // It runs without one, and pointing it at a dead socket only adds noise
        // to a probe about the X wire.
        isolate_session_bus: false,
    })
}

/// The same shell, rendering in software rather than through GL.
///
/// A second variant of one binary rather than a change to the first, as
/// `zenity` and `zenity_render` already are, so the GL probe keeps proving the
/// GL path.
///
/// It does not prove MIT-SHM 1.2, and the reason is worth stating: Qt does not
/// allocate a backing store until something exposes its window, and offline
/// there is no policy client to map a dock. The trace reaches
/// `ShmQueryVersion` and stops. `x-authority-shm-fd-smoke` proves the segment
/// requests deterministically instead; what this adds is that the software
/// path draws no protocol error on the way.
fn run_x_authority_quickshell_software_smoke()
-> Result<XAuthorityExternalProbeSmokeReport, Box<dyn std::error::Error>> {
    let command = match std::env::var_os("SOPHIA_QUICKSHELL_BIN") {
        Some(path) => std::path::PathBuf::from(path),
        None => resolve_external_probe_binary("quickshell", "quickshell")?,
    };
    if !command.is_file() {
        return Err(format!(
            "quickshell smoke executable does not exist: {}",
            command.display()
        )
        .into());
    }
    let config = std::env::var_os("SOPHIA_QUICKSHELL_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or(std::env::current_dir()?.join("tools/fixtures/quickshell_sophia/shell.qml"));
    if !config.is_file() {
        return Err(
            format!("quickshell smoke config does not exist: {}", config.display()).into(),
        );
    }
    let config = config
        .to_str()
        .ok_or("quickshell smoke config path is not valid UTF-8")?;
    let (display, socket_path) = temp_xauthority_display(7_950)?;
    run_x_authority_external_probe_smoke(ExternalProbeInvocation {
        label: "quickshell_software",
        command: &command,
        display_mode: ExternalProbeDisplayMode::Environment,
        command_args: &["--path", config],
        extra_env: &[("QT_QUICK_BACKEND", "software")],
        display,
        socket_path,
        namespace: NamespaceId::from_raw(65),
        // As the GL variant: a dock is not mapped without a policy client.
        require_transactions: false,
        pixel_proof: ExternalProbePixelProof::None,
        allow_proof_kill_without_transactions: true,
        allow_client_failure_without_x_error: false,
        render_device_provider: None,
        pixmap_allocator: None,
        proof_timeout: Duration::from_secs(20),
        isolate_session_bus: false,
    })
}

/// The probe's allocator, when this build has a device to allocate from.
///
/// Without the live scanout feature there is no GPU allocation path compiled in,
/// and a probe that cannot originate a buffer still serves the client-allocated
/// half exactly as before.
#[cfg(feature = "native-session")]
fn external_probe_pixmap_allocator() -> Result<
    Option<Arc<dyn sophia_x_authority::XServerFrontendPixmapAllocator>>,
    Box<dyn std::error::Error>,
> {
    Ok(Some(Arc::new(ExternalProbePixmapAllocator {
        device: first_openable_render_node()?,
    })))
}

#[cfg(not(feature = "native-session"))]
fn external_probe_pixmap_allocator() -> Result<
    Option<Arc<dyn sophia_x_authority::XServerFrontendPixmapAllocator>>,
    Box<dyn std::error::Error>,
> {
    Ok(None)
}

/// Originates buffers for the probe, so an offline client can exercise the
/// half of DRI3 where the server owns the storage.
#[cfg(feature = "native-session")]
struct ExternalProbePixmapAllocator {
    device: std::fs::File,
}

#[cfg(feature = "native-session")]
impl sophia_x_authority::XServerFrontendPixmapAllocator for ExternalProbePixmapAllocator {
    fn allocate_pixmap_buffer(
        &self,
        request: sophia_x_authority::XServerFrontendPixmapAllocation,
    ) -> Result<
        sophia_x_authority::XServerFrontendAllocatedPixmap,
        sophia_x_authority::XServerFrontendPixmapAllocationError,
    > {
        use sophia_x_authority::XServerFrontendPixmapAllocationError as Error;

        let allocation = sophia_backend_live::allocate_shared_buffer(
            &self.device,
            request.handle,
            request.size,
            request.depth,
        )
        .map_err(|error| match error {
            sophia_backend_live::LiveSharedBufferError::UnsupportedTarget => {
                Error::UnsupportedTarget
            }
            sophia_backend_live::LiveSharedBufferError::DeviceRejected
            | sophia_backend_live::LiveSharedBufferError::ExportFailed => Error::AllocationFailed,
        })?;
        Ok(sophia_x_authority::XServerFrontendAllocatedPixmap {
            descriptor: allocation.descriptor,
            plane_fds: allocation.plane_fds,
        })
    }
}

struct ExternalProbeRenderDeviceProvider {
    device: std::fs::File,
    import_formats: Vec<sophia_x_authority::XServerFrontendDmaBufImportFormat>,
}

impl ExternalProbeRenderDeviceProvider {
    /// Measures the node's DMA-BUF import inventory once, before the frontend
    /// exists, the way a live session does.
    ///
    /// The authority answers `GetSupportedModifiers` from this inventory and
    /// nothing else. Handed none, mesa allocates without a modifier and
    /// imports through the single-plane `PixmapFromBuffer`, which is not the
    /// path a session runs and not the stage the GLX probes require. A node
    /// that cannot be measured is reported and advertised as nothing, as the
    /// session does, so a probe that needs the layouts says which stage it
    /// lost rather than failing to start.
    fn measured(device: std::fs::File) -> Result<Self, Box<dyn std::error::Error>> {
        let import_formats = match device
            .try_clone()
            .map_err(|_| sophia_backend_live::LiveDmaBufCapabilityError::DeviceUnavailable)
            .and_then(sophia_backend_live::query_dma_buf_import_formats)
        {
            Ok(formats) => formats
                .into_iter()
                .map(
                    |row| sophia_x_authority::XServerFrontendDmaBufImportFormat {
                        format: row.format,
                        modifiers: row.modifiers,
                    },
                )
                .collect(),
            Err(reason) => {
                eprintln!(
                    "sophia_x_authority_probe schema=1 status=degraded reason=dma_buf_import_capabilities_unavailable error={reason:?}"
                );
                Vec::new()
            }
        };
        Ok(Self {
            device,
            import_formats,
        })
    }
}

impl XServerFrontendRenderDeviceProvider for ExternalProbeRenderDeviceProvider {
    fn dma_buf_import_formats(&self) -> Vec<sophia_x_authority::XServerFrontendDmaBufImportFormat> {
        self.import_formats.clone()
    }

    fn open_render_device_fd(
        &self,
    ) -> Result<std::os::fd::OwnedFd, XServerFrontendRenderDeviceError> {
        use std::os::fd::AsRawFd as _;

        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!("/proc/self/fd/{}", self.device.as_raw_fd()))
            .map(std::os::fd::OwnedFd::from)
            .map_err(|_| XServerFrontendRenderDeviceError::OpenFailed)
    }
}

fn run_x_authority_zenity_render_smoke()
-> Result<XAuthorityExternalProbeSmokeReport, Box<dyn std::error::Error>> {
    let command = resolve_external_probe_binary("zenity_render", "zenity")?;
    let provider = Arc::new(ExternalProbeRenderDeviceProvider::measured(
        first_openable_render_node()?,
    )?);
    let (display, socket_path) = temp_xauthority_display(7760)?;
    run_x_authority_external_probe_smoke(ExternalProbeInvocation {
        label: "zenity_render",
        command: &command,
        display_mode: ExternalProbeDisplayMode::Environment,
        command_args: &[
            "--entry",
            "--title",
            "Sophia zenity render",
            "--text",
            "Sophia GTK render-provider probe",
        ],
        extra_env: &[],
        display,
        socket_path,
        namespace: NamespaceId::from_raw(60),
        require_transactions: true,
        pixel_proof: ExternalProbePixelProof::Nonzero,
        allow_proof_kill_without_transactions: false,
        allow_client_failure_without_x_error: false,
        render_device_provider: Some(provider),
        pixmap_allocator: None,
        proof_timeout: Duration::from_secs(8),
        isolate_session_bus: false,
    })
}

fn run_x_authority_vkcube_smoke()
-> Result<XAuthorityExternalProbeSmokeReport, Box<dyn std::error::Error>> {
    let command = resolve_external_probe_binary("vkcube", "vkcube")?;
    let provider = Arc::new(ExternalProbeRenderDeviceProvider::measured(
        first_openable_render_node()?,
    )?);
    let (display, socket_path) = temp_xauthority_display(6680)?;
    run_x_authority_external_probe_smoke(ExternalProbeInvocation {
        label: "vkcube",
        command: &command,
        display_mode: ExternalProbeDisplayMode::Environment,
        command_args: &["--wsi", "xcb", "--c", "2", "--suppress_popups"],
        extra_env: &[],
        display,
        socket_path,
        namespace: NamespaceId::from_raw(58),
        require_transactions: false,
        pixel_proof: ExternalProbePixelProof::None,
        allow_proof_kill_without_transactions: true,
        allow_client_failure_without_x_error: false,
        render_device_provider: Some(provider),
        pixmap_allocator: None,
        proof_timeout: Duration::from_secs(8),
        isolate_session_bus: false,
    })
}

fn run_x_authority_glxgears_smoke()
-> Result<XAuthorityExternalProbeSmokeReport, Box<dyn std::error::Error>> {
    let command = resolve_external_probe_binary("glxgears", "glxgears")?;
    let provider = Arc::new(ExternalProbeRenderDeviceProvider::measured(
        first_openable_render_node()?,
    )?);
    let (display, socket_path) = temp_xauthority_display(6685)?;
    run_x_authority_external_probe_smoke(ExternalProbeInvocation {
        label: "glxgears",
        command: &command,
        display_mode: ExternalProbeDisplayMode::Environment,
        command_args: &[
            "-info",
            "-swapinterval",
            "1",
            "-geometry",
            "500x500",
        ],
        extra_env: &[],
        display,
        socket_path,
        namespace: NamespaceId::from_raw(59),
        require_transactions: true,
        pixel_proof: ExternalProbePixelProof::None,
        allow_proof_kill_without_transactions: false,
        allow_client_failure_without_x_error: false,
        render_device_provider: Some(provider),
        pixmap_allocator: None,
        proof_timeout: Duration::from_secs(8),
        isolate_session_bus: false,
    })
}

/// Drives mesa-demos' `pbdemo` through a real GLX pbuffer.
///
/// The offscreen path a browser's GL layer takes to bootstrap a display, exercised
/// against real Mesa and DRI3 in seconds rather than a rig session. `pbdemo` takes
/// its extent and a source image as positional arguments and exits before opening
/// the display without them, so the image is written here rather than assumed.
fn run_x_authority_glx_pbuffer_smoke()
-> Result<XAuthorityExternalProbeSmokeReport, Box<dyn std::error::Error>> {
    let command = resolve_external_probe_binary("pbdemo", "pbdemo")?;
    let provider = Arc::new(ExternalProbeRenderDeviceProvider::measured(
        first_openable_render_node()?,
    )?);
    let image = std::env::temp_dir().join(format!("sophia-pbuffer-probe-{}.ppm", std::process::id()));
    // A 2x2 binary PPM: the smallest input pbdemo will accept.
    std::fs::write(
        &image,
        [
            b"P6\n2 2\n255\n".as_slice(),
            &[0xff, 0, 0, 0, 0xff, 0, 0, 0, 0xff, 0xff, 0xff, 0xff],
        ]
        .concat(),
    )?;
    let (display, socket_path) = temp_xauthority_display(6675)?;
    let report = run_x_authority_external_probe_smoke(ExternalProbeInvocation {
        label: "pbdemo",
        command: &command,
        display_mode: ExternalProbeDisplayMode::Environment,
        command_args: &["64", "64", &image.to_string_lossy()],
        extra_env: &[],
        display,
        socket_path,
        namespace: NamespaceId::from_raw(64),
        require_transactions: false,
        pixel_proof: ExternalProbePixelProof::None,
        allow_proof_kill_without_transactions: true,
        allow_client_failure_without_x_error: false,
        render_device_provider: Some(provider),
        pixmap_allocator: external_probe_pixmap_allocator()?,
        proof_timeout: Duration::from_secs(8),
        isolate_session_bus: false,
    });
    let _ = std::fs::remove_file(&image);
    report
}

/// Drives the session's configured browser against the authority offline.
///
/// The browser is the client the physical gate turns on, and until now the only
/// way to watch it speak to Sophia was to hold a rig session open. It gets a
/// render node because it starts a GPU process, a scratch profile because a
/// second instance otherwise hands its window to the first and exits, and a
/// deadline sized for a cold start rather than for a terminal.
fn run_x_authority_browser_smoke()
-> Result<XAuthorityExternalProbeSmokeReport, Box<dyn std::error::Error>> {
    let command = resolve_external_probe_binary("browser", "helium")?;
    let provider = Arc::new(ExternalProbeRenderDeviceProvider::measured(
        first_openable_render_node()?,
    )?);
    // Scratch profile, per process: a browser started against the operator's own
    // profile forwards to whatever instance is already running and exits before
    // it opens the display, which reads as a silent probe rather than a refusal.
    let profile = std::env::temp_dir().join(format!(
        "sophia-browser-probe-{}",
        std::process::id()
    ));
    let profile_arg = format!("--user-data-dir={}", profile.to_string_lossy());
    let (display, socket_path) = temp_xauthority_display(6710)?;
    let report = run_x_authority_external_probe_smoke(ExternalProbeInvocation {
        label: "browser",
        command: &command,
        display_mode: ExternalProbeDisplayMode::Environment,
        command_args: &[
            &profile_arg,
            // Chromium keeps its log to a file unless told otherwise; without
            // this the probe captures an empty stderr and a stall looks silent.
            "--enable-logging=stderr",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-networking",
            "about:blank",
        ],
        extra_env: &[],
        display,
        socket_path,
        namespace: NamespaceId::from_raw(65),
        require_transactions: true,
        pixel_proof: ExternalProbePixelProof::Nonzero,
        allow_proof_kill_without_transactions: false,
        allow_client_failure_without_x_error: false,
        render_device_provider: Some(provider),
        pixmap_allocator: external_probe_pixmap_allocator()?,
        proof_timeout: Duration::from_secs(30),
        isolate_session_bus: true,
    });
    let _ = std::fs::remove_dir_all(&profile);
    report
}

fn run_x_authority_kitty_smoke()
-> Result<XAuthorityExternalProbeSmokeReport, Box<dyn std::error::Error>> {
    let command = resolve_external_probe_binary("kitty", "kitty")?;
    let provider = Arc::new(ExternalProbeRenderDeviceProvider::measured(
        first_openable_render_node()?,
    )?);
    let (display, socket_path) = temp_xauthority_display(6690)?;
    run_x_authority_external_probe_smoke(ExternalProbeInvocation {
        label: "kitty",
        command: &command,
        display_mode: ExternalProbeDisplayMode::Environment,
        command_args: &[
            "--config",
            "NONE",
            "--override",
            "close_on_child_death=yes",
            "--title",
            "Sophia Kitty GLX proof",
            "sh",
            "-c",
            "printf 'Sophia Kitty proof\\n'; sleep 5",
        ],
        extra_env: &[],
        display,
        socket_path,
        namespace: NamespaceId::from_raw(62),
        require_transactions: true,
        pixel_proof: ExternalProbePixelProof::None,
        allow_proof_kill_without_transactions: false,
        allow_client_failure_without_x_error: false,
        render_device_provider: Some(provider),
        pixmap_allocator: None,
        proof_timeout: Duration::from_secs(20),
        isolate_session_bus: true,
    })
}
