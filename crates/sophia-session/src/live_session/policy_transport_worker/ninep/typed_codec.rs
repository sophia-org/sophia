//! Complete file values use the shared codecs directly, never IPC transfers.
use super::*;
use sophia_runtime::PolicyProfileHandoffKind;

pub(super) struct TypedFileCodec;

pub(super) fn codec_error(error: WmFilePayloadError) -> Errno {
    match error {
        WmFilePayloadError::Capabilities { .. } => Errno::EACCES,
        _ => Errno::EINVAL,
    }
}

impl PolicyFileCodec for TypedFileCodec {
    fn decode_candidate(&self, bytes: &[u8], selected: u64) -> Result<DecodedFileCandidate, Errno> {
        let record =
            decode_wm_file_record(bytes, WmFileClass::Candidate).map_err(|_| Errno::EINVAL)?;
        let event = match record.header.kind {
            WmFileKind::Negotiate => PolicyAdapterEvent::Negotiation(
                decode_wm_file_negotiate(bytes).map_err(codec_error)?,
            ),
            WmFileKind::ProfilePrepared
            | WmFileKind::ProfileActive
            | WmFileKind::ProfileRolledBack => {
                let kind = match record.header.kind {
                    WmFileKind::ProfilePrepared => PolicyProfileHandoffKind::Prepare,
                    WmFileKind::ProfileActive => PolicyProfileHandoffKind::Activate,
                    _ => PolicyProfileHandoffKind::Rollback,
                };
                PolicyAdapterEvent::ProfileCompletion {
                    kind,
                    completion: decode_wm_file_profile_completion(
                        bytes,
                        record.header.kind,
                        selected,
                    )
                    .map_err(codec_error)?,
                }
            }
            WmFileKind::Configuration => {
                let value = decode_wm_file_configuration(bytes, selected).map_err(codec_error)?;
                PolicyAdapterEvent::Configuration {
                    transaction: value.transaction,
                    configuration: value.configuration,
                }
            }
            WmFileKind::Projection => PolicyAdapterEvent::Projection(Box::new(
                decode_wm_file_projection(bytes, selected).map_err(codec_error)?,
            )),
            WmFileKind::Dirty => PolicyAdapterEvent::Dirty(
                decode_wm_file_dirty(bytes, selected).map_err(codec_error)?,
            ),
            WmFileKind::SessionOperation => {
                let value =
                    decode_wm_file_session_operation(bytes, selected).map_err(codec_error)?;
                PolicyAdapterEvent::SessionOperation {
                    transaction: value.transaction,
                    request: value.request,
                }
            }
            _ => return Err(Errno::EINVAL),
        };
        // Complete codecs enforce all kind/section capabilities against the
        // owner's selected set, including extensions. There is no client mask.
        Ok(DecodedFileCandidate {
            event,
            required_capabilities: 0,
        })
    }
    fn submitted_body(
        &self,
        submission_id: u64,
        candidate_kind: WmFileKind,
    ) -> Result<Vec<u8>, Errno> {
        encode_wm_file_submitted_body(WmFileSubmitted {
            submission_id,
            candidate_kind,
        })
        .map_err(codec_error)
    }
}
