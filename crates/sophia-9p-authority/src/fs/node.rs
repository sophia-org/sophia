//! Synthetic filesystem node definitions and path resolution.

use sophia_protocol::ids::SurfaceId;

use crate::types::Qid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeKind {
    Root,
    New,
    WindowDir(SurfaceId),
    Ctl(SurfaceId),
    Data(SurfaceId),
    Refresh(SurfaceId),
    Mouse(SurfaceId),
    Kbd(SurfaceId),
    Text(SurfaceId),
}

impl NodeKind {
    pub fn to_qid(self, version: u32) -> Qid {
        match self {
            Self::Root => Qid::new(Qid::QTDIR, version, 1),
            Self::New => Qid::new(Qid::QTFILE, version, 2),
            Self::WindowDir(id) => {
                let path = ((id.index() as u64) << 8) | 0x10;
                Qid::new(Qid::QTDIR, version, path)
            }
            Self::Ctl(id) => {
                let path = ((id.index() as u64) << 8) | 0x11;
                Qid::new(Qid::QTFILE, version, path)
            }
            Self::Data(id) => {
                let path = ((id.index() as u64) << 8) | 0x12;
                Qid::new(Qid::QTFILE, version, path)
            }
            Self::Refresh(id) => {
                let path = ((id.index() as u64) << 8) | 0x13;
                Qid::new(Qid::QTFILE, version, path)
            }
            Self::Mouse(id) => {
                let path = ((id.index() as u64) << 8) | 0x14;
                Qid::new(Qid::QTFILE, version, path)
            }
            Self::Kbd(id) => {
                let path = ((id.index() as u64) << 8) | 0x15;
                Qid::new(Qid::QTFILE, version, path)
            }
            Self::Text(id) => {
                let path = ((id.index() as u64) << 8) | 0x16;
                Qid::new(Qid::QTFILE, version, path)
            }
        }
    }

    pub fn is_dir(self) -> bool {
        matches!(self, Self::Root | Self::WindowDir(_))
    }
}
