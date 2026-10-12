use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Stage {
    RelayAttach,
    RelayWalk,
    RelayOpen,
    Auth,
    Challenge,
    RelayWrite,
    RelayRead,
    Status,
    Attach,
    AfidClunk,
    RpcClunk,
    RootClunk,
    Done,
}
impl Stage {
    pub(super) fn relay(self) -> bool {
        matches!(
            self,
            Self::RelayAttach
                | Self::RelayWalk
                | Self::RelayOpen
                | Self::RelayWrite
                | Self::RelayRead
                | Self::RpcClunk
                | Self::RootClunk
        )
    }
}
impl Client {
    pub(super) fn start(&mut self) -> Result<Tag, Error> {
        let role = self.role.as_mut().unwrap();
        let relay = self.relay.as_mut().unwrap();
        Ok(match self.stage {
            Stage::RelayAttach => {
                let (tag, fid) = relay.attach(b"", b"sophia-launch-relay-v1")?;
                self.relay_root = Some(fid);
                tag
            }
            Stage::RelayWalk => {
                let (tag, fid) = relay.walk(self.relay_root.unwrap(), &[b"rpc"])?;
                self.rpc = Some(fid);
                tag
            }
            Stage::RelayOpen => relay.lopen(self.rpc.unwrap(), 2)?,
            Stage::Auth => {
                let (tag, fid) = role.auth(&self.claims[0], &self.claims[1])?;
                self.afid = Some(fid);
                tag
            }
            Stage::Challenge => role.read(self.afid.unwrap(), 0, 40)?,
            Stage::RelayWrite => relay.write(self.rpc.unwrap(), 0, &self.challenge)?,
            Stage::RelayRead => relay.read(self.rpc.unwrap(), 0, 8)?,
            Stage::Status => role.read(self.afid.unwrap(), 40, 8)?,
            Stage::Attach => {
                let (tag, fid) =
                    role.attach_with_afid(self.afid.unwrap(), &self.claims[0], &self.claims[1])?;
                self.root = Some(fid);
                tag
            }
            Stage::AfidClunk => role.clunk(self.afid.unwrap())?,
            Stage::RpcClunk => relay.clunk(self.rpc.unwrap())?,
            Stage::RootClunk => relay.clunk(self.relay_root.unwrap())?,
            Stage::Done => return Err(Error::Invalid("request after completion")),
        })
    }
    pub(super) fn answer(&mut self, reply: Reply) -> Result<(), Error> {
        if let Reply::Error(errno) = reply {
            return Err(Error::Remote(errno));
        }
        self.stage = match (self.stage, reply) {
            (Stage::RelayAttach, Reply::Attach(_)) => Stage::RelayWalk,
            (Stage::RelayWalk, Reply::Walk(qids)) if qids.len() == 1 => Stage::RelayOpen,
            (Stage::RelayOpen, Reply::Lopen { .. }) => Stage::Auth,
            (Stage::Auth, Reply::Auth(_)) => Stage::Challenge,
            (Stage::Challenge, Reply::Read(bytes))
                if bytes.len() == 40 && bytes[..8] == [1, 0, 0, 0, 1, 0, 0, 0] =>
            {
                self.challenge.copy_from_slice(&bytes);
                Stage::RelayWrite
            }
            (Stage::RelayWrite, Reply::Write(40)) => Stage::RelayRead,
            (Stage::RelayRead, Reply::Read(bytes)) => {
                status(&bytes)?;
                Stage::Status
            }
            (Stage::Status, Reply::Read(bytes)) => {
                status(&bytes)?;
                Stage::Attach
            }
            (Stage::Attach, Reply::Attach(_)) => Stage::AfidClunk,
            (Stage::AfidClunk, Reply::Clunk) => Stage::RpcClunk,
            (Stage::RpcClunk, Reply::Clunk) => Stage::RootClunk,
            (Stage::RootClunk, Reply::Clunk) => Stage::Done,
            _ => return Err(Error::Invalid("record shape or step")),
        };
        Ok(())
    }
}
fn status(bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() != 8 || bytes[..4] != [1, 0, 0, 0] {
        return Err(Error::Invalid("status record"));
    }
    let errno = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if errno != 0 {
        return Err(Error::Remote(Errno(errno)));
    }
    Ok(())
}
