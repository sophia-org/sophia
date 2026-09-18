//! Kernel-bound activation probe for the contained host's entry tests.
use std::io::Read;
use std::os::unix::net::UnixStream;

fn run() -> Result<(), String> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let mut activation = None;
    let mut control = None;
    let mut outside = None;
    for pair in arguments.chunks(2) {
        let [name, value] = pair else {
            return Err("options require values".into());
        };
        match name.as_str() {
            "--activation-fd" if activation.is_none() => {
                activation = Some(value.parse::<i32>().map_err(|e| e.to_string())?)
            }
            "--control-fd" if control.is_none() => {
                control = Some(value.parse::<i32>().map_err(|e| e.to_string())?)
            }
            "--outside" if outside.is_none() => outside = Some(value.clone()),
            _ => return Err("unknown or duplicate option".into()),
        }
    }
    let mut activation = sophia_conformance::private_instance::validate_entry(
        activation.ok_or("runner activation required")?,
    )?;
    let mut delegated = String::new();
    if let Some(fd) = control {
        activation
            .take_pipe(fd)?
            .take(4097)
            .read_to_string(&mut delegated)
            .map_err(|e| e.to_string())?;
    }
    let outside_connected = outside
        .as_ref()
        .is_some_and(|path| UnixStream::connect(path).is_ok());
    println!(
        "{}",
        serde_json::json!({"activated":true,"namespaces":activation.namespaces(),
        "delegated":delegated,"outside_connected":outside_connected})
    );
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("private-instance-probe: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
