#define _POSIX_C_SOURCE 200809L

#include "sophia_wm_v1.h"

#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

#define FRAME_CAPACITY (24u + 65536u)
#define SURFACE_FOCUSABLE (1u << 2)

static uint32_t read_u32(const uint8_t *bytes) {
    return (uint32_t)bytes[0] | ((uint32_t)bytes[1] << 8) |
        ((uint32_t)bytes[2] << 16) | ((uint32_t)bytes[3] << 24);
}

static uint64_t read_u64(const uint8_t *bytes) {
    return (uint64_t)read_u32(bytes) | ((uint64_t)read_u32(bytes + 4) << 32);
}

static int read_exact(int socket_fd, uint8_t *bytes, size_t length) {
    while (length > 0) {
        ssize_t received = read(socket_fd, bytes, length);
        if (received <= 0) return 0;
        bytes += (size_t)received;
        length -= (size_t)received;
    }
    return 1;
}

static int write_exact(int socket_fd, const uint8_t *bytes, size_t length) {
    while (length > 0) {
        ssize_t written = write(socket_fd, bytes, length);
        if (written <= 0) return 0;
        bytes += (size_t)written;
        length -= (size_t)written;
    }
    return 1;
}

static int read_frame(int socket_fd, uint8_t *frame, size_t *frame_len) {
    uint32_t payload_len;
    if (!read_exact(socket_fd, frame, 24u)) return 0;
    payload_len = read_u32(frame + 16u);
    if (payload_len > 65536u) return 0;
    *frame_len = 24u + (size_t)payload_len;
    return read_exact(socket_fd, frame + 24u, payload_len);
}

static int connect_when_ready(const char *path) {
    struct sockaddr_un address;
    struct timespec pause = {0, 10000000};
    int attempt;
    if (strlen(path) >= sizeof(address.sun_path)) return -1;
    memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    memcpy(address.sun_path, path, strlen(path) + 1u);
    for (attempt = 0; attempt < 200; ++attempt) {
        int socket_fd = socket(AF_UNIX, SOCK_STREAM, 0);
        if (socket_fd < 0) return -1;
        if (connect(socket_fd, (struct sockaddr *)&address, sizeof(address)) == 0)
            return socket_fd;
        close(socket_fd);
        if (errno != ENOENT && errno != ECONNREFUSED) return -1;
        nanosleep(&pause, NULL);
    }
    return -1;
}

static int find_output(
    const struct sophia_wm_v1_snapshot_output_record *outputs,
    size_t output_count,
    uint64_t output
) {
    size_t index;
    for (index = 0; index < output_count; ++index)
        if (outputs[index].output == output) return (int)index;
    return -1;
}

static int receive_snapshot(
    int socket_fd,
    uint64_t connection_epoch,
    uint8_t *frame,
    struct sophia_wm_v1_snapshot_output_record *outputs,
    size_t *output_count,
    struct sophia_wm_v1_snapshot_surface_record *surfaces,
    size_t *surface_count,
    uint64_t *scene_generation
) {
    struct sophia_wm_v1_snapshot_begin begin;
    struct sophia_wm_v1_snapshot_end end;
    uint64_t transaction = 0;
    uint64_t end_transaction = 0;
    size_t action_count = 0;
    size_t frame_len = 0;
    uint16_t ordinal;
    if (!read_frame(socket_fd, frame, &frame_len) ||
        sophia_wm_v1_decode_snapshot_begin(frame, frame_len, &transaction, &begin) !=
            SOPHIA_WM_V1_OK)
        return 0;
    if (begin.connection_epoch != connection_epoch || begin.output_count == 0 ||
        begin.output_count > SOPHIA_WM_MAX_OUTPUTS ||
        begin.surface_count > SOPHIA_WM_MAX_SURFACES ||
        begin.action_count > SOPHIA_WM_MAX_BINDINGS)
        return 0;
    *output_count = 0;
    *surface_count = 0;
    for (ordinal = 0; ordinal < begin.chunk_count; ++ordinal) {
        struct sophia_wm_v1_snapshot_chunk chunk;
        uint64_t chunk_transaction = 0;
        size_t index;
        if (!read_frame(socket_fd, frame, &frame_len) ||
            sophia_wm_v1_decode_snapshot_chunk(
                frame, frame_len, &chunk_transaction, &chunk
            ) != SOPHIA_WM_V1_OK)
            return 0;
        if (chunk_transaction != transaction || chunk.connection_epoch != connection_epoch ||
            chunk.ordinal != ordinal || chunk.item_count == 0)
            return 0;
        if (chunk.record_kind == SOPHIA_WM_V1_SNAPSHOT_OUTPUT_RECORD_KIND) {
            if (chunk.item_count > SOPHIA_WM_MAX_OUTPUTS - *output_count ||
                chunk.data_len !=
                    (size_t)chunk.item_count * SOPHIA_WM_V1_SNAPSHOT_OUTPUT_RECORD_SIZE)
                return 0;
            for (index = 0; index < chunk.item_count; ++index)
                if (sophia_wm_v1_decode_snapshot_output_record(
                        chunk.data, chunk.data_len, index, &outputs[*output_count + index]
                    ) != SOPHIA_WM_V1_OK)
                    return 0;
            *output_count += chunk.item_count;
        } else if (chunk.record_kind == SOPHIA_WM_V1_SNAPSHOT_SURFACE_RECORD_KIND) {
            if (chunk.item_count > SOPHIA_WM_MAX_SURFACES - *surface_count ||
                chunk.data_len !=
                    (size_t)chunk.item_count * SOPHIA_WM_V1_SNAPSHOT_SURFACE_RECORD_SIZE)
                return 0;
            for (index = 0; index < chunk.item_count; ++index)
                if (sophia_wm_v1_decode_snapshot_surface_record(
                        chunk.data, chunk.data_len, index, &surfaces[*surface_count + index]
                    ) != SOPHIA_WM_V1_OK)
                    return 0;
            *surface_count += chunk.item_count;
        } else if (chunk.record_kind == SOPHIA_WM_V1_SNAPSHOT_ACTION_RECORD_KIND) {
            struct sophia_wm_v1_snapshot_action_record action;
            if (chunk.item_count > SOPHIA_WM_MAX_BINDINGS - action_count ||
                chunk.data_len !=
                    (size_t)chunk.item_count * SOPHIA_WM_V1_SNAPSHOT_ACTION_RECORD_SIZE)
                return 0;
            for (index = 0; index < chunk.item_count; ++index)
                if (sophia_wm_v1_decode_snapshot_action_record(
                        chunk.data, chunk.data_len, index, &action
                    ) != SOPHIA_WM_V1_OK)
                    return 0;
            action_count += chunk.item_count;
        } else {
            return 0;
        }
    }
    if (!read_frame(socket_fd, frame, &frame_len) ||
        sophia_wm_v1_decode_snapshot_end(
            frame, frame_len, &end_transaction, &end
        ) != SOPHIA_WM_V1_OK)
        return 0;
    if (end_transaction != transaction || end.connection_epoch != connection_epoch ||
        end.scene_generation != begin.scene_generation ||
        end.chunk_count != begin.chunk_count || *output_count != begin.output_count ||
        *surface_count != begin.surface_count || action_count != begin.action_count)
        return 0;
    *scene_generation = begin.scene_generation;
    return 1;
}

static int build_projection(
    const struct sophia_wm_v1_projection_request *request,
    const struct sophia_wm_v1_snapshot_output_record *outputs,
    size_t output_count,
    const struct sophia_wm_v1_snapshot_surface_record *surfaces,
    size_t surface_count,
    struct sophia_wm_v1_projection_output_record *projected_outputs,
    uint8_t *output_bytes,
    struct sophia_wm_v1_projection_placement_record *placements,
    uint8_t *placement_bytes,
    size_t *placement_count
) {
    size_t output_index;
    size_t seen_outputs = 0;
    if (request->affected_output_count == 0 ||
        request->affected_output_count > SOPHIA_WM_MAX_OUTPUTS ||
        request->affected_outputs_len != (size_t)request->affected_output_count * 8u)
        return 0;
    *placement_count = 0;
    for (output_index = 0; output_index < request->affected_output_count; ++output_index) {
        uint64_t output_id = read_u64(request->affected_outputs + output_index * 8u);
        int snapshot_index = find_output(outputs, output_count, output_id);
        size_t surface_index;
        size_t assigned = 0;
        size_t placed = 0;
        if (output_id == 0 || snapshot_index < 0) return 0;
        for (surface_index = 0; surface_index < output_index; ++surface_index)
            if (read_u64(request->affected_outputs + surface_index * 8u) == output_id)
                return 0;
        ++seen_outputs;
        for (surface_index = 0; surface_index < surface_count; ++surface_index)
            if (surfaces[surface_index].current_output == output_id) ++assigned;
        projected_outputs[output_index].output = output_id;
        projected_outputs[output_index].placement_count = (uint32_t)assigned;
        projected_outputs[output_index].focus_index = outputs[snapshot_index].focus_index;
        projected_outputs[output_index].focus_generation =
            outputs[snapshot_index].focus_generation;
        if ((projected_outputs[output_index].focus_index == 0) !=
            (projected_outputs[output_index].focus_generation == 0))
            return 0;
        if (projected_outputs[output_index].focus_generation != 0) {
            int valid_focus = 0;
            for (surface_index = 0; surface_index < surface_count; ++surface_index) {
                const struct sophia_wm_v1_snapshot_surface_record *surface =
                    &surfaces[surface_index];
                if (surface->surface_index == projected_outputs[output_index].focus_index &&
                    surface->surface_generation ==
                        projected_outputs[output_index].focus_generation &&
                    surface->current_output == output_id &&
                    (surface->capability_bits & SURFACE_FOCUSABLE) != 0) {
                    valid_focus = 1;
                    break;
                }
            }
            if (!valid_focus) return 0;
        }
        for (surface_index = 0; surface_index < surface_count; ++surface_index) {
            const struct sophia_wm_v1_snapshot_surface_record *surface = &surfaces[surface_index];
            const struct sophia_wm_v1_snapshot_output_record *output = &outputs[snapshot_index];
            struct sophia_wm_v1_projection_placement_record *placement;
            int32_t column_width;
            if (surface->current_output != output_id) continue;
            placement = &placements[*placement_count];
            column_width = output->width / (int32_t)assigned;
            placement->surface_index = surface->surface_index;
            placement->surface_generation = surface->surface_generation;
            placement->state_generation = surface->state_generation;
            placement->x = output->x + column_width * (int32_t)placed;
            placement->y = output->y;
            placement->width = placed + 1u == assigned
                ? output->x + output->width - placement->x
                : column_width;
            placement->height = output->height;
            placement->requested_width = placement->width;
            placement->requested_height = placement->height;
            if (surface->min_width > 0)
                placement->requested_width = placement->requested_width < surface->min_width
                    ? surface->min_width : placement->requested_width;
            if (surface->min_height > 0)
                placement->requested_height = placement->requested_height < surface->min_height
                    ? surface->min_height : placement->requested_height;
            if (surface->max_width > 0)
                placement->requested_width = placement->requested_width > surface->max_width
                    ? surface->max_width : placement->requested_width;
            if (surface->max_height > 0)
                placement->requested_height = placement->requested_height > surface->max_height
                    ? surface->max_height : placement->requested_height;
            placement->crop_x = 0;
            placement->crop_y = 0;
            placement->crop_width = 0;
            placement->crop_height = 0;
            placement->transform = 1;
            placement->presentation_bits = 0;
            if (projected_outputs[output_index].focus_index == 0 &&
                (surface->capability_bits & SURFACE_FOCUSABLE) != 0) {
                projected_outputs[output_index].focus_index = surface->surface_index;
                projected_outputs[output_index].focus_generation = surface->surface_generation;
            }
            if (sophia_wm_v1_encode_projection_placement_record(
                    placement,
                    placement_bytes +
                        *placement_count * SOPHIA_WM_V1_PROJECTION_PLACEMENT_RECORD_SIZE,
                    SOPHIA_WM_V1_PROJECTION_PLACEMENT_RECORD_SIZE
                ) != SOPHIA_WM_V1_OK)
                return 0;
            ++*placement_count;
            ++placed;
        }
        if (sophia_wm_v1_encode_projection_output_record(
                &projected_outputs[output_index],
                output_bytes + output_index * SOPHIA_WM_V1_PROJECTION_OUTPUT_RECORD_SIZE,
                SOPHIA_WM_V1_PROJECTION_OUTPUT_RECORD_SIZE
            ) != SOPHIA_WM_V1_OK)
            return 0;
    }
    return seen_outputs == request->affected_output_count;
}

static int send_projection(
    int socket_fd,
    uint64_t connection_epoch,
    uint64_t transaction,
    const struct sophia_wm_v1_projection_request *request,
    const uint8_t *output_bytes,
    const uint8_t *placement_bytes,
    size_t placement_count,
    uint8_t *frame
) {
    struct sophia_wm_v1_projection_begin begin;
    struct sophia_wm_v1_projection_chunk chunk;
    struct sophia_wm_v1_projection_end end;
    size_t frame_len = 0;
    uint16_t ordinal = 0;
    begin.connection_epoch = connection_epoch;
    begin.request_id = request->request_id;
    begin.base_generation = request->scene_generation;
    begin.active_output = read_u64(request->affected_outputs);
    begin.chunk_count = placement_count == 0 ? 1u : 2u;
    begin.output_count = request->affected_output_count;
    begin.placement_count = (uint32_t)placement_count;
    begin.indicator_count = 0;
    begin.status_count = 0;
    if (sophia_wm_v1_encode_projection_begin(
            transaction, &begin, frame, FRAME_CAPACITY, &frame_len
        ) != SOPHIA_WM_V1_OK || !write_exact(socket_fd, frame, frame_len))
        return 0;
    chunk.connection_epoch = connection_epoch;
    chunk.ordinal = ordinal++;
    chunk.record_kind = SOPHIA_WM_V1_PROJECTION_OUTPUT_RECORD_KIND;
    chunk.item_count = request->affected_output_count;
    chunk.data = output_bytes;
    chunk.data_len =
        (size_t)request->affected_output_count * SOPHIA_WM_V1_PROJECTION_OUTPUT_RECORD_SIZE;
    if (sophia_wm_v1_encode_projection_chunk(
            transaction, &chunk, frame, FRAME_CAPACITY, &frame_len
        ) != SOPHIA_WM_V1_OK || !write_exact(socket_fd, frame, frame_len))
        return 0;
    if (placement_count != 0) {
        chunk.ordinal = ordinal++;
        chunk.record_kind = SOPHIA_WM_V1_PROJECTION_PLACEMENT_RECORD_KIND;
        chunk.item_count = (uint32_t)placement_count;
        chunk.data = placement_bytes;
        chunk.data_len = placement_count * SOPHIA_WM_V1_PROJECTION_PLACEMENT_RECORD_SIZE;
        if (sophia_wm_v1_encode_projection_chunk(
                transaction, &chunk, frame, FRAME_CAPACITY, &frame_len
            ) != SOPHIA_WM_V1_OK || !write_exact(socket_fd, frame, frame_len))
            return 0;
    }
    end.connection_epoch = connection_epoch;
    end.request_id = request->request_id;
    end.base_generation = request->scene_generation;
    end.chunk_count = ordinal;
    return sophia_wm_v1_encode_projection_end(
            transaction, &end, frame, FRAME_CAPACITY, &frame_len
        ) == SOPHIA_WM_V1_OK && write_exact(socket_fd, frame, frame_len);
}

int main(int argc, char **argv) {
    uint8_t frame[FRAME_CAPACITY];
    uint8_t output_bytes[
        SOPHIA_WM_MAX_OUTPUTS * SOPHIA_WM_V1_PROJECTION_OUTPUT_RECORD_SIZE
    ];
    uint8_t placement_bytes[
        SOPHIA_WM_MAX_SURFACES * SOPHIA_WM_V1_PROJECTION_PLACEMENT_RECORD_SIZE
    ];
    struct sophia_wm_v1_snapshot_output_record outputs[SOPHIA_WM_MAX_OUTPUTS];
    struct sophia_wm_v1_snapshot_surface_record surfaces[SOPHIA_WM_MAX_SURFACES];
    struct sophia_wm_v1_projection_output_record projected_outputs[SOPHIA_WM_MAX_OUTPUTS];
    struct sophia_wm_v1_projection_placement_record placements[SOPHIA_WM_MAX_SURFACES];
    struct sophia_wm_v1_client_hello hello = {
        3, 3,
        SOPHIA_WM_CAPABILITY_BINDINGS | SOPHIA_WM_CAPABILITY_ACTIONS |
            SOPHIA_WM_CAPABILITY_MULTI_OUTPUT
    };
    struct sophia_wm_v1_server_welcome welcome;
    struct sophia_wm_v1_projection_request request;
    struct sophia_wm_v1_projection_outcome outcome;
    uint64_t request_transaction = 0;
    uint64_t outcome_transaction = 0;
    uint64_t scene_generation = 0;
    size_t output_count = 0;
    size_t surface_count = 0;
    size_t placement_count = 0;
    size_t frame_len = 0;
    size_t cycle;
    size_t cycles = 1;
    int socket_fd;
    if (argc != 2 && argc != 3) return 2;
    if (argc == 3) {
        char *end = NULL;
        unsigned long parsed = strtoul(argv[2], &end, 10);
        if (end == argv[2] || *end != '\0' || parsed == 0 || parsed > 16) return 2;
        cycles = (size_t)parsed;
    }
    socket_fd = connect_when_ready(argv[1]);
    if (socket_fd < 0) return 1;
    if (sophia_wm_v1_encode_client_hello(
            &hello, frame, sizeof(frame), &frame_len
        ) != SOPHIA_WM_V1_OK || !write_exact(socket_fd, frame, frame_len))
        return 1;
    if (!read_frame(socket_fd, frame, &frame_len) ||
        sophia_wm_v1_decode_server_welcome(frame, frame_len, &welcome) != SOPHIA_WM_V1_OK ||
        welcome.selected_revision != 3 || welcome.connection_epoch == 0)
        return 1;
    for (cycle = 0; cycle < cycles; ++cycle) {
        uint64_t projection_transaction = (uint64_t)cycle + 1u;
        if (!receive_snapshot(
                socket_fd, welcome.connection_epoch, frame, outputs, &output_count,
                surfaces, &surface_count, &scene_generation
            ))
            return 1;
        if (!read_frame(socket_fd, frame, &frame_len) ||
            sophia_wm_v1_decode_projection_request(
                frame, frame_len, &request_transaction, &request
            ) != SOPHIA_WM_V1_OK || request.connection_epoch != welcome.connection_epoch ||
            request.scene_generation != scene_generation)
            return 1;
        if (!build_projection(
                &request, outputs, output_count, surfaces, surface_count, projected_outputs,
                output_bytes, placements, placement_bytes, &placement_count
            ))
            return 1;
        if (!send_projection(
                socket_fd, welcome.connection_epoch, projection_transaction, &request,
                output_bytes, placement_bytes, placement_count, frame
            ))
            return 1;
        if (!read_frame(socket_fd, frame, &frame_len) ||
            sophia_wm_v1_decode_projection_outcome(
                frame, frame_len, &outcome_transaction, &outcome
            ) != SOPHIA_WM_V1_OK || outcome_transaction != projection_transaction ||
            outcome.connection_epoch != welcome.connection_epoch ||
            outcome.request_id != request.request_id ||
            outcome.outcome < SOPHIA_WM_OUTCOME_COMMITTED ||
            outcome.outcome > SOPHIA_WM_OUTCOME_DISCONNECTED)
            return 1;
    }
    close(socket_fd);
    return 0;
}
