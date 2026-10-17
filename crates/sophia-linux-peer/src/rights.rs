//! Inspect every kernel control message, including kinds rustix skips.
use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

pub struct ReceivedRights {
    pub bytes: usize,
    pub descriptors: [Option<OwnedFd>; 4],
    pub count: usize,
}

/// Nonblocking recvmsg with CLOEXEC. Only SCM_RIGHTS is accepted. All installed
/// rights (including an unexpected Linux SCM_PIDFD) are owned before returning
/// an error. The fixed control buffer bounds parsing even for hostile peers.
pub fn receive_rights(socket: BorrowedFd<'_>, bytes: &mut [u8]) -> io::Result<ReceivedRights> {
    let mut result = ReceivedRights {
        bytes: 0,
        descriptors: std::array::from_fn(|_| None),
        count: 0,
    };
    // u64 alignment is sufficient for cmsghdr on supported Linux targets.
    let mut control = [0_u64; 32];
    let mut iov = libc::iovec {
        iov_base: bytes.as_mut_ptr().cast(),
        iov_len: bytes.len(),
    };
    // SAFETY: zero is a valid empty msghdr; every pointer below names a live
    // writable buffer for exactly the advertised length, through recvmsg.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = control.as_mut_ptr().cast();
    msg.msg_controllen = std::mem::size_of_val(&control);
    // SAFETY: all buffers above remain live; the socket is borrowed.
    let received = unsafe {
        libc::recvmsg(
            socket.as_raw_fd(),
            &mut msg,
            libc::MSG_DONTWAIT | libc::MSG_CMSG_CLOEXEC,
        )
    };
    if received < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut invalid = msg.msg_flags & (libc::MSG_TRUNC | libc::MSG_CTRUNC) != 0;
    // SAFETY: recvmsg initialized the control prefix and its lengths. CMSG
    // traversal stays inside that prefix; data integers are read unaligned.
    unsafe {
        let mut header = libc::CMSG_FIRSTHDR(&msg);
        while !header.is_null() {
            let h = &*header;
            let base = libc::CMSG_LEN(0) as usize;
            if h.cmsg_len < base {
                // The kernel supplies these headers, not the sender's raw
                // bytes; a short header cannot precede installed rights.
                invalid = true;
                break;
            }
            let length = h.cmsg_len - base;
            let rights = h.cmsg_level == libc::SOL_SOCKET && h.cmsg_type == libc::SCM_RIGHTS;
            // Linux uapi SCM_PIDFD = 0x04; libc exposes SO_PASSPIDFD but not
            // this message constant. Its payload is one installed descriptor.
            let pidfd = h.cmsg_level == libc::SOL_SOCKET && h.cmsg_type == 0x04;
            if !rights {
                invalid = true;
            }
            if rights || pidfd {
                if !length.is_multiple_of(std::mem::size_of::<libc::c_int>()) {
                    invalid = true;
                }
                let data = libc::CMSG_DATA(header);
                for offset in (0..length / std::mem::size_of::<libc::c_int>())
                    .map(|i| i * std::mem::size_of::<libc::c_int>())
                {
                    let raw = std::ptr::read_unaligned(data.add(offset).cast::<libc::c_int>());
                    // Each received integer is a newly installed reference.
                    let fd = OwnedFd::from_raw_fd(raw);
                    if rights && result.count < result.descriptors.len() {
                        result.descriptors[result.count] = Some(fd);
                        result.count += 1;
                    } else {
                        invalid = true;
                    }
                }
            }
            header = libc::CMSG_NXTHDR(&msg, header);
        }
    }
    if invalid {
        return Err(io::Error::from_raw_os_error(libc::EPROTO));
    }
    result.bytes = received as usize;
    Ok(result)
}
