use crate::{LiveCpuBufferUpdate, LiveCpuCompositionReport, LiveProductionComposedFrame};
use sophia_engine::{
    HeadlessOutput, ProductionPresentationAdapter, ProductionRetirement, SurfaceChromeStyle,
    surface_chrome_display_list_for_surfaces,
};
use sophia_protocol::{CommittedSurfaceState, Point, SurfaceId, TransactionCommit};
use sophia_renderer_live::{LiveCpuPresentationLayer, LiveProductionCpuScene};
use std::error::Error;
use std::time::{Duration, Instant};

pub type LiveProductionCycleError = Box<dyn Error>;

pub struct LiveProductionCpuCycleFrame {
    committed_surfaces: Vec<CommittedSurfaceState>,
    authority_commits: Vec<TransactionCommit>,
    composition: LiveCpuCompositionReport,
    native_frames: Option<Vec<LiveProductionComposedFrame>>,
    cpu_layers: Vec<LiveCpuPresentationLayer>,
    composed: bool,
    compose_elapsed: Duration,
}

#[derive(Clone, Debug)]
pub struct LiveProductionCpuCycleSubmission<Tick> {
    pub tick: Tick,
    pub composition: LiveCpuCompositionReport,
    pub composed: bool,
    pub compose_elapsed: Duration,
}

pub struct LiveProductionCpuCycleAdapter<'scene, 'layout, Submit> {
    scene: &'scene mut LiveProductionCpuScene,
    presentation_order: &'layout [SurfaceId],
    /// The surfaces the session authorised a frame for. The software frame
    /// must frame the same set the head frames and the chrome observation
    /// frame; framing every surface in the order put borders on popups no
    /// head ever showed one on.
    chrome_surfaces: &'layout [SurfaceId],
    updates: Option<Vec<LiveCpuBufferUpdate>>,
    raised_surface: Option<SurfaceId>,
    focused_surface: Option<SurfaceId>,
    surface_chrome_style: SurfaceChromeStyle,
    cursor_position: Option<Point>,
    defer_frame: bool,
    create_native_frames: bool,
    cpu_buffer_residency: &'layout [u64],
    output_descriptors: &'layout [HeadlessOutput],
    submit: Submit,
}

impl<'scene, 'layout, Submit> LiveProductionCpuCycleAdapter<'scene, 'layout, Submit> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        scene: &'scene mut LiveProductionCpuScene,
        presentation_order: &'layout [SurfaceId],
        chrome_surfaces: &'layout [SurfaceId],
        updates: Vec<LiveCpuBufferUpdate>,
        raised_surface: Option<SurfaceId>,
        focused_surface: Option<SurfaceId>,
        surface_chrome_style: SurfaceChromeStyle,
        cursor_position: Option<Point>,
        defer_frame: bool,
        create_native_frames: bool,
        cpu_buffer_residency: &'layout [u64],
        output_descriptors: &'layout [HeadlessOutput],
        submit: Submit,
    ) -> Self {
        Self {
            scene,
            presentation_order,
            chrome_surfaces,
            updates: Some(updates),
            raised_surface,
            focused_surface,
            surface_chrome_style,
            cursor_position,
            defer_frame,
            create_native_frames,
            cpu_buffer_residency,
            output_descriptors,
            submit,
        }
    }
}

impl<Submit, Tick> ProductionPresentationAdapter for LiveProductionCpuCycleAdapter<'_, '_, Submit>
where
    Submit: FnMut(
        u64,
        &[CommittedSurfaceState],
        &[TransactionCommit],
        Option<Vec<LiveProductionComposedFrame>>,
        Vec<LiveCpuPresentationLayer>,
    ) -> Result<Tick, LiveProductionCycleError>,
{
    type Frame = LiveProductionCpuCycleFrame;
    type Submission = LiveProductionCpuCycleSubmission<Tick>;
    type Retirement = ();
    type Evidence = ();
    type Error = LiveProductionCycleError;

    fn compose(
        &mut self,
        _cycle: u64,
        committed: &[CommittedSurfaceState],
        authority_commits: &[TransactionCommit],
    ) -> Result<Self::Frame, Self::Error> {
        self.scene
            .apply_production_updates(self.updates.take().unwrap_or_default())?;
        self.scene
            .reconcile_buffer_residency(self.cpu_buffer_residency);
        let missing_buffers = self.scene.missing_committed_buffer_count(committed);
        if missing_buffers != 0 {
            return Err(format!(
                "production CPU scene is missing {missing_buffers} committed buffer(s)"
            )
            .into());
        }
        let compose_started = Instant::now();
        let presentation_order =
            raised_presentation_order(self.presentation_order, self.raised_surface);
        let composition = if self.defer_frame {
            self.scene
                .last_report()
                .cloned()
                .ok_or("software redraw coalescing has no prior composed frame")?
        } else {
            let output = self
                .output_descriptors
                .first()
                .ok_or("software composition has no output descriptor")?;
            let display_list = surface_chrome_display_list_for_surfaces(
                output.id,
                &presentation_order,
                self.chrome_surfaces,
                committed,
                self.focused_surface,
                self.surface_chrome_style,
            )?;
            self.scene
                .compose_display_list(*output, committed, &display_list, self.cursor_position)?
                .clone()
        };
        let native_frames = if self.defer_frame || !self.create_native_frames {
            None
        } else {
            Some(self.scene.frames_for_outputs(self.output_descriptors)?)
        };
        let cpu_layers = self
            .scene
            .presentation_variant_layers(committed, &presentation_order);
        Ok(LiveProductionCpuCycleFrame {
            committed_surfaces: committed.to_vec(),
            authority_commits: authority_commits.to_vec(),
            composition,
            native_frames,
            cpu_layers,
            composed: !self.defer_frame,
            compose_elapsed: if self.defer_frame {
                Duration::ZERO
            } else {
                compose_started.elapsed()
            },
        })
    }

    fn submit_frame(
        &mut self,
        cycle: u64,
        frame: Self::Frame,
    ) -> Result<Self::Submission, Self::Error> {
        let tick = (self.submit)(
            cycle,
            &frame.committed_surfaces,
            &frame.authority_commits,
            frame.native_frames,
            frame.cpu_layers,
        )?;
        Ok(LiveProductionCpuCycleSubmission {
            tick,
            composition: frame.composition,
            composed: frame.composed,
            compose_elapsed: frame.compose_elapsed,
        })
    }

    fn poll_retirements(
        &mut self,
    ) -> Result<Vec<ProductionRetirement<Self::Retirement>>, Self::Error> {
        Ok(Vec::new())
    }

    fn route_protocol_feedback(
        &mut self,
        _cycle: u64,
        _retirement: Self::Retirement,
    ) -> Result<Self::Evidence, Self::Error> {
        Ok(())
    }
}

pub fn raised_presentation_order(
    presentation_order: &[SurfaceId],
    raised_surface: Option<SurfaceId>,
) -> Vec<SurfaceId> {
    let mut order = presentation_order
        .iter()
        .copied()
        .filter(|surface| Some(*surface) != raised_surface)
        .collect::<Vec<_>>();
    if let Some(raised) = raised_surface
        && presentation_order.contains(&raised)
    {
        order.push(raised);
    }
    order
}
