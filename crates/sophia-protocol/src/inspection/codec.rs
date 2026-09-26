use super::*;
use serde::de::{DeserializeOwned, Error, SeqAccess, Visitor};
use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InspectionRecordError {
    Bounds,
    Json,
    Schema,
    Identity,
}

impl fmt::Display for InspectionRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Bounds => "inspection record exceeds bounds",
            Self::Json => "invalid inspection JSON",
            Self::Schema => "unsupported inspection schema",
            Self::Identity => "incoherent inspection identities",
        })
    }
}
impl std::error::Error for InspectionRecordError {}

pub(super) mod decimal {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(n: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&n.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        let text = String::deserialize(d)?;
        parse(&text).ok_or_else(|| D::Error::custom("expected canonical decimal u64 string"))
    }
    pub(super) fn parse(text: &str) -> Option<u64> {
        if text.is_empty()
            || text.len() > 20
            || (text.len() > 1 && text.starts_with('0'))
            || !text.bytes().all(|c| c.is_ascii_digit())
        {
            return None;
        }
        text.parse().ok()
    }
}

pub(super) mod optional_decimal {
    use serde::{Deserialize, Deserializer, Serializer, de::Error};
    pub fn serialize<S: Serializer>(n: &Option<u64>, s: S) -> Result<S::Ok, S::Error> {
        match n {
            Some(n) => s.serialize_some(&n.to_string()),
            None => s.serialize_none(),
        }
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
        Option::<String>::deserialize(d)?
            .map(|s| {
                super::decimal::parse(&s)
                    .ok_or_else(|| D::Error::custom("expected decimal u64 string"))
            })
            .transpose()
    }
}

fn bounded<'de, D, T, const MAX: usize>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Rows<T, const MAX: usize>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const MAX: usize> Visitor<'de> for Rows<T, MAX> {
        type Value = Vec<T>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "at most {MAX} records")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut result = Vec::new();
            while let Some(row) = seq.next_element()? {
                if result.len() == MAX {
                    return Err(A::Error::custom("inspection row limit"));
                }
                result.push(row);
            }
            Ok(result)
        }
    }
    d.deserialize_seq(Rows::<T, MAX>(std::marker::PhantomData))
}
pub(super) fn outputs<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Vec<InspectionOutput>, D::Error> {
    bounded::<D, _, INSPECTION_MAX_OUTPUTS>(d)
}
pub(super) fn surfaces<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Vec<InspectionSurface>, D::Error> {
    bounded::<D, _, INSPECTION_MAX_SURFACES>(d)
}

/// Validate the safe model, never convert a raw WM packet into disclosure.
pub fn validate_inspection_snapshot(
    value: &InspectionSnapshot,
) -> Result<(), InspectionRecordError> {
    if value.outputs.len() > INSPECTION_MAX_OUTPUTS
        || value.surfaces.len() > INSPECTION_MAX_SURFACES
    {
        return Err(InspectionRecordError::Bounds);
    }
    let mut outputs = BTreeSet::new();
    for output in &value.outputs {
        if output.id == 0 || !outputs.insert(output.id) {
            return Err(InspectionRecordError::Identity);
        }
    }
    let mut surfaces = BTreeSet::new();
    for surface in &value.surfaces {
        if surface.id.index == u32::MAX
            || surface.id.generation == 0
            || !surfaces.insert(surface.id)
            || surface.output.is_some_and(|o| !outputs.contains(&o))
        {
            return Err(InspectionRecordError::Identity);
        }
    }
    for output in &value.outputs {
        if output.focus.is_some_and(|id| {
            !value
                .surfaces
                .iter()
                .any(|s| s.id == id && s.output == Some(output.id))
        }) {
            return Err(InspectionRecordError::Identity);
        }
    }
    Ok(())
}

/// Canonical ordering is server-owned; no caller-supplied strings survive.
pub fn sanitize_inspection_snapshot(
    mut value: InspectionSnapshot,
) -> Result<InspectionSnapshot, InspectionRecordError> {
    validate_inspection_snapshot(&value)?;
    value.outputs.sort_by_key(|o| o.id);
    value.surfaces.sort_by_key(|s| s.id);
    Ok(value)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, InspectionRecordError> {
    if bytes.len() > INSPECTION_MAX_SNAPSHOT_BYTES {
        return Err(InspectionRecordError::Bounds);
    }
    serde_json::from_slice(bytes).map_err(|_| InspectionRecordError::Json)
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, InspectionRecordError> {
    // A bounded writer rejects before a serialized buffer can cross its cap.
    struct Buffer(Vec<u8>);
    impl std::io::Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > INSPECTION_MAX_SNAPSHOT_BYTES.saturating_sub(self.0.len()) {
                return Err(std::io::Error::other("inspection record limit"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    use std::io::Write;
    let mut buffer = Buffer(Vec::new());
    serde_json::to_writer(&mut buffer, value).map_err(|_| InspectionRecordError::Bounds)?;
    buffer
        .write_all(b"\n")
        .map_err(|_| InspectionRecordError::Bounds)?;
    Ok(buffer.0)
}
fn schema(schema: u32) -> Result<(), InspectionRecordError> {
    if schema != INSPECTION_SCHEMA {
        Err(InspectionRecordError::Schema)
    } else {
        Ok(())
    }
}
fn identity(generation: u64, sequence: u64) -> Result<(), InspectionRecordError> {
    if generation == 0 || sequence == 0 {
        Err(InspectionRecordError::Identity)
    } else {
        Ok(())
    }
}
pub fn encode_inspection_snapshot(
    value: &InspectionSnapshotRecord,
) -> Result<Vec<u8>, InspectionRecordError> {
    schema(value.schema)?;
    identity(value.generation, value.sequence)?;
    validate_inspection_snapshot(&value.snapshot)?;
    encode(value)
}
pub fn decode_inspection_snapshot(
    bytes: &[u8],
) -> Result<InspectionSnapshotRecord, InspectionRecordError> {
    let value: InspectionSnapshotRecord = decode(bytes)?;
    schema(value.schema)?;
    identity(value.generation, value.sequence)?;
    validate_inspection_snapshot(&value.snapshot)?;
    Ok(value)
}
pub fn encode_inspection_event(
    value: &InspectionEventRecord,
) -> Result<Vec<u8>, InspectionRecordError> {
    schema(value.schema)?;
    identity(value.generation, value.sequence)?;
    encode(value)
}
pub fn decode_inspection_event(
    bytes: &[u8],
) -> Result<InspectionEventRecord, InspectionRecordError> {
    let value: InspectionEventRecord = decode(bytes)?;
    schema(value.schema)?;
    identity(value.generation, value.sequence)?;
    Ok(value)
}
pub fn encode_inspection_status(
    value: &InspectionStatus,
) -> Result<Vec<u8>, InspectionRecordError> {
    schema(value.schema)?;
    if value.event_floor > value.event_tail {
        return Err(InspectionRecordError::Identity);
    }
    encode(value)
}
pub fn decode_inspection_status(bytes: &[u8]) -> Result<InspectionStatus, InspectionRecordError> {
    let value: InspectionStatus = decode(bytes)?;
    schema(value.schema)?;
    if value.event_floor > value.event_tail {
        return Err(InspectionRecordError::Identity);
    }
    Ok(value)
}
