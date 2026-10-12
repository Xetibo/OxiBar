#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include "virtual-pointer.h"

static struct zwlr_virtual_pointer_manager_v1 *manager;
static struct output { struct wl_output *proxy; char *name; struct output *next; } *outputs;
static void geometry(void *d, struct wl_output *o, int32_t x, int32_t y, int32_t w, int32_t h, int32_t s, const char *m, const char *n, int32_t t) {}
static void mode(void *d, struct wl_output *o, uint32_t f, int32_t w, int32_t h, int32_t r) {}
static void done(void *d, struct wl_output *o) {}
static void scale(void *d, struct wl_output *o, int32_t f) {}
static void name(void *d, struct wl_output *o, const char *value) { ((struct output *)d)->name = strdup(value); }
static void description(void *d, struct wl_output *o, const char *value) {}
static const struct wl_output_listener output_listener = {geometry, mode, done, scale, name, description};
static void global(void *data, struct wl_registry *registry, uint32_t id, const char *interface, uint32_t version) {
    if (!strcmp(interface, zwlr_virtual_pointer_manager_v1_interface.name))
        manager = wl_registry_bind(registry, id, &zwlr_virtual_pointer_manager_v1_interface, version < 2 ? version : 2);
    if (!strcmp(interface, wl_output_interface.name)) {
        struct output *output = calloc(1, sizeof(*output));
        output->proxy = wl_registry_bind(registry, id, &wl_output_interface, version < 4 ? version : 4);
        output->next = outputs; outputs = output;
        wl_output_add_listener(output->proxy, &output_listener, output);
    }
}
static void removed(void *d, struct wl_registry *r, uint32_t id) {}
static uint32_t timestamp(void) { struct timespec value; clock_gettime(CLOCK_MONOTONIC, &value); return value.tv_sec * 1000 + value.tv_nsec / 1000000; }
int main(int argc, char **argv) {
    if (argc != 8 && argc != 9) return 2;
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) return 3;
    struct wl_registry *registry = wl_display_get_registry(display);
    const struct wl_registry_listener listener = {global, removed};
    wl_registry_add_listener(registry, &listener, NULL);
    if (wl_display_roundtrip(display) < 0 || !manager || wl_display_roundtrip(display) < 0) return 4;
    struct wl_output *target = NULL;
    for (struct output *output = outputs; output; output = output->next) if (output->name && !strcmp(output->name, argv[3])) target = output->proxy;
    if (!target) return 5;
    struct zwlr_virtual_pointer_v1 *pointer = zwlr_virtual_pointer_manager_v1_create_virtual_pointer_with_output(manager, NULL, target);
    zwlr_virtual_pointer_v1_motion_absolute(pointer, timestamp(), atoi(argv[4]), atoi(argv[5]), atoi(argv[6]), atoi(argv[7]));
    zwlr_virtual_pointer_v1_frame(pointer); wl_display_roundtrip(display); usleep(60000);
    zwlr_virtual_pointer_v1_motion(pointer, timestamp(), wl_fixed_from_int(1), 0);
    zwlr_virtual_pointer_v1_frame(pointer); wl_display_roundtrip(display);
    zwlr_virtual_pointer_v1_motion(pointer, timestamp(), wl_fixed_from_int(-1), 0);
    zwlr_virtual_pointer_v1_frame(pointer); wl_display_roundtrip(display); usleep(60000);
    if (argc != 9 || strcmp(argv[8], "move")) {
        zwlr_virtual_pointer_v1_button(pointer, timestamp(), 272, WL_POINTER_BUTTON_STATE_PRESSED);
        zwlr_virtual_pointer_v1_frame(pointer); wl_display_flush(display); usleep(100000);
        for (int step = 0; step < 20; ++step) {
            zwlr_virtual_pointer_v1_motion(pointer, timestamp(), wl_fixed_from_double(strtod(argv[1], NULL) / 20), wl_fixed_from_double(strtod(argv[2], NULL) / 20));
            zwlr_virtual_pointer_v1_frame(pointer); wl_display_flush(display); usleep(15000);
        }
        if (argc == 9) usleep(strtoul(argv[8], NULL, 10) * 1000);
        zwlr_virtual_pointer_v1_button(pointer, timestamp(), 272, WL_POINTER_BUTTON_STATE_RELEASED);
        zwlr_virtual_pointer_v1_frame(pointer); wl_display_roundtrip(display); usleep(100000);
    } else {
        puts("ready"); fflush(stdout);
        /* Device removal produces a leave event, so hover fixtures retain it. */
        while (getchar() != EOF) {}
    }
    zwlr_virtual_pointer_v1_destroy(pointer); zwlr_virtual_pointer_manager_v1_destroy(manager);
    while (outputs) { struct output *next = outputs->next; wl_output_destroy(outputs->proxy); free(outputs->name); free(outputs); outputs = next; }
    wl_registry_destroy(registry); wl_display_flush(display); wl_display_disconnect(display);
    return 0;
}
