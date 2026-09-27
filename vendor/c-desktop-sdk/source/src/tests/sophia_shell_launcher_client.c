/* Independent revision-4 conformance client. The descriptor proof supplies only
 * byte-level framing and socket helpers; no Rust ABI or Sophia library is used. */
#define main descriptor_proof_main
#include "sophia_shell_v1_client.c"
#undef main

int main(void) {
    const char *path = getenv("SOPHIA_SHELL_SOCKET");
    struct frame frame;
    uint8_t hello[12] = {0};
    uint16_t slots[4096], count = 0, expected_count = 0;
    uint64_t epoch, catalog = 0, request = 0, candidate = 0, presentation = 0;
    uint64_t catalog_tx = 0, candidate_tx = 0, last_activation = 0, activated_candidate = 0;
    uint8_t pending_activation[50] = {0};
    uint64_t activation_tx = 0;
    int pending = 0, prepared = 0, catalog_open = 0;
    int fd = path ? connect_when_ready(path) : -1;
    if (fd < 0) return 1;
    write_u16(hello, 4u); write_u16(hello + 2, 4u); write_u64(hello + 4, 97u);
    if (!send_frame(fd, 96u, 0u, hello, sizeof(hello)) || !receive_frame(fd, &frame) ||
        frame.kind != 97u || frame.payload_len != 28u) return 2;
    epoch = read_u64(frame.bytes + FRAME_HEADER_LEN + 4);
    if (!epoch || read_u16(frame.bytes + FRAME_HEADER_LEN) != 4u ||
        (read_u64(frame.bytes + FRAME_HEADER_LEN + 12) & 96u) != 96u) return 3;
    while (receive_frame(fd, &frame)) {
        const uint8_t *b = frame.bytes + FRAME_HEADER_LEN;
        size_t n = frame.payload_len;
        if (!frame.transaction || n < 16u || read_u64(b) != epoch) return 4;
        switch (frame.kind) {
        case 114u:
            if (catalog_open || n != 20u || read_u16(b + 18) != 0u ||
                read_u64(b + 8) <= catalog || read_u16(b + 16) > 4096u) return 5;
            catalog = read_u64(b + 8); expected_count = read_u16(b + 16);
            count = 0; catalog_tx = frame.transaction; catalog_open = 1;
            break;
        case 115u: {
            uint16_t slot, label, keywords;
            size_t at;
            if (!catalog_open || n < 24u || count >= expected_count ||
                frame.transaction != catalog_tx || read_u64(b + 8) != catalog) return 6;
            slot = read_u16(b + 16); label = read_u16(b + 20);
            if (!slot || slot > 4096u || read_u16(b + 18) > 1u || !label || label > 128u || 24u + label > n) return 7;
            for (at = 0; at < count; ++at) if (slots[at] == slot) return 8;
            at = 22u + label; keywords = read_u16(b + at);
            if (keywords > 256u || at + 2u + keywords != n) return 9;
            slots[count++] = slot;
            break;
        }
        case 116u:
            if (!catalog_open || n != 16u || count != expected_count ||
                frame.transaction != catalog_tx || read_u64(b + 8) != catalog) return 10;
            catalog_open = 0; break;
        case 117u: {
            uint8_t result[128] = {0};
            uint16_t rows = count < 32u ? count : 32u;
            if (catalog_open || pending || n < 54u || read_u64(b + 8) != catalog ||
                read_u64(b + 16) <= request || !read_u64(b + 24) || !read_u64(b + 32) ||
                read_u16(b + 48) > 4u || read_u16(b + 50) || read_u16(b + 52) > 256u ||
                54u + read_u16(b + 52) != n) return 11;
            request = read_u64(b + 16); ++candidate; pending = 1; prepared = 0;
            candidate_tx = frame.transaction;
            write_u64(result, epoch); write_u64(result + 8, catalog);
            write_u64(result + 16, request); write_u64(result + 24, candidate);
            write_u64(result + 32, read_u64(b + 24));
            write_u16(result + 40, read_u16(b + 48) != 4u);
            write_u16(result + 42, rows ? slots[0] : 0u);
            write_u16(result + 44, rows); write_u16(result + 46, 14u);
            write_u32(result + 48, 0xf0202020u); write_u32(result + 52, 0xffdddddd);
            write_u32(result + 56, 0xff525f66u); write_u32(result + 60, 0xffffffffu);
            for (uint16_t i = 0; i < rows; ++i) write_u16(result + 64 + 2u * i, slots[i]);
            if (!send_frame(fd, 118u, frame.transaction, result, 64u + 2u * rows)) return 12;
            break;
        }
        case 119u:
            if (!pending || n != 36u || read_u64(b + 8) != request ||
                read_u64(b + 16) != candidate || frame.transaction != candidate_tx ||
                read_u16(b + 34) || read_u16(b + 32) < 1u || read_u16(b + 32) > 4u) return 13;
            if (read_u16(b + 32) == 1u) {
                if (prepared || read_u64(b + 24)) return 14;
                prepared = 1;
            } else {
                if (read_u16(b + 32) == 2u) {
                    if (!prepared || !read_u64(b + 24)) return 15;
                    presentation = read_u64(b + 24);
                }
                pending = 0;
            }
            break;
        case 120u: {
            uint8_t ack[52];
            int valid;
            if (n != 52u || read_u16(b + 50)) return 16;
            memcpy(ack, b, sizeof(ack));
            valid = !pending && count && read_u64(b + 8) == catalog &&
                read_u64(b + 16) == request && read_u64(b + 24) == candidate &&
                presentation && read_u64(b + 32) == presentation &&
                read_u64(b + 40) > last_activation && activated_candidate != candidate &&
                read_u16(b + 48) == slots[0];
            if (valid) { last_activation = read_u64(b + 40); activated_candidate = candidate; }
            memcpy(pending_activation, b, 50u); activation_tx = frame.transaction;
            write_u16(ack + 50, (uint16_t)valid);
            if (!send_frame(fd, 121u, frame.transaction, ack, sizeof(ack))) return 17;
            break;
        }
        case 122u:
            if (n != 52u || frame.transaction != activation_tx ||
                memcmp(b, pending_activation, 50u) || read_u16(b + 50) < 1u || read_u16(b + 50) > 3u) return 18;
            activation_tx = 0; break;
        default: return 19;
        }
    }
    close(fd);
    return 0;
}
