fn parse_output_size(value: &str) -> Result<Size, Box<dyn std::error::Error>> {
    let (width, height) = value
        .split_once('x')
        .ok_or("--inject-output-size expects WIDTHxHEIGHT")?;
    let size = Size {
        width: width.parse()?,
        height: height.parse()?,
    };
    if size.width <= 0 || size.height <= 0 || size.width > 16_384 || size.height > 16_384 {
        return Err("--inject-output-size accepts dimensions from 1 through 16384".into());
    }
    Ok(size)
}

fn parse_surface_resize_sequence(
    value: &str,
) -> Result<Vec<Size>, Box<dyn std::error::Error>> {
    let values = value.split(',').collect::<Vec<_>>();
    if values.len() < 2 || values.len() > 32 {
        return Err("--inject-surface-resize-sequence accepts 2-32 sizes".into());
    }
    let mut sizes = Vec::with_capacity(values.len());
    for value in values {
        sizes.push(parse_output_size(value)?);
    }
    if sizes.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("--inject-surface-resize-sequence requires each adjacent size to change".into());
    }
    Ok(sizes)
}

fn open_randr_update_witness(
    socket_path: &std::path::Path,
    cookie: [u8; 16],
) -> Result<std::os::unix::net::UnixStream, Box<dyn std::error::Error>> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    let auth_name = b"MIT-MAGIC-COOKIE-1";
    let mut setup = Vec::with_capacity(48);
    setup.extend_from_slice(&[b'l', 0]);
    setup.extend_from_slice(&11u16.to_le_bytes());
    setup.extend_from_slice(&0u16.to_le_bytes());
    setup.extend_from_slice(&(auth_name.len() as u16).to_le_bytes());
    setup.extend_from_slice(&(cookie.len() as u16).to_le_bytes());
    setup.extend_from_slice(&[0, 0]);
    setup.extend_from_slice(auth_name);
    setup.resize((setup.len() + 3) & !3, 0);
    setup.extend_from_slice(&cookie);
    stream.write_all(&setup)?;
    stream.flush()?;

    let mut header = [0u8; 8];
    stream.read_exact(&mut header)?;
    if header[0] != 1 {
        return Err("RandR witness X11 setup was rejected".into());
    }
    let extra = usize::from(u16::from_le_bytes([header[6], header[7]])) * 4;
    let mut body = vec![0; extra];
    stream.read_exact(&mut body)?;

    let root = sophia_x_authority::X_SETUP_DEFAULT_ROOT;
    let mut select = Vec::with_capacity(12);
    select.extend_from_slice(&[
        sophia_x_authority::X_RANDR_MAJOR_OPCODE,
        sophia_x_authority::X_RANDR_SELECT_INPUT_MINOR_OPCODE,
    ]);
    select.extend_from_slice(&3u16.to_le_bytes());
    select.extend_from_slice(&root.to_le_bytes());
    select.extend_from_slice(&0x47u16.to_le_bytes());
    select.extend_from_slice(&[0, 0]);
    stream.write_all(&select)?;
    // A reply-producing core request is a deterministic barrier proving the
    // preceding void RandR selection was dispatched before Engine updates.
    stream.write_all(&[43, 0, 1, 0])?;
    stream.flush()?;
    let mut barrier = [0u8; 32];
    stream.read_exact(&mut barrier)?;
    if barrier[0] != 1 {
        return Err("RandR witness barrier request failed".into());
    }
    Ok(stream)
}

fn confirm_randr_update_witness(
    stream: &mut std::os::unix::net::UnixStream,
    size: Size,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut event = [0u8; 32];
    stream.read_exact(&mut event)?;
    if event[0] != sophia_x_authority::X_RANDR_FIRST_EVENT
        || u16::from_le_bytes([event[24], event[25]]) != u16::try_from(size.width)?
        || u16::from_le_bytes([event[26], event[27]]) != u16::try_from(size.height)?
    {
        return Err(format!("RandR witness received an unexpected update: {event:?}").into());
    }
    Ok(())
}

fn spawn_secondary_xterm(
    terminal: &std::path::Path,
    display: &str,
    xauthority: &std::path::Path,
    control_socket: Option<&std::path::Path>,
    inspection_socket: Option<&std::path::Path>,
    input_proof: Option<&str>,
) -> Result<Child, Box<dyn std::error::Error>> {
    let mut command = std::process::Command::new(terminal);
    crate::application_catalog::configure_host_application_environment(
        &mut command,
        control_socket,
        inspection_socket,
    );
    command
        .env("DISPLAY", display)
        .env("XAUTHORITY", xauthority)
        .args([
            "-cm",
            "-dc",
            "-geometry",
            "100x28+420+90",
            "-title",
            "Sophia Secondary Terminal",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if input_proof.is_some() {
        command.args(["-e", "sh", "-c", SECONDARY_POINTER_WITNESS_SCRIPT]);
    } else {
        command.args([
            "-e",
            "sh",
            "-c",
            "printf 'Sophia secondary terminal\\n'; sleep 300",
        ]);
    }
    Ok(command.spawn()?)
}

fn spawn_approved_application(
    program: &str,
    display: &str,
    xauthority: &std::path::Path,
    control_socket: Option<&std::path::Path>,
    inspection_socket: Option<&std::path::Path>,
) -> Result<Child, Box<dyn std::error::Error>> {
    let mut command = std::process::Command::new(program);
    crate::application_catalog::configure_host_application_environment(
        &mut command,
        control_socket,
        inspection_socket,
    );
    Ok(command
        .env("DISPLAY", display)
        .env("XAUTHORITY", xauthority)
        .env_remove("ENV")
        .env_remove("BASH_ENV")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?)
}

fn parse_display_number(display: &str) -> Result<u32, Box<dyn std::error::Error>> {
    let raw = display
        .strip_prefix(':')
        .filter(|raw| !raw.is_empty() && raw.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or_else(|| format!("invalid local X display {display:?}; expected :NUMBER"))?;
    let display_number = raw.parse::<u32>()?;
    if display_number > u16::MAX.into() {
        return Err(format!("X display number {display_number} exceeds u16").into());
    }
    Ok(display_number)
}

/// Prepare where this display's trusted listener binds, and answer that path.
///
/// THE SOCKET LIVES IN THE LAYOUT NOW, and the classic path is a symbolic link
/// to it. The layout is the thing a sandbox can mount one group of -- see
/// *Socket Directories* in `docs/namespaces-and-portals.md` -- and a listener
/// that binds outside it is a listener no group directory contains. The
/// trusted group is the first to move because it is the one that exists: a
/// confined group has nothing to bind yet, and a layout proved only by its own
/// controls is a layout nothing has used.
///
/// THE LINK IS AT THE CLASSIC PATH, NOT IN THE GROUP DIRECTORY. `connect`
/// follows it, so every client that names `/tmp/.X11-unix/X<n>` -- which is
/// every client outside a sandbox, and every tool in this repository -- reaches
/// the socket unchanged. A link the other way, inside a group directory, is
/// the escape the layout exists to refuse, and `verify_group` refuses it.
fn prepare_display_socket(
    classic: &std::path::Path,
    display_number: u32,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    std::fs::create_dir_all("/tmp/.X11-unix")?;
    let layout = socket_directories::SocketDirectoryLayout::prepare(display_number)?;
    let bind = layout.prepare_group(socket_directories::ClientGroup::Shared)?;
    // ASKED BEFORE THE SOCKET GOES IN, which is the moment that matters: what
    // this catches is something already waiting in the directory, and a
    // directory found holding a link named `X<n>` is one somebody prepared.
    // A stale socket from a previous run passes -- it wears the right name and
    // is no link -- and is cleared below.
    layout.verify_group(socket_directories::ClientGroup::Shared)?;
    // ASKED OF BOTH PATHS, because either can be the one still serving. A live
    // socket under the layout with no link at the classic path is a session
    // this one would otherwise bind straight over.
    for path in [classic, bind.as_path()] {
        if UnixStream::connect(path).is_ok() {
            return Err(format!("X display socket {} is already active", path.display()).into());
        }
    }
    // `symlink_metadata`, not `exists`: a dangling link left by a previous
    // session answers false to `exists` and would then be in the way of the
    // one made below.
    for path in [classic, bind.as_path()] {
        if std::fs::symlink_metadata(path).is_ok() {
            std::fs::remove_file(path)?;
        }
    }
    std::os::unix::fs::symlink(&bind, classic)?;
    Ok(bind)
}

fn resolve_executable_on_path(name: &str) -> Option<std::path::PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&paths) {
        let candidate = directory.join(name);
        if candidate.is_file()
            && candidate
                .metadata()
                .is_ok_and(|metadata| metadata.mode() & 0o111 != 0)
        {
            return Some(candidate);
        }
    }
    None
}

fn application_client_command(client: &str) -> std::process::Command {
    // GTK clients finalize through a session bus. On a bare text TTY no bus
    // address exists; without one a toolkit can destroy its window but never
    // exit, which previously stranded the post-proof completion path. Give
    // application-proof clients a bounded per-client bus when the host
    // provides dbus-run-session; the bus exits with the client.
    if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none()
        && let Some(runner) = resolve_executable_on_path("dbus-run-session")
    {
        let mut command = std::process::Command::new(runner);
        command.arg("--").arg(client);
        return command;
    }
    std::process::Command::new(client)
}

fn wait_for_x_server_socket(
    path: &std::path::Path,
    server: &mut Option<
        std::thread::JoinHandle<Result<(), sophia_x_authority::X11SetupSocketError>>,
    >,
) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if path.exists() {
            return Ok(());
        }
        if server
            .as_ref()
            .is_some_and(std::thread::JoinHandle::is_finished)
        {
            return match server.take().expect("checked above").join() {
                Ok(Ok(())) => Err("X Server Frontend exited before creating its socket".into()),
                Ok(Err(error)) => Err(format!(
                    "X Server Frontend failed before creating {}: {error}",
                    path.display()
                )
                .into()),
                Err(_) => Err("X Server Frontend panicked before creating its socket".into()),
            };
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Err(format!(
        "timed out waiting for X authority socket {}",
        path.display()
    )
    .into())
}
