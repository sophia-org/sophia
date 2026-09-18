/// A bounded scan over source-owned shared-activation receipts. The cursor
/// travels with the terminal inventory; a new source effect restarts the work
/// count without resetting its position and starving later records.
#[cfg(unix)]
#[derive(Default)]
struct PrivateSharedActivationScan {
    next: usize,
    remaining: usize,
    changed: bool,
}

#[cfg(unix)]
impl PrivateSharedActivationScan {
    fn invalidate(&mut self) {
        self.changed = true;
    }

    fn pending(&self) -> bool {
        self.changed || self.remaining != 0
    }

    fn visit(&mut self, releases: &mut [PrivateSettlingRelease]) -> Option<(usize, usize)> {
        const PAIRS_PER_VISIT: usize = 8;
        if releases.len() < 2 {
            self.remaining = 0;
            self.changed = false;
            return None;
        }
        let pairs = releases.len().checked_mul(releases.len())
            .expect("native records have a fixed pre-exposure capacity");
        if self.changed {
            self.remaining = pairs;
            self.changed = false;
        }
        if pairs == 0 {
            self.remaining = 0;
        }
        if self.remaining == 0 {
            return None;
        }
        let mut observed = 0;
        let mut joined = 0;
        while observed < PAIRS_PER_VISIT && self.remaining != 0 {
            self.next %= pairs;
            let target = self.next / releases.len();
            let donor = self.next % releases.len();
            self.next += 1;
            self.remaining -= 1;
            observed += 1;
            if target == donor {
                continue;
            }
            let (target, donor) = if target < donor {
                let (before, after) = releases.split_at_mut(donor);
                (&mut before[target], &after[0])
            } else {
                let (before, after) = releases.split_at_mut(target);
                (&mut after[0], &before[donor])
            };
            if join_shared_activation(target, donor) {
                joined += 1;
                // Give recording and delivery a chance immediately. Neither
                // native proof nor this receipt establishes a wire outcome.
                break;
            }
        }
        Some((observed, joined))
    }
}

#[cfg(unix)]
fn join_shared_activation(
    target: &mut PrivateSettlingRelease,
    donor: &PrivateSettlingRelease,
) -> bool {
    match (target.native.as_mut(), donor.native.as_ref()) {
        (Some(PrivateNativeHold::Pointer(target)), Some(PrivateNativeHold::Pointer(donor))) => {
            donor.activation_retirement().is_some_and(|receipt| {
                target.complete_shared_activation(receipt).is_ok()
            })
        }
        (Some(PrivateNativeHold::Key(target)), Some(PrivateNativeHold::Key(donor))) => {
            donor.activation_retirement().is_some_and(|receipt| {
                target.complete_shared_activation(receipt).is_ok()
            })
        }
        _ => false,
    }
}
