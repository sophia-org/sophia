//! Distinctly-valued typed fixtures for every kind whose body is a single
//! flat record: the small `Negotiate`/`Negotiated`/`Refused`/`Submitted`/
//! `ObjectPublished` family, `AllocationRequest`/`AllocationResult`, the six
//! resource kinds, and the frame/action/candidate-outcome kinds. `Limits`
//! lives in [`super::fixtures_limits`] and the table-shaped `Outputs` and
//! `Candidate` kinds live in [`super::fixtures_tables`] because each is its
//! own cohesive unit.
//!
//! Every function returns the typed value(s) alongside the KDL
//! field-name-to-value table the codec's real encoder must reproduce
//! byte-for-byte; every struct is built with every field spelled out, never
//! `..`, so a struct that grows a field breaks the literal here.

use super::{aid, fmap, grant, lrect, margins_of, oid, prect, rid};
use sophia_shell_protocol::shell_files::*;
use sophia_shell_protocol::*;
use std::collections::BTreeMap;

pub fn negotiate() -> (ShellV1ClientHello, BTreeMap<&'static str, i128>) {
    let minimum_revision = 3u16;
    let maximum_revision = 8u16;
    let required_capabilities = 0x1357_9BDFu64;
    let hello = ShellV1ClientHello {
        minimum_revision,
        maximum_revision,
        required_capabilities,
    };
    let expected = fmap!(minimum_revision, maximum_revision, required_capabilities);
    (hello, expected)
}

pub fn negotiated() -> (ShellFileNegotiated, BTreeMap<&'static str, i128>) {
    let selected_revision = 7u16;
    let connection_epoch = 555u64;
    let capabilities = 0x1357u64;
    let max_descriptors = 9u16;
    let max_label_bytes = 17u16;
    let max_pending_activations = 6u16;
    let limits_published = true;
    let value = ShellFileNegotiated {
        welcome: ShellV1ServerWelcome {
            selected_revision,
            connection_epoch,
            capabilities,
            max_descriptors,
            max_label_bytes,
            max_pending_activations,
        },
        limits_published,
    };
    let expected = fmap!(
        selected_revision,
        connection_epoch,
        capabilities,
        max_descriptors,
        max_label_bytes,
        max_pending_activations,
        limits_published
    );
    (value, expected)
}

pub fn refused() -> (ContentAdmissionRefused, BTreeMap<&'static str, i128>) {
    let reason = 3u16;
    let denied_capabilities = 9320u64;
    let value = ContentAdmissionRefused {
        reason,
        denied_capabilities,
    };
    let expected = fmap!(reason, denied_capabilities);
    (value, expected)
}

pub fn submitted() -> (ShellFileSubmitted, BTreeMap<&'static str, i128>) {
    let submission_id = 777u64;
    let candidate_kind_value = ShellFileKind::ResourceBegin;
    let candidate_kind = candidate_kind_value as u16;
    let value = ShellFileSubmitted {
        submission_id,
        candidate_kind: candidate_kind_value,
    };
    let expected = fmap!(submission_id, candidate_kind);
    (value, expected)
}

pub fn object_published() -> (ShellFileObjectPublished, BTreeMap<&'static str, i128>) {
    let object = ShellFileKind::Outputs;
    let object_kind = object as u16;
    let generation = 4321u64;
    let qid = 8765u64;
    let value = ShellFileObjectPublished {
        object,
        generation,
        qid,
    };
    let expected = fmap!(object_kind, generation, qid);
    (value, expected)
}

pub fn allocation_request() -> (u64, ContentAllocationRequest, BTreeMap<&'static str, i128>) {
    let transaction = 555u64;
    let grant_connection_epoch = 61u64;
    let grant_content_epoch = 62u64;
    let output_id = 63u64;
    let output_generation = 64u64;
    let allocation_request_id = 65u64;
    let operation = 2u16;
    let role = 2u16;
    let edge = 3u16;
    let prior_id = 66u64;
    let prior_generation = 67u64;
    let parent_id = 68u64;
    let parent_generation = 69u64;
    let parent_presentation_epoch = 70u64;
    let anchor_x = 5i32;
    let anchor_y = 6i32;
    let anchor_width = 71u32;
    let anchor_height = 72u32;
    let desired_width = 73u32;
    let desired_height = 74u32;
    let margin_top = -10i16;
    let margin_right = 20i16;
    let margin_bottom = -30i16;
    let margin_left = 40i16;
    let record = ContentAllocationRequest {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        output: oid(output_id, output_generation),
        allocation_request_id,
        operation,
        role,
        edge,
        prior: aid(prior_id, prior_generation),
        parent: aid(parent_id, parent_generation),
        parent_presentation_epoch,
        anchor_parent_rect: prect(anchor_x, anchor_y, anchor_width, anchor_height),
        desired_width,
        desired_height,
        margins: margins_of(margin_top, margin_right, margin_bottom, margin_left),
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        output_id,
        output_generation,
        allocation_request_id,
        operation,
        role,
        edge,
        prior_id,
        prior_generation,
        parent_id,
        parent_generation,
        parent_presentation_epoch,
        anchor_x,
        anchor_y,
        anchor_width,
        anchor_height,
        desired_width,
        desired_height,
        margin_top,
        margin_right,
        margin_bottom,
        margin_left
    );
    (transaction, record, expected)
}

pub fn allocation_result() -> (u64, ContentAllocationResult, BTreeMap<&'static str, i128>) {
    let transaction = 606u64;
    let grant_connection_epoch = 81u64;
    let grant_content_epoch = 82u64;
    let allocation_request_id = 83u64;
    let status = 1u16;
    let reason = 0u16;
    let output_id = 84u64;
    let output_generation = 85u64;
    let allocation_id = 86u64;
    let allocation_generation = 87u64;
    let parent_id = 88u64;
    let parent_generation = 89u64;
    let scale_generation = 90u64;
    let logical_x = 1i32;
    let logical_y = 2i32;
    let logical_width = 91u32;
    let logical_height = 92u32;
    let pixel_x = 3i32;
    let pixel_y = 4i32;
    let pixel_width = 93u32;
    let pixel_height = 94u32;
    let scale_numerator = 5u32;
    let scale_denominator = 3u32;
    let allowed_reservation_extent = 96u32;
    let margin_top = -1i16;
    let margin_right = 2i16;
    let margin_bottom = -3i16;
    let margin_left = 4i16;
    let acknowledged_anchor_x = 6i32;
    let acknowledged_anchor_y = 7i32;
    let acknowledged_anchor_width = 97u32;
    let acknowledged_anchor_height = 98u32;
    let record = ContentAllocationResult {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        allocation_request_id,
        status,
        reason,
        output: oid(output_id, output_generation),
        allocation: aid(allocation_id, allocation_generation),
        parent: aid(parent_id, parent_generation),
        scale_generation,
        logical: lrect(logical_x, logical_y, logical_width, logical_height),
        pixel: prect(pixel_x, pixel_y, pixel_width, pixel_height),
        scale_numerator,
        scale_denominator,
        allowed_reservation_extent,
        margins: margins_of(margin_top, margin_right, margin_bottom, margin_left),
        acknowledged_anchor: prect(
            acknowledged_anchor_x,
            acknowledged_anchor_y,
            acknowledged_anchor_width,
            acknowledged_anchor_height,
        ),
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        allocation_request_id,
        status,
        reason,
        output_id,
        output_generation,
        allocation_id,
        allocation_generation,
        parent_id,
        parent_generation,
        scale_generation,
        logical_x,
        logical_y,
        logical_width,
        logical_height,
        pixel_x,
        pixel_y,
        pixel_width,
        pixel_height,
        scale_numerator,
        scale_denominator,
        allowed_reservation_extent,
        margin_top,
        margin_right,
        margin_bottom,
        margin_left,
        acknowledged_anchor_x,
        acknowledged_anchor_y,
        acknowledged_anchor_width,
        acknowledged_anchor_height
    );
    (transaction, record, expected)
}

pub fn resource_begin() -> (u64, u16, ContentResourceBegin, BTreeMap<&'static str, i128>) {
    let transaction = 707u64;
    let slot = 2u16;
    let grant_connection_epoch = 91u64;
    let grant_content_epoch = 92u64;
    let resource_id = 201u64;
    let resource_generation = 202u64;
    let width_px = 10u32;
    let height_px = 5u32;
    let rendered_scale_numerator = 7u32;
    let rendered_scale_denominator = 3u32;
    let pixel_format = 1u16;
    let chunk_count = 1u32;
    let total_bytes = 200u64;
    let record = ContentResourceBegin {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        resource: rid(resource_id, resource_generation),
        width_px,
        height_px,
        rendered_scale_numerator,
        rendered_scale_denominator,
        pixel_format,
        chunk_count,
        total_bytes,
    };
    let expected = fmap!(
        transaction,
        slot,
        grant_connection_epoch,
        grant_content_epoch,
        resource_id,
        resource_generation,
        width_px,
        height_px,
        rendered_scale_numerator,
        rendered_scale_denominator,
        pixel_format,
        chunk_count,
        total_bytes
    );
    (transaction, slot, record, expected)
}

pub fn resource_end() -> (u64, ContentResourceEnd, BTreeMap<&'static str, i128>) {
    let transaction = 808u64;
    let grant_connection_epoch = 101u64;
    let grant_content_epoch = 102u64;
    let resource_id = 203u64;
    let resource_generation = 204u64;
    let total_bytes = 50_000u64;
    let chunk_count = 17u32;
    let record = ContentResourceEnd {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        resource: rid(resource_id, resource_generation),
        total_bytes,
        chunk_count,
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        resource_id,
        resource_generation,
        total_bytes,
        chunk_count
    );
    (transaction, record, expected)
}

pub fn resource_cancel() -> (u64, ContentResourceCancel, BTreeMap<&'static str, i128>) {
    let transaction = 909u64;
    let grant_connection_epoch = 111u64;
    let grant_content_epoch = 112u64;
    let resource_id = 205u64;
    let resource_generation = 206u64;
    let record = ContentResourceCancel {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        resource: rid(resource_id, resource_generation),
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        resource_id,
        resource_generation
    );
    (transaction, record, expected)
}

pub fn resource_retire() -> (u64, ContentResourceRetire, BTreeMap<&'static str, i128>) {
    let transaction = 910u64;
    let grant_connection_epoch = 121u64;
    let grant_content_epoch = 122u64;
    let resource_id = 207u64;
    let resource_generation = 208u64;
    let record = ContentResourceRetire {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        resource: rid(resource_id, resource_generation),
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        resource_id,
        resource_generation
    );
    (transaction, record, expected)
}

pub fn resource_status() -> (u64, ContentResourceStatus, BTreeMap<&'static str, i128>) {
    let transaction = 911u64;
    let grant_connection_epoch = 131u64;
    let grant_content_epoch = 132u64;
    let resource_id = 209u64;
    let resource_generation = 210u64;
    let status = 3u16;
    let reason = 5u16;
    let next_ordinal = 42u32;
    let admitted_bytes = 123_456u64;
    let record = ContentResourceStatus {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        resource: rid(resource_id, resource_generation),
        status,
        reason,
        next_ordinal,
        admitted_bytes,
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        resource_id,
        resource_generation,
        status,
        reason,
        next_ordinal,
        admitted_bytes
    );
    (transaction, record, expected)
}

pub fn resource_released() -> (u64, ContentResourceReleased, BTreeMap<&'static str, i128>) {
    let transaction = 912u64;
    let grant_connection_epoch = 141u64;
    let grant_content_epoch = 142u64;
    let resource_id = 211u64;
    let resource_generation = 212u64;
    let reason = 7u16;
    let record = ContentResourceReleased {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        resource: rid(resource_id, resource_generation),
        reason,
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        resource_id,
        resource_generation,
        reason
    );
    (transaction, record, expected)
}

pub fn frame_demand() -> (u64, ContentFrameDemand, BTreeMap<&'static str, i128>) {
    let transaction = 913u64;
    let grant_connection_epoch = 151u64;
    let grant_content_epoch = 152u64;
    let output_id = 213u64;
    let output_generation = 214u64;
    let allocation_id = 215u64;
    let allocation_generation = 216u64;
    let demand_id = 99u64;
    let reason = 2u16;
    let record = ContentFrameDemand {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        output: oid(output_id, output_generation),
        allocation: aid(allocation_id, allocation_generation),
        demand_id,
        reason,
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        output_id,
        output_generation,
        allocation_id,
        allocation_generation,
        demand_id,
        reason
    );
    (transaction, record, expected)
}

pub fn frame_permit() -> (u64, ContentFramePermit, BTreeMap<&'static str, i128>) {
    let transaction = 914u64;
    let grant_connection_epoch = 161u64;
    let grant_content_epoch = 162u64;
    let output_id = 217u64;
    let output_generation = 218u64;
    let demand_id = 100u64;
    let permit_id = 101u64;
    let state = 1u16;
    let reason = 0u16;
    let ttl_ms = 200u32;
    let max_candidate_bytes = 4096u32;
    let record = ContentFramePermit {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        output: oid(output_id, output_generation),
        demand_id,
        permit_id,
        state,
        reason,
        ttl_ms,
        max_candidate_bytes,
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        output_id,
        output_generation,
        demand_id,
        permit_id,
        state,
        reason,
        ttl_ms,
        max_candidate_bytes
    );
    (transaction, record, expected)
}

pub fn frame_demand_cancel() -> (u64, ContentFrameDemandCancel, BTreeMap<&'static str, i128>) {
    let transaction = 915u64;
    let grant_connection_epoch = 171u64;
    let grant_content_epoch = 172u64;
    let output_id = 219u64;
    let output_generation = 220u64;
    let demand_id = 102u64;
    let permit_id = 103u64;
    let record = ContentFrameDemandCancel {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        output: oid(output_id, output_generation),
        demand_id,
        permit_id,
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        output_id,
        output_generation,
        demand_id,
        permit_id
    );
    (transaction, record, expected)
}

pub fn action() -> (u64, ContentAction, BTreeMap<&'static str, i128>) {
    let transaction = 916u64;
    let grant_connection_epoch = 181u64;
    let grant_content_epoch = 182u64;
    let output_id = 221u64;
    let output_generation = 222u64;
    let candidate_generation = 51u64;
    let presentation_epoch = 52u64;
    let interaction_generation = 53u64;
    let allocation_id = 223u64;
    let allocation_generation = 224u64;
    let target_id = 54u64;
    let target_generation = 55u64;
    let action_id = 56u64;
    let event_id = 57u64;
    let kind = 1u16;
    let reason = 0u16;
    let record = ContentAction {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        output: oid(output_id, output_generation),
        candidate_generation,
        presentation_epoch,
        interaction_generation,
        allocation: aid(allocation_id, allocation_generation),
        target_id,
        target_generation,
        action_id,
        event_id,
        kind,
        reason,
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        output_id,
        output_generation,
        candidate_generation,
        presentation_epoch,
        interaction_generation,
        allocation_id,
        allocation_generation,
        target_id,
        target_generation,
        action_id,
        event_id,
        kind,
        reason
    );
    (transaction, record, expected)
}

pub fn action_ack() -> (u64, ContentActionAck, BTreeMap<&'static str, i128>) {
    let transaction = 917u64;
    let grant_connection_epoch = 191u64;
    let grant_content_epoch = 192u64;
    let output_id = 225u64;
    let output_generation = 226u64;
    let candidate_generation = 61u64;
    let presentation_epoch = 62u64;
    let interaction_generation = 63u64;
    let allocation_id = 227u64;
    let allocation_generation = 228u64;
    let target_id = 64u64;
    let target_generation = 65u64;
    let action_id = 66u64;
    let event_id = 67u64;
    let disposition = 2u16;
    let record = ContentActionAck {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        output: oid(output_id, output_generation),
        candidate_generation,
        presentation_epoch,
        interaction_generation,
        allocation: aid(allocation_id, allocation_generation),
        target_id,
        target_generation,
        action_id,
        event_id,
        disposition,
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        output_id,
        output_generation,
        candidate_generation,
        presentation_epoch,
        interaction_generation,
        allocation_id,
        allocation_generation,
        target_id,
        target_generation,
        action_id,
        event_id,
        disposition
    );
    (transaction, record, expected)
}

pub fn candidate_outcome() -> (u64, ContentCandidateOutcome, BTreeMap<&'static str, i128>) {
    let transaction = 918u64;
    let grant_connection_epoch = 201u64;
    let grant_content_epoch = 202u64;
    let candidate_generation = 71u64;
    let output_id = 229u64;
    let output_generation = 230u64;
    let kind = 2u16;
    let reason = 0u16;
    let presentation_epoch = 72u64;
    let work_area_generation = 73u64;
    let wm_commit_generation = 74u64;
    let record = ContentCandidateOutcome {
        grant: grant(grant_connection_epoch, grant_content_epoch),
        candidate_generation,
        output: oid(output_id, output_generation),
        kind,
        reason,
        presentation_epoch,
        work_area_generation,
        wm_commit_generation,
    };
    let expected = fmap!(
        transaction,
        grant_connection_epoch,
        grant_content_epoch,
        candidate_generation,
        output_id,
        output_generation,
        kind,
        reason,
        presentation_epoch,
        work_area_generation,
        wm_commit_generation
    );
    (transaction, record, expected)
}
