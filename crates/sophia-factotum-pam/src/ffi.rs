//! The only unsafe code in the helper: process hardening, a locked page for
//! the request, and a minimal libpam binding.
//!
//! Every block states the invariant it relies on. Nothing here panics: a
//! panic inside the PAM conversation callback would cross a C frame.

use sophia_factotum::proto::PamVerdict;
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::ptr;

// Linux-PAM `_pam_types.h`.
const PAM_SUCCESS: c_int = 0;
const PAM_BUF_ERR: c_int = 5;
const PAM_PERM_DENIED: c_int = 6;
const PAM_AUTH_ERR: c_int = 7;
const PAM_CRED_INSUFFICIENT: c_int = 8;
const PAM_USER_UNKNOWN: c_int = 10;
const PAM_MAXTRIES: c_int = 11;
const PAM_CONV_ERR: c_int = 19;
const PAM_SILENT: c_int = 0x8000;
const PAM_DISALLOW_NULL_AUTHTOK: c_int = 0x0001;
const PAM_REINITIALIZE_CRED: c_int = 0x0008;
const PAM_PROMPT_ECHO_OFF: c_int = 1;
const PAM_ERROR_MSG: c_int = 3;
const PAM_TEXT_INFO: c_int = 4;
/// Linux-PAM's `PAM_MAX_NUM_MSG`.
const PAM_MAX_NUM_MSG: c_int = 32;

#[repr(C)]
struct PamHandle {
    _opaque: [u8; 0],
}

#[repr(C)]
struct PamMessage {
    msg_style: c_int,
    msg: *const c_char,
}

#[repr(C)]
struct PamResponse {
    resp: *mut c_char,
    resp_retcode: c_int,
}

type ConversationFn = unsafe extern "C" fn(
    c_int,
    *mut *const PamMessage,
    *mut *mut PamResponse,
    *mut c_void,
) -> c_int;

#[repr(C)]
struct PamConv {
    conv: Option<ConversationFn>,
    appdata_ptr: *mut c_void,
}

#[link(name = "pam")]
unsafe extern "C" {
    fn pam_start(
        service: *const c_char,
        user: *const c_char,
        conversation: *const PamConv,
        handle: *mut *mut PamHandle,
    ) -> c_int;
    fn pam_start_confdir(
        service: *const c_char,
        user: *const c_char,
        conversation: *const PamConv,
        confdir: *const c_char,
        handle: *mut *mut PamHandle,
    ) -> c_int;
    fn pam_authenticate(handle: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_setcred(handle: *mut PamHandle, flags: c_int) -> c_int;
    fn pam_end(handle: *mut PamHandle, status: c_int) -> c_int;
}

/// Makes the helper undumpable, gives it no core, closes every descriptor
/// past stdio and locks what it has mapped. Locking only current pages:
/// with `MCL_FUTURE` under a small memlock limit, a PAM module's allocation
/// could fail and a right password be refused.
pub fn harden() {
    let no_core = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: plain syscalls on this process with valid arguments; each
    // failure leaves the process as it was and is tolerated as best-effort,
    // except that a helper with a dumpable or core-writing process would
    // still never write the secret anywhere but its own memory.
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
        libc::setrlimit(libc::RLIMIT_CORE, &no_core);
        libc::syscall(libc::SYS_close_range, 3_u32, u32::MAX, 0_u32);
        libc::mlockall(libc::MCL_CURRENT);
    }
}

/// An anonymous mapping that is locked in memory, excluded from core dumps
/// and zeroed before it is unmapped. The request, and so the secret, is read
/// only into this.
pub struct LockedPage {
    base: *mut u8,
    len: usize,
}

impl LockedPage {
    pub fn new(len: usize) -> Option<Self> {
        // SAFETY: an anonymous private mapping takes no file or address; the
        // result is checked before use.
        let base = unsafe {
            libc::mmap(
                ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if base == libc::MAP_FAILED {
            return None;
        }
        // SAFETY: `base..base+len` is the mapping just created.
        let locked = unsafe {
            libc::madvise(base, len, libc::MADV_DONTDUMP) == 0 && libc::mlock(base, len) == 0
        };
        let page = Self {
            base: base.cast(),
            len,
        };
        locked.then_some(page)
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: the mapping is `len` readable and writable bytes, owned by
        // this value for its whole life.
        unsafe { std::slice::from_raw_parts_mut(self.base, self.len) }
    }
}

impl Drop for LockedPage {
    fn drop(&mut self) {
        let bytes = self.as_mut_slice();
        for byte in bytes.iter_mut() {
            // SAFETY: a valid, exclusively borrowed byte of the mapping; the
            // volatile write cannot be elided.
            unsafe { ptr::write_volatile(byte, 0) };
        }
        // SAFETY: unmaps exactly the mapping created in `new`.
        unsafe {
            libc::munmap(self.base.cast(), self.len);
        }
    }
}

/// What the conversation callback reads and records. It lives on the stack
/// of [`authenticate`] for the whole PAM transaction.
struct Exchange {
    secret: *const u8,
    secret_len: usize,
    answered: bool,
    refused: bool,
}

/// Answers the first echo-off prompt with the secret and drops information
/// messages. An echo-on prompt, a second echo-off prompt or anything else
/// refuses the conversation: this helper answers one secret and nothing a
/// person would need to see.
unsafe extern "C" fn conversation(
    count: c_int,
    messages: *mut *const PamMessage,
    responses: *mut *mut PamResponse,
    appdata: *mut c_void,
) -> c_int {
    if count <= 0
        || count > PAM_MAX_NUM_MSG
        || messages.is_null()
        || responses.is_null()
        || appdata.is_null()
    {
        return PAM_CONV_ERR;
    }
    // SAFETY: Linux-PAM passes the `appdata_ptr` it was given, which is the
    // `Exchange` borrowed for the transaction in `authenticate`.
    let exchange = unsafe { &mut *appdata.cast::<Exchange>() };
    let count = count as usize;
    // SAFETY: calloc of a small bounded array; PAM frees it with `free`.
    let answers =
        unsafe { libc::calloc(count, std::mem::size_of::<PamResponse>()) }.cast::<PamResponse>();
    if answers.is_null() {
        return PAM_BUF_ERR;
    }
    for index in 0..count {
        // SAFETY: Linux-PAM's `msg` is an array of `count` message pointers.
        let message = unsafe { *messages.add(index) };
        if message.is_null() {
            return refuse(exchange, answers, count);
        }
        // SAFETY: a non-null message from PAM.
        let style = unsafe { (*message).msg_style };
        match style {
            PAM_PROMPT_ECHO_OFF if !exchange.answered => {
                // SAFETY: allocates the secret plus a terminator; PAM frees it.
                let copy = unsafe { libc::malloc(exchange.secret_len + 1) }.cast::<u8>();
                if copy.is_null() {
                    return refuse(exchange, answers, count);
                }
                // SAFETY: `secret` holds `secret_len` bytes for the
                // transaction; `copy` holds one more for the terminator.
                unsafe {
                    ptr::copy_nonoverlapping(exchange.secret, copy, exchange.secret_len);
                    *copy.add(exchange.secret_len) = 0;
                    (*answers.add(index)).resp = copy.cast();
                }
                exchange.answered = true;
            }
            PAM_ERROR_MSG | PAM_TEXT_INFO => {}
            _ => return refuse(exchange, answers, count),
        }
    }
    // SAFETY: hands PAM the array it will free.
    unsafe { *responses = answers };
    PAM_SUCCESS
}

/// Frees what a refused conversation allocated, zeroing any copy of the
/// secret first, and records the refusal.
fn refuse(exchange: &mut Exchange, answers: *mut PamResponse, count: usize) -> c_int {
    for index in 0..count {
        // SAFETY: `answers` is the calloc'ed array of `count` responses; each
        // `resp` is null or a malloc'ed copy of the secret plus terminator.
        unsafe {
            let response = (*answers.add(index)).resp.cast::<u8>();
            if !response.is_null() {
                for offset in 0..exchange.secret_len {
                    ptr::write_volatile(response.add(offset), 0);
                }
                libc::free(response.cast());
            }
        }
    }
    // SAFETY: the array came from calloc above.
    unsafe { libc::free(answers.cast()) };
    exchange.refused = true;
    PAM_CONV_ERR
}

/// One PAM transaction: start, authenticate, optionally reinitialise
/// credentials, end. Returns the verdict and PAM's code.
pub fn authenticate(
    service: &str,
    user: &str,
    secret: &[u8],
    disallow_null: bool,
    setcred: bool,
    confdir: Option<&CStr>,
) -> (PamVerdict, i16) {
    let (Ok(service), Ok(user)) = (CString::new(service), CString::new(user)) else {
        return (PamVerdict::Unavailable, 0);
    };
    let mut exchange = Exchange {
        secret: secret.as_ptr(),
        secret_len: secret.len(),
        answered: false,
        refused: false,
    };
    let conversation = PamConv {
        conv: Some(conversation),
        appdata_ptr: (&raw mut exchange).cast(),
    };
    let mut handle = ptr::null_mut();
    // SAFETY: every pointer is a valid NUL-terminated string or a live local,
    // and `exchange` outlives the transaction, which ends below.
    let started = unsafe {
        match confdir {
            Some(confdir) => pam_start_confdir(
                service.as_ptr(),
                user.as_ptr(),
                &conversation,
                confdir.as_ptr(),
                &mut handle,
            ),
            None => pam_start(service.as_ptr(), user.as_ptr(), &conversation, &mut handle),
        }
    };
    if started != PAM_SUCCESS || handle.is_null() {
        return (PamVerdict::Unavailable, code(started));
    }
    let mut flags = PAM_SILENT;
    if disallow_null {
        flags |= PAM_DISALLOW_NULL_AUTHTOK;
    }
    // SAFETY: `handle` came from a successful start and is ended once below.
    let status = unsafe { pam_authenticate(handle, flags) };
    if status == PAM_SUCCESS && setcred {
        // Best-effort, as lockme did: a refusal here does not undo a verified
        // password.
        // SAFETY: as above.
        let _ = unsafe { pam_setcred(handle, PAM_REINITIALIZE_CRED | PAM_SILENT) };
    }
    // SAFETY: ends the transaction started above; `handle` is not used again.
    unsafe { pam_end(handle, status) };
    let verdict = match status {
        PAM_SUCCESS => PamVerdict::Accepted,
        _ if exchange.refused => PamVerdict::ConversationRefused,
        PAM_AUTH_ERR
        | PAM_USER_UNKNOWN
        | PAM_MAXTRIES
        | PAM_CRED_INSUFFICIENT
        | PAM_PERM_DENIED => PamVerdict::Rejected,
        _ => PamVerdict::Unavailable,
    };
    (verdict, code(status))
}

fn code(status: c_int) -> i16 {
    i16::try_from(status).unwrap_or(i16::MAX)
}
