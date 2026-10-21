//! Development harness: real 9P and resource owners, supplied authentication,
//! simulated backend completions. Does not launch or replace a live shell.
#[path = "../tests/shell_draw.rs"]
mod panel;
fn main() {
    let mut args = std::env::args_os().skip(1);
    let preview = args.next().map(std::path::PathBuf::from);
    if args.next().is_some() {
        eprintln!("usage: vello_draw_panel [preview.ppm]");
        std::process::exit(2);
    }
    panel::paced_draw_panel(preview.as_deref());
    if let Some(path) = preview {
        println!("Rendered resource preview: {}", path.display());
    }
    println!("Passed: 64 frames, retained panel, 8 value generations; headless fixture only.");
}
