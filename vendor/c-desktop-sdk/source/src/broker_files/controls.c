#include "../sophia_broker_files.h"
#include <string.h>
static void put(unsigned char *b, uint64_t v, size_t n) {
  size_t i;
  for (i = 0; i < n; ++i)
    b[i] = (unsigned char)(v >> (i * 8));
}
int sophia_bf_encode_negotiate(uint64_t epoch, uint64_t submission,
                               uint16_t minimum, uint16_t maximum, void *output,
                               size_t capacity) {
  unsigned char *b = output;
  if (!b || !epoch || !submission || capacity < 40)
    return -1;
  memset(b, 0, 40);
  put(b, 40, 4);
  put(b + 4, 1, 2);
  put(b + 6, 256, 2);
  put(b + 8, epoch, 8);
  put(b + 16, submission, 8);
  put(b + 32, minimum, 2);
  put(b + 34, maximum, 2);
  return 0;
}
int sophia_bf_encode_submit(uint64_t epoch, uint64_t submission,
                            uint32_t length, void *output, size_t capacity) {
  unsigned char *b = output;
  if (!b || !epoch || !submission || length < 40 || length > 448 ||
      capacity < 24)
    return -1;
  memset(b, 0, 24);
  put(b, epoch, 8);
  put(b + 8, submission, 8);
  put(b + 16, length, 4);
  return 0;
}
int sophia_bf_encode_ack(uint64_t epoch, uint64_t sequence, void *output,
                         size_t capacity) {
  unsigned char *b = output;
  if (!b || !epoch || !sequence || capacity < 16)
    return -1;
  put(b, epoch, 8);
  put(b + 8, sequence, 8);
  return 0;
}
