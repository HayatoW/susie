#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>
#include "virtual-keyboard-unstable-v1-client-protocol.h"

#define KEY_PRESSED 1
#define KEY_RELEASED 0
#define KEYMAP_XKB_V1 1

struct state {
    struct wl_seat *seat;
    struct zwp_virtual_keyboard_manager_v1 *manager;
};

static void registry_global(void *data, struct wl_registry *registry, uint32_t name,
                            const char *interface, uint32_t version) {
    struct state *state = data;
    if (strcmp(interface, wl_seat_interface.name) == 0) {
        state->seat = wl_registry_bind(registry, name, &wl_seat_interface, 1);
    } else if (strcmp(interface, zwp_virtual_keyboard_manager_v1_interface.name) == 0) {
        uint32_t ver = version < 1 ? version : 1;
        state->manager =
            wl_registry_bind(registry, name, &zwp_virtual_keyboard_manager_v1_interface, ver);
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

static int send_keymap(struct zwp_virtual_keyboard_v1 *keyboard, struct xkb_keymap *keymap) {
    char *text = xkb_keymap_get_as_string(keymap, XKB_KEYMAP_FORMAT_TEXT_V1);
    if (!text) {
        fprintf(stderr, "keymap string failed\n");
        return -1;
    }
    size_t size = strlen(text) + 1;
    int fd = memfd_create("susie-keymap", MFD_CLOEXEC);
    if (fd < 0) {
        fprintf(stderr, "memfd_create failed\n");
        free(text);
        return -1;
    }
    if (ftruncate(fd, (off_t)size) != 0 || write(fd, text, size) != (ssize_t)size) {
        fprintf(stderr, "keymap write failed\n");
        close(fd);
        free(text);
        return -1;
    }
    free(text);
    if (lseek(fd, 0, SEEK_SET) < 0) {
        close(fd);
        return -1;
    }
    zwp_virtual_keyboard_v1_keymap(keyboard, KEYMAP_XKB_V1, fd, (uint32_t)size);
    close(fd);
    zwp_virtual_keyboard_v1_modifiers(keyboard, 0, 0, 0, 0);
    return 0;
}

// xkb のキーコードは evdev に 8 を足した値。プロトコルへ渡すのは evdev の値。
// level 1 は shift の付いた記号。shift が要るときは *shifted を 1 にする。
static int evdev_key(struct xkb_keymap *keymap, char ch, uint32_t *key_out, int *shifted) {
    xkb_keysym_t want = xkb_utf32_to_keysym((uint32_t)(unsigned char)ch);
    if (want == XKB_KEY_NoSymbol) {
        return -1;
    }
    xkb_keycode_t min = xkb_keymap_min_keycode(keymap);
    xkb_keycode_t max = xkb_keymap_max_keycode(keymap);
    for (int level = 0; level <= 1; level++) {
        for (xkb_keycode_t code = min; code <= max; code++) {
            const xkb_keysym_t *syms = NULL;
            int count = xkb_keymap_key_get_syms_by_level(keymap, code, 0, level, &syms);
            for (int i = 0; i < count; i++) {
                if (syms[i] == want) {
                    if (code < 8) {
                        return -1;
                    }
                    *key_out = (uint32_t)code - 8u;
                    *shifted = level == 1;
                    return 0;
                }
            }
        }
    }
    return -1;
}

static int send_key(struct wl_display *display, struct zwp_virtual_keyboard_v1 *keyboard,
                    uint32_t key, uint32_t key_state) {
    zwp_virtual_keyboard_v1_key(keyboard, now_ms(), key, key_state);
    if (wl_display_roundtrip(display) < 0) {
        fprintf(stderr, "key roundtrip failed\n");
        return -1;
    }
    return 0;
}

static int send_mods(struct wl_display *display, struct zwp_virtual_keyboard_v1 *keyboard,
                     uint32_t depressed) {
    zwp_virtual_keyboard_v1_modifiers(keyboard, depressed, 0, 0, 0);
    if (wl_display_roundtrip(display) < 0) {
        fprintf(stderr, "modifier roundtrip failed\n");
        return -1;
    }
    return 0;
}

static int type_char(struct wl_display *display, struct zwp_virtual_keyboard_v1 *keyboard,
                     struct xkb_keymap *keymap, uint32_t shift_mask, char ch) {
    uint32_t key = 0;
    int shifted = 0;
    if (evdev_key(keymap, ch, &key, &shifted) != 0) {
        fprintf(stderr, "no key for %c\n", ch);
        return -1;
    }
    if (shifted && send_mods(display, keyboard, shift_mask) != 0) {
        return -1;
    }
    if (send_key(display, keyboard, key, KEY_PRESSED) != 0) {
        return -1;
    }
    // 押しと離しを、別の入力にする。
    usleep(80000);
    int released = send_key(display, keyboard, key, KEY_RELEASED);
    if (shifted && send_mods(display, keyboard, 0) != 0) {
        return -1;
    }
    return released;
}

int main(int argc, char **argv) {
    if (argc != 2 || strcmp(argv[1], "hold") != 0) {
        fprintf(stderr, "usage: keyboard_type hold\n");
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
        fprintf(stderr, "seat or virtual keyboard manager missing\n");
        return 1;
    }
    struct zwp_virtual_keyboard_v1 *keyboard =
        zwp_virtual_keyboard_manager_v1_create_virtual_keyboard(state.manager, state.seat);
    struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
    struct xkb_rule_names names = {
        .rules = "evdev",
        .model = "pc105",
        .layout = "us",
    };
    struct xkb_keymap *keymap = xkb_keymap_new_from_names(context, &names, XKB_KEYMAP_COMPILE_NO_FLAGS);
    if (!context || !keymap || send_keymap(keyboard, keymap) != 0) {
        fprintf(stderr, "keymap setup failed\n");
        return 1;
    }
    if (wl_display_roundtrip(display) < 0) {
        fprintf(stderr, "keymap roundtrip failed\n");
        return 1;
    }
    xkb_mod_index_t shift = xkb_keymap_mod_get_index(keymap, XKB_MOD_NAME_SHIFT);
    if (shift == XKB_MOD_INVALID) {
        fprintf(stderr, "shift modifier missing\n");
        return 1;
    }
    uint32_t shift_mask = 1u << shift;
    printf("ready\n");
    fflush(stdout);

    char line[128];
    while (fgets(line, sizeof line, stdin)) {
        size_t len = strlen(line);
        if (len == 0 || line[len - 1] != '\n' || len == 1) {
            fprintf(stderr, "bad type line\n");
            return 1;
        }
        line[len - 1] = '\0';
        for (char *cursor = line; *cursor; cursor++) {
            if (*cursor < 0x20 || *cursor > 0x7e) {
                fprintf(stderr, "unsupported character\n");
                return 1;
            }
            if (type_char(display, keyboard, keymap, shift_mask, *cursor) != 0) {
                return 1;
            }
            usleep(80000);
        }
        printf("ok\n");
        fflush(stdout);
    }

    zwp_virtual_keyboard_v1_destroy(keyboard);
    xkb_keymap_unref(keymap);
    xkb_context_unref(context);
    wl_display_roundtrip(display);
    wl_display_disconnect(display);
    return 0;
}
