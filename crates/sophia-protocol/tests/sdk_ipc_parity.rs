//! The Rust desktop SDK's `sophia-shell-ipc` carries a copy of this crate's
//! shell frame codecs for the socket wire's rollback path. This holds the copy
//! to Sophia's own codecs, byte for byte and error for error, over the
//! authoritative golden corpus and every truncation and single-byte change of
//! each corpus frame, so error precedence is compared as well as values.
use sophia_protocol as sophia;
use sophia_shell_ipc as sdk;

/// One outcome in comparable form: the decoded value, or the error. The two
/// crates' `IpcCodecError`s are distinct types with the same variants, so
/// they compare by their `Debug` text.
fn outcome<T: std::fmt::Debug, E: std::fmt::Debug>(result: Result<T, E>) -> String {
    match result {
        Ok(value) => format!("ok {value:?}"),
        Err(error) => format!("err {error:?}"),
    }
}

fn corpus(name: &str) -> Vec<Vec<u8>> {
    let path = format!(
        "{}/../../protocol/golden/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{path}: {error}"))
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let hex = line.rsplit(['|', ' ']).next().unwrap();
            (0..hex.len())
                .step_by(2)
                .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
                .collect()
        })
        .collect()
}

/// The frame itself, every proper prefix, and every single-byte change.
fn variants(frame: &[u8]) -> Vec<Vec<u8>> {
    let mut out = vec![frame.to_vec()];
    out.extend((0..frame.len()).map(|length| frame[..length].to_vec()));
    for at in 0..frame.len() {
        for flip in [0x01, 0x80, 0xff] {
            let mut changed = frame.to_vec();
            changed[at] ^= flip;
            out.push(changed);
        }
    }
    out
}

/// Every single-frame decoder both sides carry, each frame and variant, and
/// re-encoding of every accepted value.
fn single_frame_parity(frame: &[u8]) {
    let content = outcome(sophia::decode_shell_content_frame(frame));
    assert_eq!(content, outcome(sdk::decode_shell_content_frame(frame)));
    if let Ok((transaction, record)) = sophia::decode_shell_content_frame(frame) {
        assert_eq!(
            outcome(sophia::encode_shell_content_frame(transaction, &record)),
            outcome(sdk::encode_shell_content_frame(transaction, &record)),
        );
    }
    assert_eq!(
        outcome(sophia::decode_shell_catalog_action_frame(frame)),
        outcome(sdk::decode_shell_catalog_action_frame(frame)),
    );
    if let Ok((transaction, record)) = sophia::decode_shell_catalog_action_frame(frame) {
        assert_eq!(
            outcome(sophia::encode_shell_catalog_action_frame(
                transaction,
                &record
            )),
            outcome(sdk::encode_shell_catalog_action_frame(transaction, &record)),
        );
    }
    assert_eq!(
        outcome(sophia::decode_shell_indicator_activation(frame)),
        outcome(sdk::decode_shell_indicator_activation(frame)),
    );
    if let Ok((transaction, activation)) = sophia::decode_shell_indicator_activation(frame) {
        assert_eq!(
            outcome(sophia::encode_shell_indicator_activation(
                transaction,
                &activation
            )),
            outcome(sdk::encode_shell_indicator_activation(
                transaction,
                &activation
            )),
        );
    }
    assert_eq!(
        outcome(sophia::decode_shell_indicator_activation_outcome(frame)),
        outcome(sdk::decode_shell_indicator_activation_outcome(frame)),
    );
    if let Ok((transaction, value)) = sophia::decode_shell_indicator_activation_outcome(frame) {
        assert_eq!(
            outcome(sophia::encode_shell_indicator_activation_outcome(
                transaction,
                &value
            )),
            outcome(sdk::encode_shell_indicator_activation_outcome(
                transaction,
                &value
            )),
        );
    }
    assert_eq!(
        outcome(sophia::decode_shell_v1_client_hello_frame(frame)),
        outcome(sdk::decode_shell_v1_client_hello_frame(frame)),
    );
    if let Ok(hello) = sophia::decode_shell_v1_client_hello_frame(frame) {
        assert_eq!(
            outcome(sophia::encode_shell_v1_client_hello_frame(hello)),
            outcome(sdk::encode_shell_v1_client_hello_frame(hello)),
        );
    }
    assert_eq!(
        outcome(sophia::decode_shell_v1_server_welcome_frame(frame)),
        outcome(sdk::decode_shell_v1_server_welcome_frame(frame)),
    );
    if let Ok(welcome) = sophia::decode_shell_v1_server_welcome_frame(frame) {
        assert_eq!(
            outcome(sophia::encode_shell_v1_server_welcome_frame(welcome)),
            outcome(sdk::encode_shell_v1_server_welcome_frame(welcome)),
        );
    }
}

/// Every multi-frame transaction decoder over every contiguous run of the
/// corpus, then over each run with one frame truncated or changed.
fn multi_frame_parity(frames: &[Vec<u8>]) {
    let compare = |run: &[Vec<u8>]| {
        assert_eq!(
            outcome(sophia::decode_shell_indicator_snapshot(run)),
            outcome(sdk::decode_shell_indicator_snapshot(run)),
        );
        if let Ok((transaction, snapshot)) = sophia::decode_shell_indicator_snapshot(run) {
            assert_eq!(
                outcome(sophia::encode_shell_indicator_snapshot(
                    transaction,
                    &snapshot
                )),
                outcome(sdk::encode_shell_indicator_snapshot(transaction, &snapshot)),
            );
        }
        assert_eq!(
            outcome(sophia::decode_shell_application_catalog(run)),
            outcome(sdk::decode_shell_application_catalog(run)),
        );
        if let Ok((transaction, catalog)) = sophia::decode_shell_application_catalog(run) {
            assert_eq!(
                outcome(sophia::encode_shell_application_catalog(
                    transaction,
                    &catalog
                )),
                outcome(sdk::encode_shell_application_catalog(transaction, &catalog)),
            );
        }
        assert_eq!(
            outcome(sophia::decode_shell_persistent_catalog(run)),
            outcome(sdk::decode_shell_persistent_catalog(run)),
        );
    };
    for start in 0..frames.len() {
        for end in start + 1..=frames.len() {
            let run = &frames[start..end];
            compare(run);
            for index in 0..run.len() {
                for variant in variants(&run[index]).into_iter().skip(1).step_by(7) {
                    let mut changed = run.to_vec();
                    changed[index] = variant;
                    compare(&changed);
                }
            }
        }
    }
}

const SHELL_CORPORA: &[&str] = &[
    "sophia-shell-content.frames",
    "sophia-shell-content-malformed.frames",
    "sophia-shell-catalog-actions.frames",
    "sophia-shell-indicators.frames",
    "sophia-shell-launcher.frames",
    "sophia-shell-reference.frames",
    "sophia-shell-v1.frames",
    "sophia-shell-v1-malformed.frames",
];

#[test]
fn single_frame_codecs_match_over_the_corpus_and_every_variant() {
    for name in SHELL_CORPORA {
        let frames = corpus(name);
        assert!(!frames.is_empty(), "{name} is empty");
        for frame in &frames {
            for variant in variants(frame) {
                single_frame_parity(&variant);
            }
        }
        // Parity alone would pass if both sides refused everything.
        let accepted = frames
            .iter()
            .filter(|frame| {
                sdk::decode_shell_content_frame(frame).is_ok()
                    || sdk::decode_shell_catalog_action_frame(frame).is_ok()
                    || sdk::decode_shell_indicator_activation(frame).is_ok()
                    || sdk::decode_shell_indicator_activation_outcome(frame).is_ok()
                    || sdk::decode_shell_v1_client_hello_frame(frame).is_ok()
                    || sdk::decode_shell_v1_server_welcome_frame(frame).is_ok()
            })
            .count();
        let expected = match *name {
            "sophia-shell-content.frames" | "sophia-shell-catalog-actions.frames" => frames.len(),
            "sophia-shell-indicators.frames" => 5,
            "sophia-shell-v1.frames" => 2,
            _ => 0,
        };
        assert_eq!(accepted, expected, "{name}");
    }
}

#[test]
fn transaction_codecs_match_over_every_run_of_the_corpus() {
    for name in [
        "sophia-shell-indicators.frames",
        "sophia-shell-launcher.frames",
        "sophia-shell-reference.frames",
    ] {
        multi_frame_parity(&corpus(name));
    }
}

#[test]
fn the_welcome_bounds_match_the_session_vocabulary() {
    assert_eq!(
        sdk::SOPHIA_SHELL_V1_MAX_DESCRIPTORS,
        sophia::SOPHIA_SHELL_MAX_DESCRIPTORS
    );
    assert_eq!(
        sdk::SOPHIA_SHELL_V1_MAX_LABEL_BYTES,
        sophia::MAX_CHROME_LABEL_LEN
    );
}
