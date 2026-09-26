//! A bounded 9P2000.L server core: the wire codec, one connection's protocol
//! state, and the seam through which an owner exports its nodes.
//!
//! WHAT IT OWNS. Framing, version and message-size negotiation, tags, fids,
//! walk and open rules, flush, and the teardown a disconnect implies. Every
//! quantity a peer controls is bounded by [`Limits`].
//!
//! WHAT IT DOES NOT. It has no role semantics and no authority of its own.
//! Nodes and open handles are the exporting owner's opaque values, and every
//! operation on a fid is put to the owner's [`Export::check`] first, so the
//! owner decides admission, disclosure and revocation. Attach names, user
//! names and peer credentials reach the owner as data and grant nothing here.
//!
//! [`client`] is a separate bounded, read-only client with its own codec; it
//! shares only the protocol's value records with the server core.
//!
//! The older `sophia-9p-authority` scaffold is unrelated and unused.

pub mod client;
pub mod connection;
pub mod export;
pub mod records;
pub mod unix;
pub mod wire;

pub use connection::{Connection, ConnectionId, Fatal};
pub use export::{
    Access, AttachContext, Attachment, DirEntry, Entry, Epoch, Export, NodeKind, Operation,
    PeerCredentials, ReadOutcome, WalkName,
};
pub use records::{
    Errno, Fid, Limits, LimitsError, OpenAccess, OpenFlags, Qid, QidKind, Reply, Request, Tag,
};
