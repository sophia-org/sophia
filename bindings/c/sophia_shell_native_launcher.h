#ifndef SOPHIA_SHELL_NATIVE_LAUNCHER_H
#define SOPHIA_SHELL_NATIVE_LAUNCHER_H
#include "sophia_shell_wire.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Structural validation of revision-7 kinds 187..197, including bounded UTF-8
 * and rows. No allocation or I/O; frame/payload are unchanged and remain borrowed.
 * This does NOT authorize a lease, catalog, candidate, event or launch. */
int sophia_shell_native_launcher_validate(const struct sophia_shell_frame *frame);
#ifdef __cplusplus
}
#endif
#endif
