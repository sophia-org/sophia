//! Fixed presentation extension records; connection and scene authority are
//! validated by the projection owner after this bounded decoding step.
use super::cursor::{Cursor, push_i32, push_u16, push_u32, push_u64};
use super::{IpcCodecError, PolicyRecordSection, PolicyRecordSectionRef, WmV1ProjectionChunk};
use crate::*;

pub const PROJECTION_PRESENTATION_RECORD_KIND: u16 = 0xff09;
pub const PROJECTION_PRESENTATION_OUTPUT_RECORD_KIND: u16 = 0xff0a;
pub const PROJECTION_SURFACE_INSTANCE_RECORD_KIND: u16 = 0xff0b;
pub const PROJECTION_PRESENTATION_REGION_RECORD_KIND: u16 = 0xff0c;
pub const PROJECTION_PRESENTATION_BINDING_RECORD_KIND: u16 = 0xff0d;

pub fn wm_presentation_record_layout(kind: u16) -> Option<(usize, usize, u64)> {
    let visual = super::SOPHIA_WM_CAPABILITY_SURFACE_INSTANCES;
    Some(match kind {
        PROJECTION_PRESENTATION_RECORD_KIND => (32, 1, visual),
        PROJECTION_PRESENTATION_OUTPUT_RECORD_KIND => (40, POLICY_MAX_PRESENTATION_OUTPUTS, visual),
        PROJECTION_SURFACE_INSTANCE_RECORD_KIND => (80, POLICY_MAX_SURFACE_INSTANCES, visual),
        PROJECTION_PRESENTATION_REGION_RECORD_KIND => (72, POLICY_MAX_PRESENTATION_REGIONS, visual),
        PROJECTION_PRESENTATION_BINDING_RECORD_KIND => (
            16,
            POLICY_MAX_PRESENTATION_BINDINGS,
            visual | super::SOPHIA_WM_CAPABILITY_PRESENTATION_ACTIONS,
        ),
        _ => return None,
    })
}

fn invalid() -> IpcCodecError {
    IpcCodecError::InvalidEnum {
        field: "wm_presentation",
        value: 0,
    }
}

fn rect(out: &mut Vec<u8>, r: Rect) {
    for value in [r.x, r.y, r.width, r.height] {
        push_i32(out, value);
    }
}

fn read_rect(c: &mut Cursor<'_>) -> Result<Rect, IpcCodecError> {
    Ok(Rect {
        x: c.i32()?,
        y: c.i32()?,
        width: c.i32()?,
        height: c.i32()?,
    })
}

fn optional_action(value: u64) -> Option<WmActionId> {
    (value != 0).then(|| WmActionId::from_raw(value))
}

pub fn encode_wm_presentation(
    presentation: Option<&PolicyPresentation>,
    epoch: u64,
    ordinal: u16,
) -> Result<Vec<WmV1ProjectionChunk>, IpcCodecError> {
    super::wm_record_sections::projection_chunks(
        encode_policy_presentation_records(presentation, epoch)?,
        epoch,
        ordinal,
        |kind| wm_presentation_record_layout(kind).map(|r| r.0),
    )
    .map_err(|_| invalid())
}

pub fn decode_wm_presentation(
    chunks: &[WmV1ProjectionChunk],
) -> Result<Option<PolicyPresentation>, IpcCodecError> {
    decode_policy_presentation_records(&super::wm_record_sections::projection_sections(chunks))
}

pub fn encode_policy_presentation_records(
    presentation: Option<&PolicyPresentation>,
    epoch: u64,
) -> Result<Vec<PolicyRecordSection>, IpcCodecError> {
    let Some(p) = presentation else {
        return Ok(Vec::new());
    };
    crate::validate_policy_presentation_shape(p).map_err(|_| invalid())?;
    if epoch == 0 {
        return Err(invalid());
    }
    let mut header = Vec::new();
    push_u64(&mut header, p.generation);
    push_u64(&mut header, p.keyboard_output.map_or(0, OutputId::raw));
    push_u16(&mut header, p.outputs.len() as u16);
    push_u16(&mut header, p.bindings.len() as u16);
    push_u32(&mut header, p.instances.len() as u32);
    push_u32(&mut header, p.regions.len() as u32);
    push_u32(&mut header, 0);
    let mut outputs = Vec::new();
    for o in &p.outputs {
        push_u64(&mut outputs, o.output.raw());
        push_u64(&mut outputs, o.generation);
        rect(&mut outputs, o.coverage);
        push_u16(&mut outputs, o.mode as u16);
        push_u16(&mut outputs, 0);
        push_u32(&mut outputs, 0);
    }
    let mut instances = Vec::new();
    for i in &p.instances {
        push_u64(&mut instances, i.id);
        push_u64(&mut instances, i.generation);
        push_u64(&mut instances, i.output.raw());
        push_u32(&mut instances, i.source.index());
        push_u32(&mut instances, i.source.generation());
        rect(&mut instances, i.destination);
        rect(&mut instances, i.clip);
        push_u16(&mut instances, i.opacity_millis);
        push_u16(&mut instances, i.z_index);
        push_u32(&mut instances, 0);
        push_u64(&mut instances, i.action.map_or(0, WmActionId::raw));
    }
    let mut regions = Vec::new();
    for r in &p.regions {
        push_u64(&mut regions, r.id);
        push_u64(&mut regions, r.generation);
        push_u64(&mut regions, r.output.raw());
        rect(&mut regions, r.geometry);
        rect(&mut regions, r.clip);
        push_u16(&mut regions, r.z_index);
        push_u16(&mut regions, r.role as u16);
        push_u32(&mut regions, 0);
        push_u64(&mut regions, r.action.map_or(0, WmActionId::raw));
    }
    let mut bindings = Vec::new();
    for b in &p.bindings {
        push_u64(&mut bindings, b.action.raw());
        push_u32(&mut bindings, b.keycode);
        push_u32(&mut bindings, b.modifiers.bits);
    }
    let mut sections = Vec::new();
    for (kind, bytes) in [
        (PROJECTION_PRESENTATION_RECORD_KIND, header),
        (PROJECTION_PRESENTATION_OUTPUT_RECORD_KIND, outputs),
        (PROJECTION_SURFACE_INSTANCE_RECORD_KIND, instances),
        (PROJECTION_PRESENTATION_REGION_RECORD_KIND, regions),
        (PROJECTION_PRESENTATION_BINDING_RECORD_KIND, bindings),
    ] {
        let (size, _, _) = wm_presentation_record_layout(kind).ok_or_else(invalid)?;
        if !bytes.is_empty() {
            sections.push(PolicyRecordSection {
                kind,
                count: (bytes.len() / size) as u32,
                bytes,
            });
        }
    }
    Ok(sections)
}

pub fn decode_policy_presentation_records(
    sections: &[PolicyRecordSectionRef<'_>],
) -> Result<Option<PolicyPresentation>, IpcCodecError> {
    let mut p = None;
    let mut counts = (0_usize, 0_usize, 0_usize, 0_usize);
    let mut last_kind = 0;
    for chunk in sections {
        let Some((size, maximum, _)) = wm_presentation_record_layout(chunk.kind) else {
            continue;
        };
        if chunk.kind < last_kind
            || chunk.count == 0
            || chunk.count as usize > maximum
            || chunk.bytes.len() != chunk.count as usize * size
        {
            return Err(invalid());
        }
        last_kind = chunk.kind;
        for data in chunk.bytes.chunks_exact(size) {
            let mut c = Cursor::new(data);
            if chunk.kind == PROJECTION_PRESENTATION_RECORD_KIND {
                if p.is_some() {
                    return Err(invalid());
                }
                let generation = c.u64()?;
                let keyboard = c.u64()?;
                counts = (
                    usize::from(c.u16()?),
                    usize::from(c.u16()?),
                    c.u32()? as usize,
                    c.u32()? as usize,
                );
                if c.u32()? != 0
                    || counts.0 > POLICY_MAX_PRESENTATION_OUTPUTS
                    || counts.1 > POLICY_MAX_PRESENTATION_BINDINGS
                    || counts.2 > POLICY_MAX_SURFACE_INSTANCES
                    || counts.3 > POLICY_MAX_PRESENTATION_REGIONS
                {
                    return Err(invalid());
                }
                p = Some(PolicyPresentation {
                    generation,
                    keyboard_output: (keyboard != 0).then(|| OutputId::from_raw(keyboard)),
                    outputs: Vec::new(),
                    instances: Vec::new(),
                    regions: Vec::new(),
                    bindings: Vec::new(),
                });
            } else {
                let p = p.as_mut().ok_or_else(invalid)?;
                match chunk.kind {
                    PROJECTION_PRESENTATION_OUTPUT_RECORD_KIND => {
                        if p.outputs.len() >= counts.0 {
                            return Err(invalid());
                        }
                        let output = OutputId::from_raw(c.u64()?);
                        let generation = c.u64()?;
                        let coverage = read_rect(&mut c)?;
                        let mode = match c.u16()? {
                            1 => PolicyPresentationMode::Overlay,
                            2 => PolicyPresentationMode::ReplaceApplications,
                            _ => return Err(invalid()),
                        };
                        if c.u16()? != 0 || c.u32()? != 0 {
                            return Err(invalid());
                        }
                        p.outputs.push(PolicyPresentationOutput {
                            output,
                            generation,
                            coverage,
                            mode,
                        });
                    }
                    PROJECTION_SURFACE_INSTANCE_RECORD_KIND => {
                        if p.instances.len() >= counts.2 {
                            return Err(invalid());
                        }
                        let id = c.u64()?;
                        let generation = c.u64()?;
                        let output = OutputId::from_raw(c.u64()?);
                        let source = SurfaceId::new(c.u32()?, c.u32()?);
                        let destination = read_rect(&mut c)?;
                        let clip = read_rect(&mut c)?;
                        let opacity_millis = c.u16()?;
                        let z_index = c.u16()?;
                        if c.u32()? != 0 {
                            return Err(invalid());
                        }
                        let action = optional_action(c.u64()?);
                        p.instances.push(PolicySurfaceInstance {
                            id,
                            generation,
                            output,
                            source,
                            destination,
                            clip,
                            opacity_millis,
                            z_index,
                            action,
                        });
                    }
                    PROJECTION_PRESENTATION_REGION_RECORD_KIND => {
                        if p.regions.len() >= counts.3 {
                            return Err(invalid());
                        }
                        let id = c.u64()?;
                        let generation = c.u64()?;
                        let output = OutputId::from_raw(c.u64()?);
                        let geometry = read_rect(&mut c)?;
                        let clip = read_rect(&mut c)?;
                        let z_index = c.u16()?;
                        let role = match c.u16()? {
                            1 => PolicyPresentationRegionRole::Backdrop,
                            2 => PolicyPresentationRegionRole::Frame,
                            3 => PolicyPresentationRegionRole::Emphasis,
                            _ => return Err(invalid()),
                        };
                        if c.u32()? != 0 {
                            return Err(invalid());
                        }
                        let action = optional_action(c.u64()?);
                        p.regions.push(PolicyPresentationRegion {
                            id,
                            generation,
                            output,
                            geometry,
                            clip,
                            z_index,
                            role,
                            action,
                        });
                    }
                    PROJECTION_PRESENTATION_BINDING_RECORD_KIND => {
                        if p.bindings.len() >= counts.1 {
                            return Err(invalid());
                        }
                        p.bindings.push(PolicyPresentationBinding {
                            action: WmActionId::from_raw(c.u64()?),
                            keycode: c.u32()?,
                            modifiers: WmModifierMask { bits: c.u32()? },
                        });
                    }
                    _ => return Err(invalid()),
                }
            }
            c.finish()?;
        }
    }
    if let Some(p) = &p {
        if (
            p.outputs.len(),
            p.bindings.len(),
            p.instances.len(),
            p.regions.len(),
        ) != counts
        {
            return Err(invalid());
        }
        crate::validate_policy_presentation_shape(p).map_err(|_| invalid())?;
    }
    Ok(p)
}
