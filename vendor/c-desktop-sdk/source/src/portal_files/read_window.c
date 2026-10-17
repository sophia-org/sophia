#include "../sophia_portal_read_window.h"
#include <string.h>
int sophia_pr_init(struct sophia_pr_window *w, uint64_t length, uint32_t msize,
                   uint32_t iounit, uint32_t chunk) {
  if (!w || !length || length > SOPHIA_PF_IMAGE_MAX_BYTES || msize < 512 ||
      msize > 65536 || !chunk)
    return SOPHIA_PR_INVALID;
  if (chunk > msize - 11)
    chunk = msize - 11;
  if (iounit && chunk > iounit)
    chunk = iounit;
  memset(w, 0, sizeof *w);
  w->length = length;
  w->chunk = chunk;
  return 0;
}
int sophia_pr_reserve(struct sophia_pr_window *w,
                      struct sophia_pr_request *out) {
  size_t i;
  if (!w || !out)
    return SOPHIA_PR_INVALID;
  if (w->error)
    return w->error;
  for (i = 0; i < SOPHIA_PR_WINDOW; ++i) {
    struct sophia_pr_slot *s = &w->slots[i];
    if (s->pending || (!s->count && w->next == w->length))
      continue;
    if (w->serial == UINT64_MAX)
      return w->error = SOPHIA_PR_EXHAUSTED;
    if (!s->count) {
      s->offset = w->next;
      s->count = w->chunk;
      if (s->count > w->length - w->next)
        s->count = (uint32_t)(w->length - w->next);
      w->next += s->count;
    }
    s->token = ++w->serial;
    s->pending = 1;
    out->token = s->token;
    out->offset = s->offset;
    out->count = s->count;
    return 1;
  }
  return 0;
}
int sophia_pr_accept(struct sophia_pr_window *w, uint64_t token, size_t length,
                     uint64_t *offset) {
  size_t i;
  if (!w || !offset)
    return SOPHIA_PR_INVALID;
  if (w->error)
    return w->error;
  for (i = 0; i < SOPHIA_PR_WINDOW; ++i) {
    struct sophia_pr_slot *s = &w->slots[i];
    if (!s->pending || s->token != token)
      continue;
    if (!length || length > s->count)
      return w->error = SOPHIA_PR_INVALID;
    *offset = s->offset;
    s->offset += length;
    s->count -= (uint32_t)length;
    s->pending = 0;
    w->received += length;
    return 0;
  }
  return w->error = SOPHIA_PR_INVALID;
}
int sophia_pr_payload_complete(const struct sophia_pr_window *w) {
  return w && !w->error && w->received == w->length;
}
void sophia_pr_close(struct sophia_pr_window *w) {
  if (w && !w->error)
    w->error = SOPHIA_PR_CLOSED;
}
