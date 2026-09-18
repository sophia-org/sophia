#ifndef SOPHIA_SHELL_CONTENT_TYPES_H
#define SOPHIA_SHELL_CONTENT_TYPES_H
#include <stdint.h>
/* Passive wire identities shared by content codecs. Nonzero values are not
 * authority; the negotiated connection and current lifecycle must match. */
struct sophia_shell_content_grant { uint64_t connection_epoch, content_grant_epoch; };
struct sophia_shell_content_id { uint64_t id, generation; };
#endif
