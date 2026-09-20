# Application launcher

Sophia's native launcher opens with a session shortcut. Your shell searches and
orders the application names; Engine renders the menu and owns its keyboard and
pointer input. The session service executes the selection. The window manager
knows only the operation that opens the menu.

This uses revision 4 of `sophia_shell_v1`. Narthex is the first implementation.
It requires neither Rofi nor Quickshell, and the wire carries no X11 identifiers
or toolkit objects. Applications launched today use Sophia's X11 frontend.

## Choose the applications

Register a catalog in your core configuration, usually
`~/.config/sophia/config.kdl`. Source order is precedence order: a desktop-file
identity in an earlier directory masks the same identity in a later directory,
including when the earlier entry is hidden or malformed.

```kdl
session {
    application-catalog "installed" launch-policy="trusted-host" {
        source "/home/alice/.local/share/applications"
        source "/usr/local/share/applications"
        source "/usr/share/applications"
        application "terminal"
        terminal "terminal" {
            arg "--"
        }
    }
}
```

Use your own absolute home directory. `application` adds a registered Sophia
application by name. `terminal` selects a registered terminal and the explicit
argv prefix used to run a `Terminal=true` entry. For Kitty, `--` separates its
options from the application command. Both names must exist in the session's
application registry; they may come from the installed session's launch options.
The shell never receives those paths or arguments.

Select the catalog and shortcut in your desktop profile, usually
`~/.config/sophia/desktop.kdl`:

```kdl
session {
    application-catalog "installed"
}
shortcut {
    bind "Super+Space" "session:application-launcher" label="Open applications" group="Session"
}
```

Merge these settings into the existing blocks. Keep your existing native shell
selection. A catalog requires a shell that negotiates the launcher capability;
an older shell can continue providing its existing features but cannot open this
menu. Catalog selection and its execution policy take effect at the next login.
There is no implicit scan of home directories and no default catalog that grants
permission to run everything installed.

Type to search, use Up and Down or the wheel to select, and press Enter or click
a row to launch. Backspace edits the query; Ctrl+U clears it. Escape dismisses the
menu. Engine composes text using the session's XKB layout and locale. Text goes
to the shell only while the presented launcher owns input. Modifier shortcuts
remain distinct from plain Enter. Help and the window switcher can dismiss the
launcher; an already active switcher takes precedence over opening it.

The menu claims no work area and changes neither window focus nor layout when it
opens. The launched application's first window goes through normal admission and
WM placement. Closing the menu does not terminate that application.

## Execution policy

`trusted-host` is an explicit policy choice. A launched program has the same host
user authority as the session's other registered applications. The launcher does
not sandbox it, create a private application namespace, or grant filesystem
confinement. A writable desktop-entry source is therefore a source of executable
host commands. Approve only directories whose publishers you trust at that level.

The shell's existing protection domain is unchanged: catalog support adds no
host paths, process execution, display credentials or control socket. Installed
application names are a deliberate metadata disclosure to that shell. Catalog
slots authorize display only. They are not accepted as executable names, WM
operations, scripting commands or reusable launch tokens.

Desktop entries follow the freedesktop format for desktop-file identity, source
precedence, localized names, `Hidden`, `NoDisplay`, `OnlyShowIn`, `NotShowIn`,
`TryExec` and `Exec` argument expansion. Parsing does not evaluate shell syntax.
An explicitly approved entry can itself invoke a shell; its authority is the
host policy, not the menu parser. Unsupported or missing commands are shown as
unavailable. Entries requiring D-Bus activation are unavailable, as are terminal
entries without an explicit adapter. File/URI opening, desktop actions and new
application-confinement policies are deferred. No unsupported policy falls back
to `trusted-host`. When a confinement policy is chosen, the directory it mounts
and the promise it keeps are already written: see *Socket Directories* in
`namespaces-and-portals.md`.

Each opening requests a fresh immutable catalog on a worker thread. A selection
waits in the session's bounded application-admission queue. At dispatch the worker
rescans source precedence, compares the original desktop-entry bytes and command,
and checks executable availability. The session checks the pending grant and
admission again before spawning, without another admission queue in between.
A changed entry is rejected; reopen the menu to review its replacement. This is
not a filesystem sandbox or an executable-content attestation: trusted host code
can still replace files or executables around a launch.

## Wire and presentation

The frame version remains 1. Revision 4 adds capability bits 5
(`application_catalog`) and 6 (`application_launcher`); bit 6 requires bit 5.
The existing revision 1–3 messages and negotiation remain compatible. The
normative field order is in [`sophia-shell-v1.kdl`](../protocol/sophia-shell-v1.kdl),
and byte fixtures are in
[`sophia-shell-launcher.frames`](../protocol/golden/sophia-shell-launcher.frames).
All integers are little endian. Every new message has a nonzero transaction.

| Kind | Direction | Meaning |
| --- | --- | --- |
| 114–116 | Session → shell | Atomic catalog begin, entries, end |
| 117 | Session → shell | Open, committed query, navigation or dismissal |
| 118 | Shell → session | Ordered slots, selection and bounded appearance |
| 119 | Session → shell | Prepared, presented, rejected or superseded candidate |
| 120 | Session → shell | One Engine-issued activation of a presented target |
| 121 | Shell → session | Exact activation tuple echoed with consumed 0 or 1 |
| 122 | Session → shell | Started 1, rejected 2 or failed 3 |

An entry contains a nonzero slot, availability, a label and search keywords.
There are at most 4,096 entries; slots are 1–4,096 and unique. Labels are nonempty
UTF-8 of at most 128 bytes; keywords and queries allow 256 bytes. Text excludes
control characters and bidirectional formatting controls. The transfer is
complete only after its matching end. Other message families may interleave;
receivers assemble the catalog without publishing partial entries.

Operations are Open 0, Query 1, Next 2, Previous 3 and Dismiss 4. Requests carry
connection, catalog, request, output and output-generation identities, the last
presented epoch and the full committed query. Candidates echo their request and
output, advance candidate generation, and contain at most 32 unique catalog
slots. Selected is zero or a member of that set. Font size is 10–32 pixels. Four
ARGB colors describe background, text, selected background and selected text;
the latter three must be opaque. Engine supplies geometry and the catalog's
immutable labels. Unavailable and clipped rows receive no activation target.

Prepared acknowledges validation and submission to the renderer. Presented is
sent only when that exact candidate retires on its output; only then does input
capture become active. A query edit disarms activation immediately. Old results
cannot authorize launch while a new request or candidate is pending. Outcome
values match the existing shell family: prepared 1, presented 2, rejected 3,
superseded 4. The presentation epoch is zero before presentation and nonzero for
a presented outcome.

An activation binds the connection, catalog, request, candidate, presentation,
activation serial and selected slot. Engine creates it from a physical input
completion. The session accepts one exact acknowledgement for its pending grant.
An unsolicited, altered or replayed acknowledgement cannot enqueue a launch.
Started means process creation succeeded; it does not promise a mapped window.

Dismissal, output changes, session interaction revocation, timeout and shell
replacement revoke pending authority. Stale pixels do not retain input rights.
Catalogs have a 16-source, 16,384-directory-entry, eight-level and 16 MiB accepted
source-data budget, with 64 KiB per file. The worker and admission queues are
bounded. Catalog transfer sends at most 32 frames per owner-loop pass. Candidate,
presentation, acknowledgement and active worker waits have five-second budgets.
A timed-out worker remains bounded and its eventual result is discarded.

`tools/check_shell_protocol.sh` checks Rust codecs, independent Nim fixtures and
protected C/Nim socket exchanges, including the maximum catalog and rejection of
unpresented, replayed and obsolete-query activations. Engine tests cover hit
targets, input release accounting and disarming between presentations. Session
tests cover source changes, precedence masking, terminal adapters and nonregular
files. Physical use remains the final daily-driver canary.
