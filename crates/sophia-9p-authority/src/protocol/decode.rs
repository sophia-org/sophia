//! 9P2000 binary wire decoding.

use crate::types::{Fid, TMessage, Tag};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodeError {
    UnexpectedEof,
    InvalidUtf8,
    UnknownMessageType(u8),
    OversizedMessage(u32),
}

/// Decodes a 9P string: `len: u16`, followed by UTF-8 bytes.
pub fn decode_string(buf: &[u8], offset: &mut usize) -> Result<String, DecodeError> {
    if *offset + 2 > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let len = u16::from_le_bytes([buf[*offset], buf[*offset + 1]]) as usize;
    *offset += 2;

    if *offset + len > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let s = std::str::from_utf8(&buf[*offset..*offset + len])
        .map_err(|_| DecodeError::InvalidUtf8)?
        .to_string();
    *offset += len;
    Ok(s)
}

pub fn decode_u16(buf: &[u8], offset: &mut usize) -> Result<u16, DecodeError> {
    if *offset + 2 > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let val = u16::from_le_bytes([buf[*offset], buf[*offset + 1]]);
    *offset += 2;
    Ok(val)
}

pub fn decode_u32(buf: &[u8], offset: &mut usize) -> Result<u32, DecodeError> {
    if *offset + 4 > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let val = u32::from_le_bytes([
        buf[*offset],
        buf[*offset + 1],
        buf[*offset + 2],
        buf[*offset + 3],
    ]);
    *offset += 4;
    Ok(val)
}

pub fn decode_u64(buf: &[u8], offset: &mut usize) -> Result<u64, DecodeError> {
    if *offset + 8 > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&buf[*offset..*offset + 8]);
    *offset += 8;
    Ok(u64::from_le_bytes(bytes))
}

pub fn decode_u8(buf: &[u8], offset: &mut usize) -> Result<u8, DecodeError> {
    if *offset + 1 > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }
    let val = buf[*offset];
    *offset += 1;
    Ok(val)
}

/// Decodes an incoming 9P request frame into a `(Tag, TMessage)`.
pub fn decode_t_message(buf: &[u8]) -> Result<(Tag, TMessage), DecodeError> {
    if buf.len() < 7 {
        return Err(DecodeError::UnexpectedEof);
    }

    let mut offset = 0;
    let size = decode_u32(buf, &mut offset)?;
    if size as usize > buf.len() {
        return Err(DecodeError::UnexpectedEof);
    }

    let msg_type = decode_u8(buf, &mut offset)?;
    let tag = Tag::new(decode_u16(buf, &mut offset)?);

    let msg = match msg_type {
        100 => {
            // Tversion
            let msize = decode_u32(buf, &mut offset)?;
            let version = decode_string(buf, &mut offset)?;
            TMessage::Version { msize, version }
        }
        102 => {
            // Tauth
            let afid = Fid::new(decode_u32(buf, &mut offset)?);
            let uname = decode_string(buf, &mut offset)?;
            let aname = decode_string(buf, &mut offset)?;
            TMessage::Auth { afid, uname, aname }
        }
        104 => {
            // Tattach
            let fid = Fid::new(decode_u32(buf, &mut offset)?);
            let afid = Fid::new(decode_u32(buf, &mut offset)?);
            let uname = decode_string(buf, &mut offset)?;
            let aname = decode_string(buf, &mut offset)?;
            TMessage::Attach {
                fid,
                afid,
                uname,
                aname,
            }
        }
        108 => {
            // Tflush
            let oldtag = Tag::new(decode_u16(buf, &mut offset)?);
            TMessage::Flush { oldtag }
        }
        110 => {
            // Twalk
            let fid = Fid::new(decode_u32(buf, &mut offset)?);
            let newfid = Fid::new(decode_u32(buf, &mut offset)?);
            let nwname = decode_u16(buf, &mut offset)? as usize;
            let mut wnames = Vec::with_capacity(nwname);
            for _ in 0..nwname {
                wnames.push(decode_string(buf, &mut offset)?);
            }
            TMessage::Walk {
                fid,
                newfid,
                wnames,
            }
        }
        112 => {
            // Topen
            let fid = Fid::new(decode_u32(buf, &mut offset)?);
            let mode = decode_u8(buf, &mut offset)?;
            TMessage::Open { fid, mode }
        }
        114 => {
            // Tcreate
            let fid = Fid::new(decode_u32(buf, &mut offset)?);
            let name = decode_string(buf, &mut offset)?;
            let perm = decode_u32(buf, &mut offset)?;
            let mode = decode_u8(buf, &mut offset)?;
            TMessage::Create {
                fid,
                name,
                perm,
                mode,
            }
        }
        116 => {
            // Tread
            let fid = Fid::new(decode_u32(buf, &mut offset)?);
            let offset_val = decode_u64(buf, &mut offset)?;
            let count = decode_u32(buf, &mut offset)?;
            TMessage::Read {
                fid,
                offset: offset_val,
                count,
            }
        }
        118 => {
            // Twrite
            let fid = Fid::new(decode_u32(buf, &mut offset)?);
            let offset_val = decode_u64(buf, &mut offset)?;
            let count = decode_u32(buf, &mut offset)? as usize;
            if offset + count > buf.len() {
                return Err(DecodeError::UnexpectedEof);
            }
            let data = buf[offset..offset + count].to_vec();
            TMessage::Write {
                fid,
                offset: offset_val,
                data,
            }
        }
        120 => {
            // Tclunk
            let fid = Fid::new(decode_u32(buf, &mut offset)?);
            TMessage::Clunk { fid }
        }
        122 => {
            // Tremove
            let fid = Fid::new(decode_u32(buf, &mut offset)?);
            TMessage::Remove { fid }
        }
        124 => {
            // Tstat
            let fid = Fid::new(decode_u32(buf, &mut offset)?);
            TMessage::Stat { fid }
        }
        126 => {
            // Twstat
            let fid = Fid::new(decode_u32(buf, &mut offset)?);
            let len = decode_u16(buf, &mut offset)? as usize;
            if offset + len > buf.len() {
                return Err(DecodeError::UnexpectedEof);
            }
            let stat_bytes = buf[offset..offset + len].to_vec();
            TMessage::Wstat { fid, stat_bytes }
        }
        unknown => return Err(DecodeError::UnknownMessageType(unknown)),
    };

    Ok((tag, msg))
}
