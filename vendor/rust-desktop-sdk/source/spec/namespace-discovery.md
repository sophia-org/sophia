# Namespace endpoint discovery

Version 1 describes one immutable, Session-selected namespace view. It names
endpoints; it does not authenticate a client or authorize any operation. The
recipe compiler, role admission owner and individual grant owner remain separate.

```text
installed application / approved developer launch
                    |
             Session selects policy
                    |
          immutable directory + discovery
                    |
         SDK selects and pins an endpoint
                    |
         protocol-specific authentication
                    |
            role checks each operation
```

## Location and lifetime

`SOPHIA_NAMESPACE_DIR` contains the absolute directory path supplied by Session.
The reserved file `discovery` describes the endpoints relative to that directory.
The environment variable and all discovery fields are locations and observations,
never credentials. There is no fallback from failed authentication to UID-based
or unauthenticated role access.

Session gives a launch its exact generation directory, not a mutable `current`
symlink. A new recipe revision can publish a different directory. Existing
connections retain their captured service identity and admission; changing a
name cannot retarget them. Authority revocation is independent and may invalidate
operations even while the old directory and discovery bytes remain readable.

Aliases have distinct endpoint paths and equal service identity/generation.
Equal paths in different views need not identify the same service. Discovery
does not disclose principal, namespace, grant identifiers or portal payloads.

## Encoding

The entire file is UTF-8, at most 131072 bytes. Every line ends with LF,
including the last. Fields are separated by one TAB; no quoting, escapes, BOM,
comments, blank lines or extra fields are accepted. Parsing consumes the whole
file before returning a view.

```text
sophia-namespace<TAB>1<TAB>revision<TAB>endpoint-count<LF>
endpoint<TAB>path<TAB>protocol<TAB>interface<TAB>version<TAB>service-id<TAB>service-generation<TAB>availability<LF>
```

The first line occurs exactly once. Exactly `endpoint-count` endpoint lines
follow, in recipe order. The count is at most 64 and may be zero. A truncated
file refuses even when it ends on a complete endpoint-line boundary.

All numbers are canonical unsigned decimal: no sign, whitespace or leading
zeroes. Revision, service identity and service generation are nonzero `u64`;
interface version is a nonzero `u32`. The format version is exactly `1`.
Only the endpoint count permits the value `0`.

| Field | Meaning and accepted values |
| --- | --- |
| path | Normalized relative UTF-8 path under this view |
| protocol | Exactly `9p2000.L` or `x11` |
| interface | 1–255 ASCII letters, digits, `_` or `-` |
| version | Interface revision; independent of discovery format and service generation |
| service-id | Opaque owner-selected service identity |
| service-generation | Incarnation of that service |
| availability | Exactly `available`, `waiting` or `recovering`; never a permission verdict |

Paths have no leading/trailing slash, empty component, `.` or `..`, or Unicode
control character. Components contain at most 255 UTF-8 bytes. The normalized
absolute logical path, including its leading slash, contains at most 1024 bytes;
the relative path written here therefore contains at most 1023. `discovery`
and descendants of that reserved name refuse. Duplicate paths and a path that
is the parent of another endpoint refuse. Names compare by exact UTF-8 bytes;
there is no case folding or Unicode normalization.

For example, two names may refer to one read-only service:

```text
sophia-namespace	1	12	2
endpoint	roles/inspection	9p2000.L	inspection	1	31	2	available
endpoint	roles/status	9p2000.L	inspection	1	31	2	available
```

These are endpoint aliases, not automatic grants. A reader must still use the
interface's ordinary authentication and file operations.

## Safe client lookup

An SDK pins the selected directory before reading discovery. Traversal rejects
symlinks and never changes the process working directory. Discovery must be a
regular file; reads remain bounded and reject malformed or incomplete content.
The caller selects a declared path and checks its protocol/interface/version
before using a protocol-specific client.

Logical names can exceed Linux's Unix-socket address limit. On Linux, open each
component relative to the pinned directory without following symlinks, open the
socket with `O_PATH | O_NOFOLLOW | O_CLOEXEC`, verify its socket type, and connect
through `/proc/self/fd/<fd>`. This pins the socket inode, rather than resolving
a mutable path again. Descriptor ownership is explicit and close-on-exec. A
missing or retired socket fails; it is not silently replaced by another path.

The resulting stream enters its existing protocol owner. A 9P endpoint uses
standard 9P2000.L and the approved afid mechanism; discovery invents no protocol
opcode and carries no descriptor. The conventional X display/cookie setup
remains the X authority's responsibility.

## Implementation and qualification

Sophia's protocol codec and runtime compiler/publication foundation implement
this representation. Session launch selection and C/Rust SDK adoption are t318
work in progress. This document does not claim an installed discovery endpoint,
observer grant, capture operation, native 9P application frontend or completed
physical acceptance.
