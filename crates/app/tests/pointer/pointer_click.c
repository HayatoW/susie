#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include "wlr-virtual-pointer-unstable-v1-client-protocol.h"

#define BTN_LEFT 0x110

struct state {
    struct wl_seat *seat;
    struct zwlr_virtual_pointer_manager_v1 *manager;
};

static void registry_global(void *data, struct wl_registry *registry, uint32_t name,
                            const char *interface, uint32_t version) {
    struct state *state = data;
    if (strcmp(interface, wl_seat_interface.name) == 0) {
        state->seat = wl_registry_bind(registry, name, &wl_seat_interface, 1);
    } else if (strcmp(interface, zwlr_virtual_pointer_manager_v1_interface.name) == 0) {
        uint32_t ver = version < 2 ? version : 2;
        state->manager = wl_registry_bind(
            registry, name, &zwlr_virtual_pointer_manager_v1_interface, ver);
    }
}

static void registry_remove(void *data, struct wl_registry *registry, uint32_t name) {
    (void)data;
    (void)registry;
    (void)name;
}

static const struct wl_registry_listener registry_listener = {registry_global, registry_remove};

static uint32_t now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint32_t)(ts.tv_sec * 1000u + ts.tv_nsec / 1000000u);
}

static void send_button(struct zwlr_virtual_pointer_v1 *pointer, uint32_t button_state) {
    zwlr_virtual_pointer_v1_button(pointer, now_ms(), BTN_LEFT, button_state);
    zwlr_virtual_pointer_v1_frame(pointer);
}

int main(int argc, char **argv) {
    if (argc != 2 || strcmp(argv[1], "hold") != 0) {
        fprintf(stderr, "usage: pointer_click hold\n");
        return 2;
    }
    struct wl_display *display = wl_display_connect(NULL);
    if (!display) {
        fprintf(stderr, "wayland connect failed\n");
        return 1;
    }
    struct state state = {0};
    struct wl_registry *registry = wl_display_get_registry(display);
    wl_registry_add_listener(registry, &registry_listener, &state);
    if (wl_display_roundtrip(display) < 0 || !state.seat || !state.manager) {
        fprintf(stderr, "seat or virtual pointer manager missing\n");
        return 1;
    }
    struct zwlr_virtual_pointer_v1 *pointer =
        zwlr_virtual_pointer_manager_v1_create_virtual_pointer(state.manager, state.seat);
    // 作った直後のボタンは、面へ入る前に捨てられる。1 往復してからクリックを受ける。
    if (wl_display_roundtrip(display) < 0) {
        fprintf(stderr, "pointer enter failed\n");
        return 1;
    }
    printf("ready\n");
    fflush(stdout);

    char line[128];
    while (fgets(line, sizeof line, stdin)) {
        if (strcmp(line, "click\n") != 0) {
            fprintf(stderr, "bad click line\n");
            return 1;
        }
        send_button(pointer, WL_POINTER_BUTTON_STATE_PRESSED);
        if (wl_display_roundtrip(display) < 0) {
            fprintf(stderr, "button press failed\n");
            return 1;
        }
        // 同じミリ秒の押しと離しを、別の入力にする。
        usleep(50000);
        send_button(pointer, WL_POINTER_BUTTON_STATE_RELEASED);
        if (wl_display_roundtrip(display) < 0) {
            fprintf(stderr, "button release failed\n");
            return 1;
        }
        printf("ok\n");
        fflush(stdout);
    }

    zwlr_virtual_pointer_v1_destroy(pointer);
    wl_display_roundtrip(display);
    wl_display_disconnect(display);
    return 0;
}
