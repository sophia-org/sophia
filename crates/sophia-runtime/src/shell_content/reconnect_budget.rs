use sophia_protocol::ContentLimits;

use super::ContentStoreError;

/// A measured registry inventory, not an additional reservation or ownership
/// ledger. Source bytes and conservative resource backing credit are distinct
/// from native head buffers and driver memory.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ContentReconnectBudget {
    pub capacity_bytes: u64,
    pub capacity_backing_bytes: u64,
    pub reserved_bytes: u64,
    pub reserved_backing_bytes: u64,
    pub own_retired_bytes: u64,
    pub own_retired_epochs: usize,
    pub active_epochs: usize,
    pub retired_epochs: usize,
}

/// Shared numeric decision inputs for allowance selection and host evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ContentReconnectAllowance {
    pub available_bytes: u64,
    pub available_backing_bytes: u64,
    pub required_bytes: u64,
    pub required_backing_bytes: u64,
}

pub fn content_reconnect_allowance(
    nominal: &ContentLimits,
    budget: ContentReconnectBudget,
) -> Result<ContentReconnectAllowance, ContentStoreError> {
    nominal
        .validate()
        .map_err(|_| ContentStoreError::Malformed)?;
    if nominal.max_session_retiring_bytes != budget.capacity_bytes {
        return Err(ContentStoreError::Budget);
    }
    let total = nominal.max_staging_bytes + nominal.max_resident_bytes + nominal.max_retiring_bytes;
    let source_free = budget.capacity_bytes.saturating_sub(budget.reserved_bytes);
    let backing_free = budget
        .capacity_backing_bytes
        .saturating_sub(budget.reserved_backing_bytes);
    let align = |bytes: u64| bytes / 4 * 4;
    let available = align(
        total
            .saturating_sub(budget.own_retired_bytes)
            .min(source_free),
    );
    let backing =
        align((nominal.max_resident_bytes + nominal.max_retiring_bytes).min(backing_free));
    // An overdrawn inventory has zero useful allowance, never wrapped credit.
    Ok(ContentReconnectAllowance {
        available_bytes: available,
        available_backing_bytes: backing,
        required_bytes: 4 * nominal.max_resource_bytes,
        required_backing_bytes: 3 * nominal.max_resource_bytes,
    })
}

/// Choose initial allowances within a fixed nominal profile envelope. The
/// caller must bind each profile to one immutable role and allow only one
/// active grant per role; a storage profile is not itself admission authority.
/// This neither reserves storage nor edits an active grant. The registry's
/// normal serialized admission must still succeed before publication.
pub fn select_reconnect_limits(
    nominal: &ContentLimits,
    budget: ContentReconnectBudget,
) -> Result<ContentLimits, ContentStoreError> {
    let allowance = content_reconnect_allowance(nominal, budget)?;
    let available = allowance.available_bytes;
    let backing = allowance.available_backing_bytes;
    let align = |bytes: u64| bytes / 4 * 4;
    let minimum = nominal.max_resource_bytes;
    // One full-size upload can replace one resident image without requiring
    // that displayed image to retire first. This is not arbitrary-output capacity.
    if available < allowance.required_bytes || backing < allowance.required_backing_bytes {
        return Err(ContentStoreError::Budget);
    }
    let resident = align(
        nominal
            .max_resident_bytes
            .min(available - 2 * minimum)
            .min(backing - minimum),
    );
    let staging = align(
        nominal
            .max_staging_bytes
            .min(available - resident - minimum),
    );
    let retiring = align(
        nominal
            .max_retiring_bytes
            .min(available - resident - staging)
            .min(backing - resident),
    );
    if staging < minimum || resident < 2 * minimum || retiring < minimum {
        return Err(ContentStoreError::Budget);
    }
    let mut limits = nominal.clone();
    limits.max_staging_bytes = staging;
    limits.max_resident_bytes = resident;
    limits.max_retiring_bytes = retiring;
    limits
        .validate()
        .map_err(|_| ContentStoreError::Malformed)?;
    Ok(limits)
}
