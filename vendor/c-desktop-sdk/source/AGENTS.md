# Sophia desktop SDK for C

Read README.md and PROVENANCE.md before changing the library.

- Keep the generic 9P client independent of shell records and IPC framing.
- File record layouts and value rules come from the pinned KDL in spec/.
  Report contract gaps to Sophia; do not infer rules from its implementation.
- Keep public values passive, memory bounded, and ownership explicit.
- Put tests in src/tests/. Keep live production-export harnesses in Sophia.
- Build and test with nice 19 and at most two jobs during coordinated work.
- Sign commits; never reset or restart gpg-agent. Do not add coauthor trailers.
- Preserve license and copyright text.
