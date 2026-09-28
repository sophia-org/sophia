// Socket frames of the neutral content records; the records themselves are
// in tests/support/shell_content_records.rs.
include!("../../tests/support/shell_content_records.rs");

pub fn frames() -> Vec<Vec<u8>> {
    fixtures()
        .iter()
        .enumerate()
        .map(|(i, record)| {
            encode_shell_content_frame(
                TransactionId::from_raw(if i < 2 { 0 } else { i as u64 }),
                record,
            )
            .unwrap()
        })
        .collect()
}

/// Mutations use fixed wire offsets from the ADR, not the Rust field layout.
pub fn malformed() -> Vec<(String, Vec<u8>)> {
    let frames = frames();
    let mut result = Vec::new();
    for (i, bytes) in frames.iter().enumerate() {
        let mut truncated = bytes.clone();
        truncated.pop();
        result.push((format!("truncated-{}", i + 160), truncated));
        let mut trailing = bytes.clone();
        trailing.push(0);
        let n = (trailing.len() - 24) as u32;
        trailing[16..20].copy_from_slice(&n.to_le_bytes());
        result.push((format!("trailing-{}", i + 160), trailing));
        if i > 0 {
            let mut epoch = bytes.clone();
            epoch[24..32].fill(0);
            result.push((format!("zero-epoch-{}", i + 160), epoch));
        }
    }
    for (name, mask) in [
        ("mask-zero", 0u64),
        ("mask-wrong-singleton", 2),
        ("mask-second-bit", 3),
    ] {
        let mut bytes = frames[1].clone();
        bytes[88..96].copy_from_slice(&mask.to_le_bytes());
        result.push((name.to_owned(), bytes));
    }
    let mut format = frames[5].clone();
    format[72..74].copy_from_slice(&2u16.to_le_bytes());
    result.push(("unadmitted-format".to_owned(), format));
    let mut count = frames[13].clone();
    count[52..56].copy_from_slice(&u32::MAX.to_le_bytes());
    result.push(("surface-count-overflow".to_owned(), count));
    let mut reserved = frames[5].clone();
    reserved[74] = 1;
    result.push(("resource-reserved".to_owned(), reserved));
    result
}
