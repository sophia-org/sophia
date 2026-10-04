/* Lock provider file records against Sophia's golden records and the
 * contract's rules (spec/sophia-lock-files-v1.kdl). */
#include "sophia_lock_files.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static int hex(char c) {
  if (c >= '0' && c <= '9')
    return c - '0';
  assert(c >= 'a' && c <= 'f');
  return c - 'a' + 10;
}

struct golden {
  char name[64];
  uint8_t bytes[4096];
  size_t count;
};
static struct golden golden[32];
static size_t golden_count;

static void load_golden(void) {
  FILE *file = fopen("spec/golden/sophia-lock-files-v1.records", "r");
  char line[8192], *value, *end;
  size_t i;
  assert(file);
  while (fgets(line, sizeof line, file)) {
    struct golden *row;
    if (line[0] == '#')
      continue;
    value = strchr(line, '|');
    assert(value);
    *value++ = 0;
    end = strchr(value, '\n');
    assert(end);
    *end = 0;
    assert(golden_count < 32);
    row = &golden[golden_count++];
    assert(strlen(line) < sizeof row->name);
    strcpy(row->name, line);
    row->count = strlen(value) / 2;
    assert(row->count && row->count <= sizeof row->bytes &&
           strlen(value) == row->count * 2);
    for (i = 0; i < row->count; ++i)
      row->bytes[i] =
          (uint8_t)(hex(value[i * 2]) * 16 + hex(value[i * 2 + 1]));
  }
  fclose(file);
  assert(golden_count == 22);
}

static const struct golden *find(const char *name) {
  size_t i;
  for (i = 0; i < golden_count; ++i)
    if (!strcmp(golden[i].name, name))
      return &golden[i];
  assert(!"missing golden record");
  return NULL;
}

/* Every golden record decodes and re-encodes to the same bytes. */
static void golden_round_trips(void) {
  size_t i, bytes;
  uint8_t out[4096];
  struct sophia_lf_record record;
  for (i = 0; i < golden_count; ++i) {
    const struct golden *row = &golden[i];
    if (!strcmp(row->name, "submit")) {
      assert(sophia_lf_submit_encode(out, 7, 11, 128) == 0);
      assert(row->count == 24 && !memcmp(out, row->bytes, 24));
      continue;
    }
    if (!strcmp(row->name, "ack")) {
      assert(sophia_lf_ack_encode(out, 7, 13) == 0);
      assert(row->count == 16 && !memcmp(out, row->bytes, 16));
      continue;
    }
    assert(sophia_lf_decode(row->bytes, row->count, &record) == 0);
    assert(record.header.epoch == 7);
    assert(sophia_lf_encode(out, sizeof out, &record, &bytes) == 0);
    assert(bytes == row->count && !memcmp(out, row->bytes, bytes));
  }
}

static void golden_values(void) {
  struct sophia_lf_record r;
  const struct golden *lock = find("lock_locked");
  assert(sophia_lf_decode(lock->bytes, lock->count, &r) == 0);
  assert(r.header.kind == SOPHIA_LF_LOCK && r.value.lock.lock_epoch == 5);
  assert(r.value.lock.phase == SOPHIA_LF_LOCKED);
  assert(r.value.lock.allocation_count == 2);
  assert(r.value.lock.allocations[1].output == 2);
  assert(r.value.lock.allocations[1].allocation == 42);
  assert(r.value.lock.allocations[0].pixel_width == 2560);
  assert(r.value.lock.allocations[0].scale_numerator == 3 &&
         r.value.lock.allocations[0].scale_denominator == 2);
  lock = find("negotiate");
  assert(sophia_lf_decode(lock->bytes, lock->count, &r) == 0);
  assert(r.header.submission == 11 && r.value.negotiate.chord_count == 2);
  assert(r.value.negotiate.chords[0].keysym == 0x62 &&
         r.value.negotiate.chords[0].modifiers == SOPHIA_LF_MOD_ALT);
  assert(r.value.negotiate.capabilities ==
         (SOPHIA_LF_CAP_PRESENT | SOPHIA_LF_CAP_CHORDS));
  lock = find("frame_permit");
  assert(sophia_lf_decode(lock->bytes, lock->count, &r) == 0);
  assert(r.value.frame_permit.pacing_permit == 77 &&
         r.value.frame_permit.expires_after_ms == 100);
  lock = find("entry");
  assert(sophia_lf_decode(lock->bytes, lock->count, &r) == 0);
  assert(r.value.entry.entry == SOPHIA_LF_INSERT && !r.value.entry.empty_after);
}

/* A copy of a golden record with one byte changed must not decode. */
static void refuses(const char *name, size_t at, uint8_t value) {
  const struct golden *row = find(name);
  uint8_t bytes[4096];
  struct sophia_lf_record r;
  memcpy(bytes, row->bytes, row->count);
  assert(at < row->count);
  bytes[at] = value;
  assert(sophia_lf_decode(bytes, row->count, &r) == -1);
}

static void contract_rules(void) {
  struct sophia_lf_record r;
  uint8_t out[4096];
  size_t bytes;
  /* Header identity: an event with a submission, an object with a sequence. */
  refuses("entry", 16, 1);
  refuses("lock_locked", 24, 1);
  refuses("candidate", 16, 0);
  refuses("limits", 4, 2);           /* API version */
  refuses("limits", 6, 99);          /* unknown kind */
  refuses("limits", 32 + 26, 2);     /* pixel format */
  refuses("lock_locked", 32 + 0, 0); /* locked with a zero lock epoch */
  refuses("lock_unlocked", 32, 1);   /* unlocked with a lock epoch */
  refuses("lock_locked", 32 + 16, 4); /* unlocking with allocations */
  refuses("lock_locked", 32 + 20, 1); /* reserved */
  refuses("negotiate", 32 + 16 + 4, 0);  /* chord without a modifier */
  refuses("negotiate", 32 + 16 + 4, 16); /* unknown modifier bit */
  refuses("negotiated", 32 + 8, 2);      /* chords without present */
  refuses("refused", 32, 4);
  refuses("submitted", 32 + 8, 1);        /* 257 is no candidate kind */
  refuses("object_published", 32, 1);     /* the lock object is kind 2 */
  refuses("resource_begin", 32 + 32, 4);  /* slot */
  refuses("resource_begin", 32 + 34, 2);  /* pixel format */
  refuses("resource_status", 32 + 24, 5);
  refuses("candidate_outcome", 32 + 40, 6);
  refuses("frame_permit", 32 + 40, 0);
  refuses("frame_permit", 32 + 40, 251);
  refuses("entry", 32 + 8, 8);
  refuses("entry", 32 + 10, 2);
  refuses("chord", 32 + 8, 8);
  /* A shift-only chord is well formed: the server refuses it. */
  {
    const struct golden *row = find("negotiate");
    uint8_t bytes[4096];
    memcpy(bytes, row->bytes, row->count);
    bytes[32 + 16 + 4] = SOPHIA_LF_MOD_SHIFT;
    assert(sophia_lf_decode(bytes, row->count, &r) == 0);
  }
  /* The encoder refuses what the decoder refuses. */
  memset(&r, 0, sizeof r);
  r.header.kind = SOPHIA_LF_LOCK;
  r.header.epoch = 7;
  r.value.lock.lock_epoch = 0;
  r.value.lock.topology_generation = 1;
  r.value.lock.phase = SOPHIA_LF_LOCKED;
  assert(sophia_lf_encode(out, sizeof out, &r, &bytes) == -1);
  r.value.lock.phase = SOPHIA_LF_UNLOCKED;
  assert(sophia_lf_encode(out, sizeof out, &r, &bytes) == 0 && bytes == 64);
  assert(sophia_lf_encode(out, 40, &r, &bytes) == -4);
  /* Candidates are bounded, and so are the submit controls. */
  assert(sophia_lf_submit_encode(out, 7, 11, 129) == -1);
  assert(sophia_lf_submit_encode(out, 7, 11, 47) == -1);
  assert(sophia_lf_submit_encode(out, 0, 11, 48) == -1);
  assert(sophia_lf_ack_encode(out, 7, 0) == -1);
  assert(sophia_lf_decode(NULL, 0, &r) == -4);
}

int main(void) {
  load_golden();
  golden_round_trips();
  golden_values();
  contract_rules();
  puts("lock_files_test ok");
  return 0;
}
