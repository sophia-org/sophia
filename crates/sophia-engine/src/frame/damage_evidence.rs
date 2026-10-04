//! Bounded explanations gathered by the damage reducer itself. Causes may
//! overlap; they describe contributors or conservative fallbacks, not pixels
//! attributed exclusively to one cause. No counters are advanced by planning.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputDamageCause {
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
pub struct OutputDamageCauses(u32);

impl OutputDamageCauses {
    pub fn insert(&mut self, cause: OutputDamageCause) {
        self.0 |= 1 << cause as u32;
    }

    pub fn contains(self, cause: OutputDamageCause) -> bool {
        self.0 & (1 << cause as u32) != 0
    }
}

impl OutputDamageCause {
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
}
