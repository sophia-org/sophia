"""Small host Linux-PAM binding. Only the root worker uses the installed stack.

CPU controls explicitly use pam_start_confdir with a private fixture stack. No
password conversation, authenticate call, ambient PAM environment, or fallback.
The handle and module-owned session lifetime descriptors stay in this worker.
"""
import ctypes as C
import signal


class Message(C.Structure):
    _fields_ = [("style", C.c_int), ("message", C.c_char_p)]


class Response(C.Structure):
    _fields_ = [("response", C.c_void_p), ("code", C.c_int)]


Callback = C.CFUNCTYPE(C.c_int, C.c_int, C.POINTER(C.POINTER(Message)),
                     C.POINTER(C.POINTER(Response)), C.c_void_p)


class Conversation(C.Structure):
    _fields_ = [("callback", Callback), ("data", C.c_void_p)]


class Pam:
    def __init__(self, library, service, user, environment, *, fixture_directory=None):
        self.lib = C.CDLL(library)
        self.libc = C.CDLL(None)
        self.libc.calloc.argtypes = [C.c_size_t, C.c_size_t]
        self.libc.calloc.restype = C.c_void_p
        self.handle = C.c_void_p()
        self.opened = False
        self.callback = Callback(self.converse)
        self.conversation = Conversation(self.callback, None)
        for name in ("pam_acct_mgmt", "pam_open_session", "pam_close_session", "pam_end"):
            fn = getattr(self.lib, name)
            fn.argtypes = [C.c_void_p, C.c_int]
            fn.restype = C.c_int
        self.lib.pam_putenv.argtypes = [C.c_void_p, C.c_char_p]
        self.lib.pam_putenv.restype = C.c_int
        self.lib.pam_getenv.argtypes = [C.c_void_p, C.c_char_p]
        self.lib.pam_getenv.restype = C.c_char_p
        self.lib.pam_get_item.argtypes = [C.c_void_p, C.c_int, C.POINTER(C.c_void_p)]
        self.lib.pam_get_item.restype = C.c_int
        if fixture_directory is None:
            fn = self.lib.pam_start
            fn.argtypes = [C.c_char_p, C.c_char_p, C.POINTER(Conversation), C.POINTER(C.c_void_p)]
            args = [service.encode(), user.encode(), C.byref(self.conversation), C.byref(self.handle)]
        else:
            fn = self.lib.pam_start_confdir
            fn.argtypes = [C.c_char_p, C.c_char_p, C.POINTER(Conversation), C.c_char_p,
                           C.POINTER(C.c_void_p)]
            args = [service.encode(), user.encode(), C.byref(self.conversation),
                    str(fixture_directory).encode(), C.byref(self.handle)]
        fn.restype = C.c_int
        self.checked("pam_start", fn(*args))
        try:
            for key, value in environment.items():
                if "\0" in key + value or "=" in key:
                    raise ValueError("invalid PAM environment")
                self.checked("pam_putenv", self.lib.pam_putenv(self.handle, f"{key}={value}".encode()))
            self.checked("pam_acct_mgmt", self.lib.pam_acct_mgmt(self.handle, 0))
            value = C.c_void_p()
            self.checked("pam_get_item", self.lib.pam_get_item(self.handle, 2, C.byref(value)))
            if not value.value or C.string_at(value).decode() != user:
                raise ValueError("PAM changed the dedicated account")
        except BaseException as error:
            try:
                self.close()
            except ValueError as cleanup:
                raise ValueError(f"{error}; {cleanup}") from error
            raise

    def converse(self, count, messages, responses, _):
        # PAM takes ownership of the calloc array. Informational messages need
        # empty responses. Refuse every prompt, including unexpected username.
        if not 0 < count <= 32 or not messages or not responses:
            return 19
        try:
            if any(not messages[i] or messages[i].contents.style not in (3, 4)
                   for i in range(count)):
                return 19
            memory = self.libc.calloc(count, C.sizeof(Response))
            if not memory:
                return 5
            responses[0] = C.cast(memory, C.POINTER(Response))
            return 0
        except BaseException:
            return 19

    @staticmethod
    def checked(name, status):
        if status != 0:
            raise ValueError(f"{name} failed: {status}")

    def open(self):
        # A later module may refuse after an earlier one created resources.
        # Close that attempted session too; the owner also bounds this call.
        self.opened = True
        try:
            self.checked("pam_open_session", self.lib.pam_open_session(self.handle, 0))
        except BaseException as error:
            try:
                self.close()
            except ValueError as cleanup:
                raise ValueError(f"{error}; {cleanup}") from error
            raise

    def getenv(self, name):
        value = self.lib.pam_getenv(self.handle, name.encode())
        return None if value is None else value.decode("utf-8")

    def close(self):
        if not self.handle.value:
            return
        # close() is also used on a failed open before worker.finally. Delay
        # TERM until both PAM operations finish; an outer cleanup mask stays.
        previous = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM})
        try:
            status = self.lib.pam_close_session(self.handle, 0) if self.opened else 0
            self.opened = False
            end = self.lib.pam_end(self.handle, status)
            self.handle = C.c_void_p()
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, previous)
        self.checked("pam_close_session", status)
        self.checked("pam_end", end)
