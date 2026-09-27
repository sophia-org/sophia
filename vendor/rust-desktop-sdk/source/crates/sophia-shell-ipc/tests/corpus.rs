//! The socket-wire codec against Sophia's golden shell corpus, as pinned in
//! `spec/golden`. Sophia's gate checks those copies against its own and runs
//! a byte-for-byte parity test of this crate against its codec.
use sophia_shell_ipc::*;

fn corpus(name: &str) -> Vec<(String, Vec<u8>)> {
    let path = format!("{}/../../spec/golden/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{path}: {error}"))
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let label = line.split(['|', ' ']).next().unwrap().to_owned();
            let hex = line.rsplit(['|', ' ']).next().unwrap();
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
                .collect();
            (label, bytes)
        })
        .collect()
}

fn frames(entries: &[(String, Vec<u8>)]) -> Vec<Vec<u8>> {
    entries.iter().map(|(_, bytes)| bytes.clone()).collect()
}

#[test]
fn content_frames_decode_and_reencode_exactly() {
    let entries = corpus("sophia-shell-content.frames");
    assert!(!entries.is_empty());
    for (label, frame) in entries {
        let (transaction, record) =
            decode_shell_content_frame(&frame).unwrap_or_else(|error| panic!("{label}: {error:?}"));
        assert_eq!(
            encode_shell_content_frame(transaction, &record).unwrap(),
            frame,
            "{label}"
        );
    }
}

#[test]
fn malformed_content_frames_are_refused() {
    let entries = corpus("sophia-shell-content-malformed.frames");
    assert!(!entries.is_empty());
    for (label, frame) in entries {
        assert!(
            decode_shell_content_frame(&frame).is_err(),
            "accepted {label}"
        );
    }
}

#[test]
fn catalog_action_frames_decode_and_reencode_exactly() {
    let entries = corpus("sophia-shell-catalog-actions.frames");
    assert!(!entries.is_empty());
    for (label, frame) in entries {
        let (transaction, record) = decode_shell_catalog_action_frame(&frame)
            .unwrap_or_else(|error| panic!("{label}: {error:?}"));
        assert_eq!(
            encode_shell_catalog_action_frame(transaction, &record).unwrap(),
            frame,
            "{label}"
        );
    }
}

#[test]
fn indicator_transactions_and_activations_decode_and_reencode_exactly() {
    let all = frames(&corpus("sophia-shell-indicators.frames"));
    for run in [&all[0..6], &all[6..8]] {
        let (transaction, snapshot) = decode_shell_indicator_snapshot(run).unwrap();
        assert_eq!(
            encode_shell_indicator_snapshot(transaction, &snapshot).unwrap(),
            run
        );
    }
    let (transaction, activation) = decode_shell_indicator_activation(&all[8]).unwrap();
    assert_eq!(
        encode_shell_indicator_activation(transaction, &activation).unwrap(),
        all[8]
    );
    for frame in &all[9..] {
        let (transaction, outcome) = decode_shell_indicator_activation_outcome(frame).unwrap();
        assert_eq!(
            &encode_shell_indicator_activation_outcome(transaction, &outcome).unwrap(),
            frame
        );
    }
}

#[test]
fn the_application_catalog_transaction_decodes_and_reencodes_exactly() {
    let all = frames(&corpus("sophia-shell-launcher.frames"));
    let run = &all[0..5];
    let (transaction, catalog) = decode_shell_application_catalog(run).unwrap();
    assert_eq!(
        encode_shell_application_catalog(transaction, &catalog).unwrap(),
        run
    );
}

#[test]
fn the_handshake_frames_decode_and_reencode_exactly() {
    let entries = corpus("sophia-shell-v1.frames");
    let find = |label: &str| {
        entries
            .iter()
            .find(|(name, _)| name == label)
            .map(|(_, bytes)| bytes.clone())
            .unwrap()
    };
    let hello = find("client_hello");
    let decoded = decode_shell_v1_client_hello_frame(&hello).unwrap();
    assert_eq!(encode_shell_v1_client_hello_frame(decoded).unwrap(), hello);
    let welcome = find("server_welcome");
    let decoded = decode_shell_v1_server_welcome_frame(&welcome).unwrap();
    assert_eq!(
        encode_shell_v1_server_welcome_frame(decoded).unwrap(),
        welcome
    );
}

#[test]
fn malformed_handshake_and_header_frames_are_refused() {
    let mut checked = 0;
    for (label, frame) in corpus("sophia-shell-v1-malformed.frames") {
        let refused = match label.as_str() {
            "hello_nonzero_transaction" => decode_shell_v1_client_hello_frame(&frame).is_err(),
            "welcome_reserved_nonzero" => decode_shell_v1_server_welcome_frame(&frame).is_err(),
            "truncated_header"
            | "bad_magic"
            | "bad_frame_version"
            | "payload_too_large"
            | "header_reserved_nonzero"
            | "trailing_frame_byte" => decode_frame(&frame).is_err(),
            _ => continue,
        };
        assert!(refused, "accepted {label}");
        checked += 1;
    }
    assert_eq!(checked, 8);
}
