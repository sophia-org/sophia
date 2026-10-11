/* Independent XCB encoding/decoding against an isolated Sophia socket.
 * No application, display selection, device, or private configuration. */
#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#include <xcb/xcb.h>
#include <xcb/randr.h>

int main(int argc, char **argv)
{
    alarm(8);
    assert(argc == 2);
    struct sockaddr_un address = { .sun_family = AF_UNIX };
    assert(strlen(argv[1]) < sizeof(address.sun_path));
    strcpy(address.sun_path, argv[1]);
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    assert(fd >= 0 && connect(fd, (struct sockaddr *)&address, sizeof(address)) == 0);
    xcb_connection_t *c = xcb_connect_to_fd(fd, NULL);
    assert(c && !xcb_connection_has_error(c));
    const xcb_query_extension_reply_t *ext = xcb_get_extension_data(c, &xcb_randr_id);
    assert(ext && ext->present && ext->first_error != 0);
    xcb_generic_error_t *error = NULL;
    xcb_randr_query_version_reply_t *version = xcb_randr_query_version_reply(
        c, xcb_randr_query_version(c, 1, 5), &error);
    assert(version && !error && version->major_version == 1);
    free(version);
    xcb_screen_t *screen = xcb_setup_roots_iterator(xcb_get_setup(c)).data;
    assert(screen);
    xcb_randr_get_screen_resources_current_reply_t *resources =
        xcb_randr_get_screen_resources_current_reply(c,
            xcb_randr_get_screen_resources_current(c, screen->root), &error);
    assert(resources && !error && resources->num_outputs > 0);
    xcb_randr_output_t *outputs = xcb_randr_get_screen_resources_current_outputs(resources);
    for (int i = 0; i < resources->num_outputs; ++i) {
        xcb_randr_list_output_properties_reply_t *list = xcb_randr_list_output_properties_reply(
            c, xcb_randr_list_output_properties(c, outputs[i]), &error);
        assert(list && !error && list->num_atoms == 1);
        xcb_atom_t atom = xcb_randr_list_output_properties_atoms(list)[0];
        xcb_get_atom_name_reply_t *name = xcb_get_atom_name_reply(c, xcb_get_atom_name(c, atom), &error);
        assert(name && !error && name->name_len == 11);
        assert(memcmp(xcb_get_atom_name_name(name), "non-desktop", 11) == 0);
        free(name);
        xcb_randr_query_output_property_reply_t *meta = xcb_randr_query_output_property_reply(
            c, xcb_randr_query_output_property(c, outputs[i], atom), &error);
        assert(meta && !error && !meta->pending && meta->range && meta->immutable);
        assert(xcb_randr_query_output_property_valid_values_length(meta) == 2);
        int32_t *range = xcb_randr_query_output_property_valid_values(meta);
        assert(range[0] == 0 && range[1] == 1);
        free(meta);
        xcb_randr_get_output_property_reply_t *value = xcb_randr_get_output_property_reply(
            c, xcb_randr_get_output_property(c, outputs[i], atom, XCB_ATOM_INTEGER, 0, 1, 0, 0), &error);
        assert(value && !error && value->format == 32 && value->type == XCB_ATOM_INTEGER);
        assert(value->num_items == 1 && value->bytes_after == 0);
        uint32_t integer;
        memcpy(&integer, xcb_randr_get_output_property_data(value), sizeof(integer));
        assert(integer == 0);
        free(value);
        free(list);
    }
    xcb_randr_list_output_properties_reply_t *bad = xcb_randr_list_output_properties_reply(
        c, xcb_randr_list_output_properties(c, 0), &error);
    assert(!bad && error && error->error_code == ext->first_error + XCB_RANDR_BAD_OUTPUT);
    assert(error->minor_code == 10 && error->major_code == ext->major_opcode && error->resource_id == 0);
    free(error);
    error = NULL;
    xcb_randr_list_output_properties_reply_t *again = xcb_randr_list_output_properties_reply(
        c, xcb_randr_list_output_properties(c, outputs[0]), &error);
    assert(again && !error && again->num_atoms == 1);
    free(again);
    free(resources);
    xcb_disconnect(c);
    puts("randr-properties: independent XCB enumeration/query/read and error recovery PASS");
    return 0;
}
