use super::scanout::NativeCompositionDamageRect;

/// The reason selected by the render's actual EGL buffer age. Planning an
/// unused age never increments these counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NativeFullRepaintReason {
    #[default]
    NoTable,
    Disabled,
    UnknownAge,
    NoHistory,
    BeyondHistory,
    DamageUnavailable,
    PlanFull,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NativeRepaintPlan {
    Full(NativeFullRepaintReason),
    Partial(Vec<NativeCompositionDamageRect>),
}

/// A bounded caller-supplied plan for each possible age. Even an all-full
/// table carries evidence; omitting it would erase why partial repaint was
/// unavailable. This metadata never makes a full plan partial.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeCompositionRepaintTable {
    by_age: Vec<NativeRepaintPlan>,
    fallback: NativeFullRepaintReason,
    stable_geometry: bool,
}

impl Default for NativeCompositionRepaintTable {
    fn default() -> Self {
        Self::full(NativeFullRepaintReason::NoTable)
    }
}

impl NativeCompositionRepaintTable {
    pub fn from_ages(by_age: Vec<Option<Vec<NativeCompositionDamageRect>>>) -> Self {
        Self::with_evidence(
            by_age
                .into_iter()
                .map(|damage| {
                    damage.map_or(
                        NativeRepaintPlan::Full(NativeFullRepaintReason::PlanFull),
                        NativeRepaintPlan::Partial,
                    )
                })
                .collect(),
            false,
        )
    }

    pub fn with_evidence(by_age: Vec<NativeRepaintPlan>, stable_geometry: bool) -> Self {
        Self {
            by_age,
            fallback: NativeFullRepaintReason::BeyondHistory,
            stable_geometry,
        }
    }

    pub fn full(reason: NativeFullRepaintReason) -> Self {
        Self {
            by_age: Vec::new(),
            fallback: reason,
            stable_geometry: false,
        }
    }

    pub fn damage_for_age(&self, age: u32) -> Option<&[NativeCompositionDamageRect]> {
        match self
            .by_age
            .get(usize::try_from(age.checked_sub(1)?).ok()?)?
        {
            NativeRepaintPlan::Partial(rects) => Some(rects),
            NativeRepaintPlan::Full(_) => None,
        }
    }

    pub fn full_reason_for_age(&self, age: u32) -> Option<NativeFullRepaintReason> {
        // Disabled or absent history explains the full repaint even on a
        // driver without buffer-age support. Otherwise age zero is unknown.
        if self.by_age.is_empty() {
            return Some(self.fallback);
        }
        if age == 0 {
            return Some(NativeFullRepaintReason::UnknownAge);
        }
        match self.by_age.get((age - 1) as usize) {
            Some(NativeRepaintPlan::Partial(_)) => None,
            Some(NativeRepaintPlan::Full(reason)) => Some(*reason),
            None => Some(self.fallback),
        }
    }

    pub fn stable_geometry(&self) -> bool {
        self.stable_geometry
    }
    pub fn is_empty(&self) -> bool {
        self.by_age.is_empty()
    }
}

/// Successful compositions only, cumulative within one context. The geometry
/// cohort is defined by the backend and does not assert valid client damage.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeCompositionDamageStats {
    pub full_no_table: u64,
    pub full_disabled: u64,
    pub full_unknown_age: u64,
    pub full_no_history: u64,
    pub full_beyond_history: u64,
    pub full_damage_unavailable: u64,
    pub full_plan: u64,
    pub stable_geometry_frames: u64,
    pub stable_geometry_full: u64,
    pub stable_geometry_partial: u64,
    pub stable_geometry_repaint_pixels: u64,
    pub stable_geometry_target_pixels: u64,
}

impl NativeCompositionDamageStats {
    pub fn observe(
        &mut self,
        reason: Option<NativeFullRepaintReason>,
        stable: bool,
        repaint: u64,
        target: u64,
    ) {
        use NativeFullRepaintReason as R;
        if let Some(reason) = reason {
            let count = match reason {
                R::NoTable => &mut self.full_no_table,
                R::Disabled => &mut self.full_disabled,
                R::UnknownAge => &mut self.full_unknown_age,
                R::NoHistory => &mut self.full_no_history,
                R::BeyondHistory => &mut self.full_beyond_history,
                R::DamageUnavailable => &mut self.full_damage_unavailable,
                R::PlanFull => &mut self.full_plan,
            };
            *count = count.saturating_add(1);
        }
        if stable {
            self.stable_geometry_frames = self.stable_geometry_frames.saturating_add(1);
            let count = if reason.is_some() {
                &mut self.stable_geometry_full
            } else {
                &mut self.stable_geometry_partial
            };
            *count = count.saturating_add(1);
            self.stable_geometry_repaint_pixels =
                self.stable_geometry_repaint_pixels.saturating_add(repaint);
            self.stable_geometry_target_pixels =
                self.stable_geometry_target_pixels.saturating_add(target);
        }
    }

    pub fn add(&mut self, other: Self) {
        macro_rules! add { ($($field:ident),+ $(,)?) => { $(self.$field = self.$field.saturating_add(other.$field);)+ }; }
        add!(
            full_no_table,
            full_disabled,
            full_unknown_age,
            full_no_history,
            full_beyond_history,
            full_damage_unavailable,
            full_plan,
            stable_geometry_frames,
            stable_geometry_full,
            stable_geometry_partial,
            stable_geometry_repaint_pixels,
            stable_geometry_target_pixels
        );
    }
}
