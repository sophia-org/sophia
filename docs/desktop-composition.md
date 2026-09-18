# Choose what starts with your desktop

Your desktop is a collection of parts you choose. You might use one project's
window manager, another project's panel, and a small shell for your window
switcher. Sophia starts those parts and keeps their responsibilities separate.

Put those choices in `~/.config/sophia/desktop.kdl`. If you set
`XDG_CONFIG_HOME`, use its `sophia/desktop.kdl` instead. This is the existing
desktop profile, now with a home that belongs to the whole session.

## Choose the parts

The `session` section selects your window manager, native shell, and login
applications. For an installed Hagia desktop, the choices can look like this:

```kdl
session {
    window-manager "/opt/sophia/current/target/release/hagia"
    shell-client "/opt/sophia/current/target/release/narthex"
    shell-config "/home/alex/.config/narthex/config.kdl"
    terminal "terminal"
    browser "browser"
    startup "terminal" "quickshell-panel"
}
```

This is one section of a complete profile. Keep its `schema 1`, policy,
shortcuts, and other authority sections. Replace the example paths with yours.
For Hagia's replaceable development binary, use the absolute path to
`~/.local/state/sophia/bin/hagia`.

Executable and configuration paths must be absolute. Sophia does not expand
`~`, substitute environment variables, or run a command through a shell.
`window-manager` accepts additional string arguments. Native shell clients use
the existing `--serve` entry point. `shell-config` selects a file the session
makes readable inside the shell's protection domain; the shell interprets it.

The native shell starts in a protection domain without an X11 or Wayland display
connection; selecting a GTK panel as `shell-client` does not provide a toolkit
presentation backend.
An explicitly selected file must exist.

The inherited Narthex setup keeps its usual `~/.config/narthex/config.kdl`.
When you explicitly select a shell, also select its private file if it needs
one. A replacement shell does not inherit Narthex's settings.

You may omit these selections to use the launcher's defaults. An explicit
command-line selection wins over the profile. The profile's WM selection wins
over `config.kdl`'s older `external-wm` setting; both win over a launcher default.

The names in `startup` refer to applications declared in this profile or to
existing Sophia registrations. Define everyday commands here; use `use-core`
when you deliberately want an advanced registration from Sophia's `config.kdl`. Listing an application here grants it no native shell
role. Use `startup` with no arguments to start no applications at login.

## Configure each part where it belongs

The desktop profile answers which parts run and what they may do. Each part
keeps its own settings:

| Change | Where to make it |
| --- | --- |
| Choose a WM, native shell, or login application | Sophia desktop profile, `session` section |
| Enable the native shell or set its reservation allowance | Desktop profile, `shell` section |
| Choose layouts, gaps, and navigation | Your WM's policy configuration |
| Change a panel's widgets or appearance | Your panel's configuration |
| Change shortcut-help startup behavior | Your native shell's configuration |
| Define a command or change what a shortcut launches | Desktop profile, `session.application` or inline `shortcut.bind` |
| Configure advanced application metadata | Sophia `config.kdl`, explicitly referenced with `use-core` |

The profile can carry WM settings in its `policy` section. Sophia passes that
section to the chosen WM for validation. You can keep it in a separate file
and include it using the profile's existing bounded include support. Sophia
does not interpret your WM's layout vocabulary.

Hagia bindings can still request a terminal, browser, or other admitted session
action. The session performs the launch. Choosing a WM does not give it the
ability to start arbitrary host processes or select its own shell permissions.

The `terminal` mapping chooses what the terminal shortcut opens; it does not
require a terminal at login. To start only your panel, keep the mapping and
list only the panel in `startup`:

```kdl
session {
    terminal "terminal"
    startup "quickshell-panel"
}
```

Define commands alongside the bindings that launch them:

```kdl
session {
    application "terminal" { exec "kitty"; }
    application "browser" { exec "brave-origin"; }
    application "work" { use-core "work-browser"; }
    terminal "terminal"
    browser "browser"
}
shortcut {
    profile "desktop"
    bind "Super+Return" "session:spawn-terminal"
    bind "Super+b" { launch "browser"; }
    bind "Super+e" { exec "thunar"; }
    bind "Super+Shift+b" { launch "work"; }
}
```

Arguments are separate quoted strings after `exec`'s executable. They pass
literally to the program; spaces, dollar signs and wildcard characters are not
reinterpreted. A command that needs a shell must name it explicitly, for example
`exec "sh" "-c" "printf hello"`. PATH basenames and absolute executables work.
The WM receives no executable or argument data; Session performs each launch.
Changing a command or binding takes effect on profile reload, together, once
held shortcut keys are released. Invalid edits preserve the prior working
commands and bindings. Existing queued launches retain their original command.

The role names above refer to your declared applications. Ordinary Hagia login does
not require a focused application window. The focused-frame startup deadline
is reserved for the launcher profiles that explicitly exercise applications.
There is no replacement timeout for opening your first application. Empty and
panel-only desktops remain usable indefinitely, and a failed login application
does not prevent later applications from starting. Sophia still enforces the
deadlines of individual operations, such as a WM response or a pending page flip.

## Keep the panel and helper together

Quickshell and Narthex can run in the same desktop. In the current setup,
Quickshell supplies an X11 panel; Narthex supplies native features such as the
switcher and shortcut helper. Keep `shell { enabled #true; }` for the native
shell and list the panel in `session.startup`.

Sophia supports the legacy single-shell mode and explicit independent bar,
application-launcher and dock selections, bounded to three components. There
is no first-client-wins selection, and an X11 panel does not become a native
component by appearing in the startup list. Additional component roles are not
currently admitted by this configuration.

The [native component proposal](notes/concepts/k2d9l42p-native-shell-components-compose-through-explicit-scoped-grants.md)
and [candidate sequence t104–t108](notes/plans/ptil1ejw-modular-native-shell-components-and-independent-launcher-critical-path.md)
record the design history and remaining integration/acceptance criteria. The
[capability map](native-desktop-capabilities.md) records current source support
separately from those historical proposals and physical acceptance.

## Choose a shell and its permissions

The descriptor shell remains supported. Narthex chooses among the features
Sophia knows how to draw: a switcher, tabs, shortcut help, and the application
launcher. Its own configuration controls the choices and appearance settings
those features support. Changing shells does not change the application's
execution policy or give the WM access to shell metadata.

The content model lets an admitted shell submit its own widgets, typography,
and artwork while Sophia controls placement, composition, physical input, and
presentation. Such a shell can provide a custom panel while using the existing
descriptor launcher in an integrated client. Explicit modular selections can
instead assign supported responsibilities to independent native processes.

Choosing that shell, permitting custom content, granting direct GPU execution,
and admitting discrete content actions are separate operator decisions. The
profile uses `content`, `gpu`, and `content-input`; each defaults denied. The
shell cannot grant itself more permission, and the effective profile records
the selected authority. Deterministic source gates cover these paths, while
daily-driver native acceptance remains open.

The added permission concerns what the shell can show you. A content shell
could imitate a prompt or mislabel a button inside its own space, so you would
need to trust its presentation more. That permission would not let it read other
applications' pixels, capture the desktop, run arbitrary commands, or replace
the lock screen. Those operations have separate authorities. Descriptor
restrictions reduce visual freedom without promising that a shell can never
mislead you.

Your Quickshell X11 panel remains an ordinary application under its existing
application policy. The native-shell restrictions do not retroactively
confine it or grant it the native role. See the
[content-shell proposal](content-shell.md) for the developer contract; X11
compatibility and your current shell selection remain the development priority.

## Understand startup and restart

Login applications start once when the session opens. Reloading the WM or
restarting it does not run that list again. Changing component selections or
the startup list takes effect at your next login. A live profile reload reports
non-launch Session changes as deferred while applying commands and bindings
together. A command-only edit does not restart Hagia. A policy edit waits for
the replacement WM to accept its configuration, with rollback on failure.
The running component selections and startup list remain unchanged.

Sophia supervises the native WM and shell through their existing recovery
paths. An application in `startup` uses the ordinary application lifecycle;
it does not acquire automatic restart merely because it is a panel. Keep
services that should outlive the graphical session under your usual service
manager. No particular service manager is required.

## Move an existing profile

An existing `~/.config/hagia/config.kdl` continues to work. When you are ready,
move the complete desktop profile to `~/.config/sophia/desktop.kdl`, preserving
the meaning of any relative include paths. There is no automatic merge of the
two files. Edit the selected file; changes in the other one have no effect.

`config print-effective` can produce a complete profile with includes expanded.
If the destination does not yet exist, this gives you a copy to review while
keeping the old file as a fallback:

```sh
(
    set -eu
    umask 077
    config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
    mkdir -p "$config_home/sophia"
    staged="$(mktemp "$config_home/sophia/.desktop.XXXXXX")"
    trap 'rm -f -- "$staged"' EXIT
    sophia config print-effective \
        --desktop-profile="$config_home/hagia/config.kdl" \
        > "$staged"
    sophia config check --desktop-profile="$staged"
    ln -T -- "$staged" "$config_home/sophia/desktop.kdl"
)
```

Run this with the new Sophia build. It validates the copy and refuses to replace
an existing desktop file. Expanded output keeps settings and their order;
comments inside the original authority sections are not retained.

Discovery checks an explicit `--desktop-profile` first, then the user's Sophia
desktop file, the user's legacy Hagia file, `/etc/sophia/desktop.kdl`, and
`/etc/hagia/config.kdl`. The installed launcher falls back to its packaged
profile. Promotion runs retain their exact packaged profile.

Before your next login, check the profile:

```sh
sophia config check --desktop-profile="$HOME/.config/sophia/desktop.kdl"
sophia config print-effective --desktop-profile="$HOME/.config/sophia/desktop.kdl"
```

The first command validates Sophia's sections and the profile structure. The
second shows the parsed choices and their source. WM settings are validated by
the selected WM before the graphical session is admitted. Shell-specific
settings are validated by the shell that reads them.

## Application menus

Choose a named `application-catalog` in the desktop profile and bind
`session:application-launcher` to open it. The core configuration defines the
catalog sources and execution policy; your native shell supplies search and
selection. This adds no launcher process to WM startup. See
[application launcher](application-launcher.md) for the configuration and trust
boundary.
