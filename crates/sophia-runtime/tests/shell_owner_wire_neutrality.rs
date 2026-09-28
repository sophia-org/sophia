//! The shell owners stay wire-neutral: only the socket adapter
//! (`shell_transport/socket.rs` and `shell_transport/socket/`) may name the
//! IPC frame header, its message kinds, frame codecs or the socket-shaped
//! limits. Deleting that adapter must not require editing an owner.
use std::path::{Path, PathBuf};

/// Names that belong to the socket wire alone.
const SOCKET_ONLY: &[&str] = &[
    "SOPHIA_IPC_HEADER_LEN",
    "SOPHIA_IPC_MAX_PAYLOAD_LEN",
    "IpcMessageKind",
    "decode_frame",
    "encode_frame",
    "_frame(",
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

fn socket_adapter(path: &Path) -> bool {
    path.components().any(|part| part.as_os_str() == "socket")
        || path.file_name().is_some_and(|name| name == "socket.rs")
}

#[test]
fn shell_owners_name_no_socket_framing_outside_the_socket_adapter() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![root.join("shell_transport.rs")];
    sources(&root.join("shell_transport"), &mut files);
    sources(&root.join("shell_content"), &mut files);
    let mut violations = Vec::new();
    for path in files.iter().filter(|path| !socket_adapter(path)) {
        let text = std::fs::read_to_string(path).unwrap();
        for (number, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
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
        "socket framing outside the socket adapter:\n{}",
        violations.join("\n")
    );
    assert!(files.len() > 40, "the owner sources were found");
}
