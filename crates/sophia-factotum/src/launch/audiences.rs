//! An immutable policy-selected set. A role name or client request cannot add
//! members after registration; changing the set requires a new launch.
use super::Refusal;

pub const MAX_AUDIENCES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Audiences {
    values: [u64; MAX_AUDIENCES],
    len: u8,
}

impl Audiences {
    pub fn new(values: &[u64]) -> Result<Self, Refusal> {
        if values.is_empty() || values.len() > MAX_AUDIENCES || values.contains(&0) {
            return Err(Refusal::Invalid);
        }
        let mut set = Self {
            values: [0; MAX_AUDIENCES],
            len: values.len() as u8,
        };
        set.values[..values.len()].copy_from_slice(values);
        set.values[..values.len()].sort_unstable();
        if set.as_slice().windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(Refusal::Invalid);
        }
        Ok(set)
    }
    pub fn as_slice(&self) -> &[u64] {
        &self.values[..usize::from(self.len)]
    }
    pub fn contains(&self, audience: u64) -> bool {
        self.as_slice().binary_search(&audience).is_ok()
    }
}
