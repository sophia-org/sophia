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
    PlanDamageCapacity,
    PlanRectLimit,
    PlanCoverage,
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
    by_age: Vec<(NativeRepaintPlan, NativeDamageCauses)>,
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
        Self::with_attribution(
            by_age
                .into_iter()
                .map(|plan| (plan, NativeDamageCauses::default()))
                .collect(),
            stable_geometry,
        )
    }

    pub fn with_attribution(
        by_age: Vec<(NativeRepaintPlan, NativeDamageCauses)>,
        stable_geometry: bool,
    ) -> Self {
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
            (NativeRepaintPlan::Partial(rects), _) => Some(rects),
            (NativeRepaintPlan::Full(_), _) => None,
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
            Some((NativeRepaintPlan::Partial(_), _)) => None,
            Some((NativeRepaintPlan::Full(reason), _)) => Some(*reason),
            None => Some(self.fallback),
        }
    }

    /// Only evidence from the selected age is eligible for successful-render
    /// accounting. Unknown or out-of-history ages have no reduction evidence.
    pub fn causes_for_age(&self, age: u32) -> NativeDamageCauses {
        age.checked_sub(1)
            .and_then(|index| self.by_age.get(index as usize))
            .map_or_else(NativeDamageCauses::default, |(_, causes)| *causes)
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
    /// Sum of the plan subreasons; retained for existing consumers.
    pub full_plan: u64,
    pub full_plan_unspecified: u64,
    pub full_plan_capacity: u64,
    pub full_plan_rect_limit: u64,
    pub full_plan_coverage: u64,
    /// Per successful frame, at most one increment per cause. Causes overlap.
    pub causes: [u64; NativeDamageCause::ALL.len()],
    pub full_causes: [u64; NativeDamageCause::ALL.len()],
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
                R::PlanFull => &mut self.full_plan_unspecified,
                R::PlanDamageCapacity => &mut self.full_plan_capacity,
                R::PlanRectLimit => &mut self.full_plan_rect_limit,
                R::PlanCoverage => &mut self.full_plan_coverage,
            };
            *count = count.saturating_add(1);
            if matches!(
                reason,
                R::PlanFull | R::PlanDamageCapacity | R::PlanRectLimit | R::PlanCoverage
            ) {
                self.full_plan = self.full_plan.saturating_add(1);
            }
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

    pub fn observe_causes(&mut self, causes: NativeDamageCauses, full: bool) {
        for cause in NativeDamageCause::ALL {
            if causes.contains(cause) {
                let count = &mut self.causes[cause as usize];
                *count = count.saturating_add(1);
                if full {
                    let count = &mut self.full_causes[cause as usize];
                    *count = count.saturating_add(1);
                }
            }
        }
    }

    pub fn add(&mut self, other: Self) {
        for (count, other) in self.full_causes.iter_mut().zip(other.full_causes) {
            *count = count.saturating_add(other);
        }
        for (count, other) in self.causes.iter_mut().zip(other.causes) {
            *count = count.saturating_add(other);
        }
        macro_rules! add { ($($field:ident),+ $(,)?) => { $(self.$field = self.$field.saturating_add(other.$field);)+ }; }
        add!(
            full_no_table,
            full_disabled,
            full_unknown_age,
            full_no_history,
            full_beyond_history,
            full_damage_unavailable,
            full_plan,
            full_plan_unspecified,
            full_plan_capacity,
            full_plan_rect_limit,
            full_plan_coverage,
            stable_geometry_frames,
            stable_geometry_full,
            stable_geometry_partial,
            stable_geometry_repaint_pixels,
            stable_geometry_target_pixels
        );
    }
}

/// A reason contributed by Engine's conservative damage reduction. These are
/// independent of the final full/partial threshold and may overlap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeDamageCause {
    NewOutput,
    OutputChanged,
    Compositor,
    Order,
    Geometry,
    Sampling,
    Generation,
    MissingIdentity,
    NoMatchingTransition,
    InvalidTransition,
    Origin,
    RectLimit,
    PrecisionRestricted,
    CoordinateOverflow,
    TerminalIdentity,
    HistoryLimit,
    Rebased,
    PreciseSurface,
    PreviewIdentity,
    Cursor,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NativeDamageCauses(u32);
impl NativeDamageCauses {
    pub fn insert(&mut self, cause: NativeDamageCause) {
        self.0 |= 1 << cause as u32;
    }
    pub fn contains(self, cause: NativeDamageCause) -> bool {
        self.0 & (1 << cause as u32) != 0
    }
}
impl NativeDamageCause {
    pub const ALL: [Self; 20] = [
        Self::NewOutput,
        Self::OutputChanged,
        Self::Compositor,
        Self::Order,
        Self::Geometry,
        Self::Sampling,
        Self::Generation,
        Self::MissingIdentity,
        Self::NoMatchingTransition,
        Self::InvalidTransition,
        Self::Origin,
        Self::RectLimit,
        Self::PrecisionRestricted,
        Self::CoordinateOverflow,
        Self::TerminalIdentity,
        Self::HistoryLimit,
        Self::Rebased,
        Self::PreciseSurface,
        Self::PreviewIdentity,
        Self::Cursor,
    ];
    pub const fn code(self) -> &'static str {
        match self {
            Self::NewOutput => "new_output",
            Self::OutputChanged => "output_changed",
            Self::Compositor => "compositor",
            Self::Order => "order",
            Self::Geometry => "geometry",
            Self::Sampling => "sampling",
            Self::Generation => "generation",
            Self::MissingIdentity => "missing_identity",
            Self::NoMatchingTransition => "no_matching_transition",
            Self::InvalidTransition => "invalid_transition",
            Self::Origin => "origin",
            Self::RectLimit => "rect_limit",
            Self::PrecisionRestricted => "precision_restricted",
            Self::CoordinateOverflow => "coordinate_overflow",
            Self::TerminalIdentity => "terminal_identity",
            Self::HistoryLimit => "history_limit",
            Self::Rebased => "rebased",
            Self::PreciseSurface => "precise_surface",
            Self::PreviewIdentity => "preview_identity",
            Self::Cursor => "cursor",
        }
    }
}
