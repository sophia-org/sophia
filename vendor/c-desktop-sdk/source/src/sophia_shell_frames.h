#ifndef SOPHIA_SHELL_FRAMES_H
#define SOPHIA_SHELL_FRAMES_H
/* Explicit SFT r13 codec, spec/sophia-frame-transactions-wire.md. Passive
 * caller-owned values; no allocation or borrowed input survives decoding.
 * Shape validation does not establish negotiated capability, stream credit,
 * source custody, current authority, or native presentation. */
#include "sophia_owned_ring.h"
#include "sophia_shell_files.h"

struct sophia_sft_record {
    struct sophia_sf_header header;
    union sophia_sft_value value;
};
/* Zero on invalid value/revision. No rows are copied by sizing. */
size_t sophia_sft_record_bytes(const struct sophia_sft_record *, uint16_t revision);
/* Return 0, -1 for invalid records, or -4 for arguments/capacity. Encode leaves
 * dst/written unchanged on refusal; decode leaves out unchanged. The granted
 * max_bytes is checked before reading any scene rows, along with the 8192 cap.
 * Input/output storage must not overlap. Neither function grants SFT authority. */
int sophia_sft_encode(void *, size_t, uint16_t revision, const struct sophia_sft_record *,
                      size_t *written);
int sophia_sft_decode(const void *, size_t, enum sophia_sft_class, uint16_t revision,
                      size_t max_bytes, struct sophia_sft_record *);
#endif
