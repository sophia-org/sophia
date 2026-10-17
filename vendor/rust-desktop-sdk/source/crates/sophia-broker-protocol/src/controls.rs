use crate::Invalid;
fn put(b: &mut [u8], at: usize, v: u64, n: usize) {
    b[at..at + n].copy_from_slice(&v.to_le_bytes()[..n]);
}
/// Revision ranges are domain values, including unsupported or inverted ranges.
/// All encoders leave output untouched on error.
pub fn encode_negotiate(
    epoch: u64,
    submission: u64,
    minimum: u16,
    maximum: u16,
    out: &mut [u8],
) -> Result<usize, Invalid> {
    if epoch == 0 || submission == 0 || out.len() < 40 {
        return Err(Invalid);
    }
    let b = &mut out[..40];
    b.fill(0);
    put(b, 0, 40, 4);
    put(b, 4, 1, 2);
    put(b, 6, 256, 2);
    put(b, 8, epoch, 8);
    put(b, 16, submission, 8);
    put(b, 32, u64::from(minimum), 2);
    put(b, 34, u64::from(maximum), 2);
    Ok(40)
}
/// Declared candidate length follows the contract's 40..=448 range; complete
/// candidate syntax is validated separately before custody.
pub fn encode_submit(
    epoch: u64,
    submission: u64,
    candidate_bytes: u32,
    out: &mut [u8],
) -> Result<usize, Invalid> {
    if epoch == 0 || submission == 0 || !(40..=448).contains(&candidate_bytes) || out.len() < 24 {
        return Err(Invalid);
    }
    let b = &mut out[..24];
    b.fill(0);
    put(b, 0, epoch, 8);
    put(b, 8, submission, 8);
    put(b, 16, u64::from(candidate_bytes), 4);
    Ok(24)
}
pub fn encode_ack(epoch: u64, sequence: u64, out: &mut [u8]) -> Result<usize, Invalid> {
    if epoch == 0 || sequence == 0 || out.len() < 16 {
        return Err(Invalid);
    }
    put(out, 0, epoch, 8);
    put(out, 8, sequence, 8);
    Ok(16)
}
