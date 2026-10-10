//! Custody of retained renderer images across a requested terminal switch (t322).

/// What a requested terminal switch did with the retained renderer images.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RendererHandoffCustody {
    /// The live owner's images were exported as a fresh handoff.
    Captured,
    /// No owner was live, so the handoff already held was kept.
    Retained,
}

impl RendererHandoffCustody {
    pub(super) const fn reduced_name(self) -> &'static str {
        match self {
            Self::Captured => "captured",
            Self::Retained => "retained",
        }
    }
}

/// The handoff a requested terminal switch leaves behind.
///
/// With a live owner its images are captured, as before. Without one, as in
/// zero-output Waiting after a loss retired the owner, the held handoff is the
/// only copy of the images the runtime still names: dropping it would make
/// every later resume refuse with "omitted retained renderer images". A failed
/// capture leaves the held handoff untouched.
pub(super) fn suspend_renderer_handoff_for_terminal_switch<H, E>(
    held: &mut Option<H>,
    capture: Option<impl FnOnce() -> Result<H, E>>,
) -> Result<RendererHandoffCustody, E> {
    let Some(capture) = capture else {
        return Ok(RendererHandoffCustody::Retained);
    };
    *held = Some(capture()?);
    Ok(RendererHandoffCustody::Captured)
}

#[path = "../../tests/support/renderer_handoff_custody.rs"]
mod tests;
