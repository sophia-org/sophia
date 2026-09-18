#[path = "support/catalog_action_fixtures.rs"]
mod fixtures;
use sophia_protocol::{
    TransactionId, decode_shell_catalog_action_frame, encode_shell_catalog_action_frame,
};
fn emit(label: &str, bytes: &[u8]) {
    print!("{label} ");
    for byte in bytes {
        print!("{byte:02x}");
    }
    println!();
}
fn main() {
    let mutations = std::env::args().nth(1).as_deref() == Some("--mutations");
    for (index, record) in fixtures::records().iter().enumerate() {
        let bytes = encode_shell_catalog_action_frame(TransactionId::from_raw(25), record).unwrap();
        if !mutations {
            emit(&format!("catalog-action-{}", 187 + index), &bytes);
            continue;
        }
        for offset in 24..bytes.len() {
            for value in [0, 1, 127, 255] {
                if bytes[offset] == value {
                    continue;
                }
                let mut changed = bytes.clone();
                changed[offset] = value;
                let expected = if decode_shell_catalog_action_frame(&changed).is_ok() {
                    "accept"
                } else {
                    "reject"
                };
                emit(&format!("{expected}-{index}-{offset}-{value}"), &changed);
            }
        }
    }
}
