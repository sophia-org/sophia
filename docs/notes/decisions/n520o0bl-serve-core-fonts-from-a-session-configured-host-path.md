---
id: n520o0bl
date: 2026-09-19
kind: adr
status: accepted
tags: [adr]
---
# Serve core fonts from a session-configured host path

## Context

Sophia's X authority carried one font: a 6x13 Latin-1 bitmap of 256 glyphs
compiled into the binary, with a fixed list of admitted names. The stated
policy was that it "does not consult host font paths or silently accept
arbitrary names".

That policy had a consequence nobody had connected to it. The built-in face is
indexed by one byte, and `QueryFont` reported `min_byte1 = max_byte1 = 0`,
which tells Xlib the font is single-byte. xterm believed it and drew with the
8-bit requests using only the low byte of each character, so under a UTF-8
locale every character above U+00FF was painted as whatever Latin-1 glyph
shared its low byte. Nothing was refused and no error was raised; the terminal
simply rendered the wrong letters. Implementing the 16-bit requests alone would
not have changed that, because the reply is what decides which requests a
client sends.

The repertoire cannot be closed by embedding more glyphs either, except by
embedding a whole Unicode face and then another for bold, and then answering
the same question again for every face a client asks for.

## Decision

Serve core fonts from an ordered font path, session-configured, with the
embedded faces as a final `built-ins` element that is always present. This is
XLibre's model, and the built-in element is the same concession libXfont makes
for `fixed` and `cursor`.

Four safeguards, each tested by what it refuses:

1. The path is session configuration. `SetFontPath` is decoded and answered
   `BadAccess`, so no client can point it anywhere. It is decoded rather than
   left unknown because a client that meets `BadRequest` may exit.
2. A client's string is matched against an index built from each directory's
   own `fonts.dir` and `fonts.alias`; the file opened is the one the directory
   published. No spelling of a font name becomes a path.
3. Reads follow no symbolic links, accept only regular files, are size-bounded
   before parsing, and the PCF reader uses a checked accessor for every field.
4. The face cache is bounded by count and by bytes, and a face whose declared
   matrix exceeds the protocol's two-byte maximum is refused at load.

## Consequences

A session finds the host's core fonts by default, so a terminal in UTF-8 mode
renders the repertoire it asks for. `--font-path=` selects none, which keeps a
proof independent of installed packages, and the built-in element means that
choice still renders text.

Sophia now parses a file format from outside itself, which is new attack
surface where there was none. That is the real cost of this decision, and the
safeguards above are the answer to it; the parser is bounded and total, and its
tests include sweeping every prefix of a valid file.

The authority gains a dependency on a pure-Rust inflate, because the host ships
core fonts gzipped. It links no font library: no FreeType, no fontconfig.

What this does not do: it makes no claim about glyph coverage. The repertoire
is whatever the configured path supplies, and the built-in element remains one
Latin-1 bitmap.

## Alternatives considered

**Embed the Unicode 6x13 as well.** About 54 KB of tables, no new parsing, the
existing policy intact. Rejected because it answers one face and not the
question: bold, wider cells, and any face a client actually names would each
need the same decision again, and a client can name anything.

**Latin-1 only, with 16-bit decoding.** Cheapest. Rejected because it leaves
the box-drawing and accented characters blank, which is the visible complaint.

## Connections

- [docs/sophia-x-authority.md](../../sophia-x-authority.md) carries the
  normative statement of the policy this replaces.
