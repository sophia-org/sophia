//! Bounded uniqueness checks without heap allocation or quadratic scans.
pub(super) fn unique<const N: usize>(values: impl IntoIterator<Item = u64>) -> bool {
    let mut scratch = [0u64; N];
    let mut length = 0;
    for value in values {
        if length == N {
            return false;
        }
        scratch[length] = value;
        length += 1;
    }
    let values = &mut scratch[..length];
    values.sort_unstable();
    values.windows(2).all(|pair| pair[0] != pair[1])
}
