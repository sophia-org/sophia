#ifndef SOPHIA_SHELL_CATALOG_H
#define SOPHIA_SHELL_CATALOG_H

#include "sophia_shell_wire.h"

#ifdef __cplusplus
extern "C" {
#endif

#define SOPHIA_SHELL_CATALOG_MAX_ENTRIES 4096u
#define SOPHIA_SHELL_CATALOG_LABEL_BYTES 128u
#define SOPHIA_SHELL_CATALOG_KEYWORDS_BYTES 256u

struct sophia_shell_catalog_entry {
    uint16_t slot, available;
    uint16_t label_bytes, keywords_bytes;
    char label[SOPHIA_SHELL_CATALOG_LABEL_BYTES + 1u];
    char keywords[SOPHIA_SHELL_CATALOG_KEYWORDS_BYTES + 1u];
};

enum sophia_shell_catalog_result {
    SOPHIA_SHELL_CATALOG_PENDING = 0,
    SOPHIA_SHELL_CATALOG_COMMITTED = 1,
    SOPHIA_SHELL_CATALOG_UNRELATED = 2,
    SOPHIA_SHELL_CATALOG_INVALID = -1,
    SOPHIA_SHELL_CATALOG_ARGUMENT = -2
};

/* Caller-owned single-threaded state. Both equally sized entry arrays must
 * be disjoint from each other and this object, and outlive it. Initialize only
 * after the connection's welcome is validated. No allocation or peer I/O.
 * An invalid catalog latches failed; initialize fresh state on reconnection.
 * Catalog slots are display references, NEVER authority to execute a command. */
struct sophia_shell_catalog {
    struct sophia_shell_catalog_entry *entries[2];
    size_t capacity;
    uint64_t connection_epoch, generation, pending_generation, transaction;
    uint64_t seen[SOPHIA_SHELL_CATALOG_MAX_ENTRIES / 64u];
    uint16_t count, expected, received;
    unsigned active, assembling, failed;
};

int sophia_shell_catalog_init(struct sophia_shell_catalog *catalog,
                             const struct sophia_shell_welcome *welcome,
                             struct sophia_shell_catalog_entry *first,
                             struct sophia_shell_catalog_entry *second,
                             size_t capacity);
/* Accept only kinds 114-116; other families return UNRELATED and do not alter
 * the assembly. Full transaction/epoch/generation/count validation precedes
 * commit. Incomplete or invalid transfers leave the last committed catalog
 * intact. Input payload is copied and may be released after this call. */
int sophia_shell_catalog_accept(struct sophia_shell_catalog *catalog,
                               const struct sophia_shell_frame *frame);
/* Borrowed until the next successful catalog commit or reinitialization.
 * An empty committed catalog has count zero and a nonzero generation. */
const struct sophia_shell_catalog_entry *sophia_shell_catalog_entries(
    const struct sophia_shell_catalog *catalog, size_t *count, uint64_t *generation);

#ifdef __cplusplus
}
#endif
#endif
