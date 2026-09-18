//! Session's private input service.
//!
//! WHAT SESSION OWNS AND WHAT IT HANDS OUT. Session keeps the authority
//! instance, the issuer, the namespace and admission registry, the durable
//! settlement store, the service owner, and the receiving end of every
//! acknowledgement, delivery and transaction channel. What an adapter
//! receives is a submission handle and nothing else: no lease, no owner, no
//! issuer, no broker, no sender. The separation is the point, so it is
//! expressed in the types rather than in a convention somebody has to keep.
//!
//! THE SERVICE THREAD OUTLIVES THE INVOCATION. The execution keeper is made
//! on the thread that serves and stays there, outside the invocation and
//! outside any unwind boundary, so an invocation that fails or unwinds still
//! hands its keyboard history, supervisor and accounting to a keeper that is
//! still standing. The owner and the maintenance channel outlive the
//! invocation for the same reason. Nothing here reconstructs a keeper, and
//! nothing resets an interruption once it is recorded.

mod admission;
mod committed;
mod config;
mod control;
mod handle;
mod service;
mod submission;

pub use admission::{PrivateInputAdmissionRecord, PrivateInputIssueRefusal};
pub use config::{PrivateInputConfig, PrivateInputGrantPolicy, PrivateInputInstanceCookie};
pub use control::{
    PrivateInputAction, PrivateInputCommitted, PrivateInputCommittedEffect,
    PrivateInputControlError, PrivateInputSubmitted,
};
pub use handle::{
    PrivateInputHandle, PrivateInputOutcome, PrivateInputReadiness, PrivateInputRefusal,
    PrivateInputService, PrivateInputSettlement, PrivateInputStatus, PrivateInputThreadJoin,
    PrivateInputTopologyRefusal, PrivateInputUnavailable, PrivateInputWaitExpired,
};
pub use submission::{
    PrivateInputAccepted, PrivateInputConnection, PrivateInputSubmission, PrivateInputSubmitError,
};
