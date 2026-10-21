use super::*;
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let (head, tail) = self.0.split_at_checked(N).ok_or(Error::Invalid)?;
        self.0 = tail;
        head.try_into().map_err(|_| Error::Invalid)
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take()?))
    }
    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.take()?))
    }
}
impl Batch {
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > MAX_BATCH_BYTES {
            return Err(Error::Limit);
        }
        let mut r = Reader(bytes);
        if r.take::<4>()? != *b"DRA1" {
            return Err(Error::Invalid);
        }
        let transaction = r.u64()?;
        let grant = ContentGrant {
            connection_epoch: r.u64()?,
            content_grant_epoch: r.u64()?,
        };
        let resource = ContentResourceId {
            id: r.u64()?,
            generation: r.u64()?,
        };
        let (width, height, count) = (r.u16()?, r.u16()?, r.u16()?);
        if usize::from(count) > MAX_COMMANDS {
            return Err(Error::Limit);
        }
        let mut commands = Vec::with_capacity(count.into());
        for _ in 0..count {
            let cmd = match r.u8()? {
                1 => Command::Rect {
                    x: r.u16()?,
                    y: r.u16()?,
                    width: r.u16()?,
                    height: r.u16()?,
                    rgb: r.take()?,
                },
                2 => {
                    let (x, y, size, rgb, len) = (r.u16()?, r.u16()?, r.u8()?, r.take()?, r.u8()?);
                    let (text, tail) = r.0.split_at_checked(len.into()).ok_or(Error::Invalid)?;
                    r.0 = tail;
                    Command::Text {
                        x,
                        y,
                        size,
                        rgb,
                        text: std::str::from_utf8(text)
                            .map_err(|_| Error::Invalid)?
                            .into(),
                    }
                }
                _ => return Err(Error::Invalid),
            };
            commands.push(cmd);
        }
        if !r.0.is_empty() {
            return Err(Error::Invalid);
        }
        let batch = Self {
            transaction,
            grant,
            resource,
            width,
            height,
            commands,
        };
        batch.validate()?;
        Ok(batch)
    }
    pub fn encode(&self) -> Result<Vec<u8>, Error> {
        self.validate()?;
        let mut bytes = b"DRA1".to_vec();
        for value in [
            self.transaction,
            self.grant.connection_epoch,
            self.grant.content_grant_epoch,
            self.resource.id,
            self.resource.generation,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        for value in [self.width, self.height, self.commands.len() as u16] {
            bytes.extend(value.to_le_bytes());
        }
        for command in &self.commands {
            match command {
                Command::Rect {
                    x,
                    y,
                    width,
                    height,
                    rgb,
                } => {
                    bytes.push(1);
                    for value in [x, y, width, height] {
                        bytes.extend(value.to_le_bytes());
                    }
                    bytes.extend(rgb);
                }
                Command::Text {
                    x,
                    y,
                    size,
                    rgb,
                    text,
                } => {
                    bytes.push(2);
                    bytes.extend(x.to_le_bytes());
                    bytes.extend(y.to_le_bytes());
                    bytes.push(*size);
                    bytes.extend(rgb);
                    bytes.push(text.len() as u8);
                    bytes.extend(text.as_bytes());
                }
            }
        }
        if bytes.len() > MAX_BATCH_BYTES {
            return Err(Error::Limit);
        }
        Ok(bytes)
    }
}
