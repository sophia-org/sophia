//! Read-only inspection of captured WM file records, never a live reader.
#[path = "support/wm_file_inspection.rs"]
mod inspection;

use std::io::Write;

fn run() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments == ["--help"] {
        println!(
            "wm_file_inspect snapshot|events --path=CAPTURE --epoch=NONZERO --capabilities=0xMASK\nReads one captured Snapshot or a captured journal window (at most 1 MiB/64 records).\nEpoch/capabilities are unauthenticated supplied context. Unknown mask bits are preserved.\nNo live connection, acknowledgements, completeness or phase inference."
        );
        return Ok(());
    }
    let options = inspection::Options::parse(&arguments)?;
    if !std::fs::metadata(&options.path)
        .map_err(|e| format!("capture metadata: {e}"))?
        .is_file()
    {
        return Err("capture must be a regular file".into());
    }
    let file = std::fs::File::open(&options.path).map_err(|e| format!("open capture: {e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("opened capture is not a regular file".into());
    }
    let bytes = inspection::read_bounded(file)?;
    // No success output until the entire capture, including its last body,
    // passes the existing public decoders. File reads are not atomic custody.
    let capture = inspection::inspect(&bytes, options.mode, options.epoch, options.capabilities)?;
    std::io::stdout()
        .lock()
        .write_all(capture.text().as_bytes())
        .map_err(|e| format!("write stdout: {e}"))
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("WM capture refused: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
