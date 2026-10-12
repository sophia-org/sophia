# Developer launch over 9P

Status: t318 implementation candidate. The codec, export, Session dispatch and
CLI are implemented in the worktree; release qualification and installation
remain separate.

This interface starts an independent executable under an explicitly authorized
namespace recipe. It uses the shared 9P2000.L core. It does not extend the legacy
control envelope or authorize an application from its requested recipe name.

## Authority and ownership

Session publishes no endpoint by default. Publication requires both the existing
explicit `host-admin` control policy and at least one confined recipe marked
`developer-launch=#true`. Admission reuses the HostDomain checks: same user,
live kernel peer and matching user, mount and PID namespaces. Same UID or
possession of a socket path alone is insufficient. This is the explicitly
trusted host administrative bootstrap, not unlaunched application authentication
or a new observer grant. Confined callers cannot inherit this authority.

The owner rechecks host admission before dispatch, then checks the requested
recipe against the immutable trusted policy. Shared-X recipes cannot grant
developer launch. No role, principal, environment, namespace ID or service
reference comes from the request. Session assigns custody and uses the same
resource resolver and managed process owner as ordinary application launches.

```text
developer CLI -- 9P request + submit --> HostDomain export
                                             |
                              fresh admission check + claim
                                             |
                           Session checks developer recipe policy
                                             |
                          common recipe resolver / process owner
                                             |
                          Started / Denied / Failed / Unavailable
```

The transport owns a request until Session claims it. Disconnect or timeout
before claim cancels it. After claim, Session owns the launch and its process;
disconnect does not terminate a successfully launched tool. A result deadline
after claim reports Indeterminate, not permission to retry automatically. A
new connection is a new request and can create another process.

## File vocabulary and records

One connection may submit one launch. Its root contains:

| File | Access | Meaning |
| --- | --- | --- |
| `api` | read | Exact `DEVELOPER_LAUNCH_API` text from `sophia-protocol` |
| `request` | write | One complete bounded JSON candidate at offset zero |
| `submit` | write | Four bytes: nonzero little-endian u32 matching the candidate's request ID |
| `result` | read | Immutable final result; open returns EAGAIN until available |

`request` does not execute anything. Its write must contain the whole candidate,
at most 32,768 bytes; fragments, trailing non-whitespace data and unknown or
duplicate fields refuse. The client must negotiate enough msize for that write.
There is one candidate writer, and a candidate cannot change after submission.
An exact submit retry on the same connection has no additional effect. A
different ID cannot replace the submitted request. Tversion resets fids but does
not renew the transport's request budget, authority or deadline.
Keep the candidate writer open until submit succeeds. Clunking it before submit
discards the candidate. A refused candidate write leaves the previous complete
candidate unchanged; it never exposes a partial record.

The request is UTF-8 JSON with exactly these fields:

```json
{"version":1,"request":17,"recipe":"tools","executable":"/usr/bin/example","arguments":["--flag","one argument"]}
```

`request` is a nonzero u32, scoped to this transport. The recipe uses the trusted
configuration's 1–64 character lowercase ASCII/digit/underscore/hyphen grammar.
The executable is an absolute UTF-8 path. At most 128 arguments are allowed;
each string, including the executable, is at most 4,096 bytes and contains no NUL.
Empty arguments are preserved. No shell interprets them. Working-directory and
environment overrides are absent in revision 1; the selected recipe controls
the process environment and host reach.

Result JSON has exactly `version`, `request`, `status` and `process`. Status is
`started`, `denied`, `unavailable`, `failed` or `indeterminate`. Only `started`
has a nonzero u32 process value; every other status has JSON null. The number is
the diagnostic monitor PID, never authentication, application identity or a
claim about pixels. Started means the managed process reached readiness. It does
not mean the application opened a window or authenticated an optional 9P role.
Opening a ready result pins its bytes, size and qid. Flushing a read does not
undo a submitted launch. Wire write replies attest submission, not readiness.

The runtime bounds accepted connections and queued requests to eight each. An
unsubmitted connection expires five seconds after accept; a submitted request
must be claimed within five seconds. Thirty seconds after submission, an
unsettled dispatched request becomes Indeterminate. The connection retires five
seconds later. These are liveness bounds, not startup-latency guarantees. One
worker services peers and rechecks quiet admission every 20 ms, with wakeups for
owner claims and outcomes. The Session owner never parses sockets, inspects a
peer's `/proc` state, or waits for authentication preparation. Worker failure
closes admission; it cannot substitute a host launch.

## Developer command

With `host-admin` and a developer recipe configured, Session logs its private
launch socket at startup. A caller explicitly names that endpoint:

```sh
sophia launch --socket /absolute/session/launch.sock --recipe tools -- /usr/bin/example --flag
```

The command uses the independent Rust 9P pipeline, checks the API identity,
submits once, and prints the correlated result as JSON. Only Started exits
successfully. A missing result after submission is reported as unknown; the
CLI never reconnects or retries automatically. No endpoint is discovered from
a requested role, and this command does not change normal installed-app launch.

See the [recipe contract](namespaces-and-portals.md#trusted-launch-policy-configuration)
and [t318 acceptance](notes/plans/jsschoen-converge-public-roles-on-one-9p-core.md#t318).
