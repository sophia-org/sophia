#include "../sophia_shell_wire.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* This is an envelope/negotiation cross-reader, not a content payload decoder. */
static unsigned nibble(char value)
{
    if (value >= '0' && value <= '9') return (unsigned)(value - '0');
    if (value >= 'a' && value <= 'f') return (unsigned)(value - 'a') + 10;
    abort();
}

int main(int argc, char **argv)
{
    assert(argc == 2);
    FILE *file = fopen(argv[1], "r");
    assert(file);
    char *line = malloc(2 * SOPHIA_SHELL_MAX_FRAME_BYTES + 256);
    uint8_t *bytes = malloc(SOPHIA_SHELL_MAX_FRAME_BYTES);
    uint8_t *encoded = malloc(SOPHIA_SHELL_MAX_FRAME_BYTES);
    assert(line && bytes && encoded);
    unsigned count = 0;
    while (fgets(line, 2 * SOPHIA_SHELL_MAX_FRAME_BYTES + 256, file)) {
        if (line[0] == '#' || line[0] == '\n') continue;
        char *tx = strchr(line, '|');
        if (!tx) tx = strchr(line, ' ');
        assert(tx);
        *tx++ = 0;
        char *hex = strchr(tx, '|');
        if (hex) {
            *hex++ = 0;
        } else {
            hex = tx;
            tx = NULL;
        }
        size_t n = strcspn(hex, "\r\n");
        assert(n && n % 2 == 0 && n / 2 <= SOPHIA_SHELL_MAX_FRAME_BYTES);
        for (size_t i = 0; i < n / 2; ++i)
            bytes[i] = (uint8_t)(nibble(hex[2*i]) * 16 + nibble(hex[2*i+1]));
        struct sophia_shell_frame frame;
        assert(sophia_shell_frame_decode(bytes, n / 2, &frame) == SOPHIA_SHELL_OK);
        if (tx) assert(frame.transaction == strtoull(tx, NULL, 10));
        size_t encoded_bytes;
        assert(sophia_shell_frame_encode(encoded, SOPHIA_SHELL_MAX_FRAME_BYTES,
            frame.kind, frame.transaction, frame.payload, frame.payload_bytes, &encoded_bytes) == SOPHIA_SHELL_OK);
        assert(encoded_bytes == n / 2 && !memcmp(encoded, bytes, encoded_bytes));
        if (!strcmp(line, "client_hello")) {
            assert(sophia_shell_hello_encode(encoded, SOPHIA_SHELL_MAX_FRAME_BYTES,
                (struct sophia_shell_hello){1,1,1}, &encoded_bytes) == SOPHIA_SHELL_OK);
            assert(encoded_bytes == n / 2 && !memcmp(encoded, bytes, encoded_bytes));
        }
        if (!strcmp(line, "server_welcome")) {
            struct sophia_shell_welcome welcome;
            assert(sophia_shell_welcome_decode(&frame, (struct sophia_shell_hello){1,1,1}, &welcome) == SOPHIA_SHELL_OK);
            assert(welcome.connection_epoch == 5);
        }
        ++count;
    }
    assert(!ferror(file) && count);
    fclose(file);
    free(line); free(bytes); free(encoded);
    printf("sophia_shell_wire frames=%u envelope_roundtrip=pass payload_validation=negotiation_only\n", count);
    return 0;
}
