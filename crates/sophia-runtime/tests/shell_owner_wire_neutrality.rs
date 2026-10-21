//! The shell owners cannot reintroduce the retired socket frame codecs or
//! socket-shaped limits. There is no adapter exception after its removal.
use std::path::{Path, PathBuf};

/// Names that belong to the socket wire alone.
const SOCKET_ONLY: &[&str] = &[
    "SOPHIA_IPC_HEADER_LEN",
    "SOPHIA_IPC_MAX_PAYLOAD_LEN",
    "IpcMessageKind",
    "decode_frame",
    "encode_frame",
    "max_frame_payload",
    "max_input_queue_bytes",
    "encode_shell_tab_snapshot(",
    "decode_shell_tab_candidate(",
    "encode_shell_shortcut_catalog(",
    "encode_shell_reference_request(",
    "decode_shell_reference_candidate(",
    "encode_shell_reference_outcome(",
    "encode_shell_launcher_request(",
    "decode_shell_launcher_candidate(",
    "encode_shell_launcher_outcome(",
    "encode_shell_launcher_activation(",
    "decode_shell_launcher_activation_ack(",
    "encode_shell_launch_outcome(",
];

fn socket_frame_call(code: &str) -> bool {
    code.match_indices('(').any(|(offset, _)| {
        let prefix = code[..offset].trim_end();
        let name = prefix
            .rsplit(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .next()
            .unwrap_or_default();
        // Renderer snapshots expose a borrowed descriptor view, not a wire
        // codec. Keep rejecting every other frame helper in shell owners.
        name.ends_with("_frame") && name != "as_frame"
    })
}

#[test]
fn renderer_descriptor_views_are_not_socket_frame_codecs() {
    assert!(!socket_frame_call("let frame = snapshot.as_frame();"));
    assert!(socket_frame_call("encode_shell_content_frame (value)"));
    assert!(socket_frame_call("decode_frame(bytes)"));
    assert!(socket_frame_call("response_frame(value)"));
}

fn sources(directory: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn shell_owners_name_no_socket_framing() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![root.join("shell_transport.rs")];
    sources(&root.join("shell_transport"), &mut files);
    sources(&root.join("shell_content"), &mut files);
    let mut violations = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            if socket_frame_call(code) {
                violations.push(format!(
                    "{}:{}: socket frame helper",
                    path.strip_prefix(&root).unwrap().display(),
                    number + 1
                ));
            }
            for name in SOCKET_ONLY {
                if code.contains(name) {
                    violations.push(format!(
                        "{}:{}: {name}",
                        path.strip_prefix(&root).unwrap().display(),
                        number + 1
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "retired socket framing in shell owners:\n{}",
        violations.join("\n")
    );
    assert!(files.len() > 40, "the owner sources were found");
}
