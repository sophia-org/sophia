mod commands;
mod scanout_diagnostics;

use sophia_runtime::{TraceLevel, init_tracing_with_layer};

fn session_stdout(line: &str) {
    if !sophia_session::diagnostics::capture_line(line) {
        println!("{line}");
    }
}

fn session_stderr(line: &str) {
    if !sophia_session::diagnostics::capture_line(line) {
        eprintln!("{line}");
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let verbose = args.iter().any(|arg| arg == "-v" || arg == "--verbose");
    let level = if verbose {
        TraceLevel::Debug
    } else {
        TraceLevel::Info
    };

    init_tracing_with_layer(level, scanout_diagnostics::layer())?;
    sophia_session::install_session_output(sophia_session::SessionOutput::new(
        session_stdout,
        session_stderr,
    ))
    .map_err(std::io::Error::other)?;
    let is_run = args.first().map(String::as_str) == Some("session")
        && args.get(1).map(String::as_str) == Some("run")
        && !args.iter().any(|arg| arg == "--validate-session-args");
    let ordinary = is_run
        && args.iter().any(|arg| arg == "--session-mode=normal")
        && !args.iter().any(|arg| {
            let key = arg.split('=').next().unwrap_or(arg);
            key == "--proof" || key.contains("-proof") || key == "--max-runtime-ms"
        });
    let mut owned = None;
    let capture_path = if is_run {
        std::env::var_os("SOPHIA_DIAGNOSTIC_DIR").map(std::path::PathBuf::from)
    } else {
        None
    };
    let capture_path = if capture_path.is_none() && ordinary {
        match sophia_session::diagnostics::Store::from_environment().and_then(|store| {
            let identity = commands::diagnostics::launch_identity()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let record = store.begin("direct", std::process::id(), &identity)?;
            Ok((store, record))
        }) {
            Ok((store, record)) => {
                let path = record.path.clone();
                owned = Some((store, record.id));
                Some(path)
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(error.into());
            }
            Err(error) => {
                eprintln!("session diagnostics unavailable: {error}");
                None
            }
        }
    } else {
        capture_path
    };
    let capture =
        capture_path
            .as_deref()
            .and_then(
                |path| match sophia_session::diagnostics::Capture::start(path) {
                    Ok(capture) => Some(capture),
                    Err(error) => {
                        eprintln!("session diagnostics unavailable: {error}");
                        None
                    }
                },
            );
    let application_capture = if ordinary && capture.is_some() {
        capture_path.as_deref().and_then(|path| {
            match sophia_session::diagnostics::application::ApplicationCapture::start(path)
                .and_then(|capture| { capture.install()?; Ok(capture) }) {
                Ok(capture) => Some(capture),
                Err(_) => {
                    session_stderr("sophia_application_capture schema=1 status=unavailable reason=setup_failure");
                    None
                }
            }
        })
    } else {
        None
    };
    if capture.is_some() {
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |information| {
            if let Some(location) = information.location() {
                let source = std::path::Path::new(location.file())
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("unknown");
                sophia_session::diagnostics::capture_line(&format!(
                    "sophia_session_panic schema=1 status=failed source_file={source} source_line={}",
                    location.line()
                ));
            }
            previous_hook(information);
        }));
    }
    let result = commands::run(&args, verbose);
    if is_run && result.is_err() {
        session_stderr("sophia_session_result schema=1 status=failed");
    }
    drop(application_capture);
    drop(capture);
    if let Some((store, id)) = owned {
        let _ = store.finish(&id, Some(if result.is_ok() { 0 } else { 1 }));
    }
    result
}
