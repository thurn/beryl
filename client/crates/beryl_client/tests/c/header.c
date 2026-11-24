/*
 * The test of include/beryl_client.h, as a C program would use it: it calls every
 * function the header declares, with the header's own types and constants, and
 * checks what each returns. tests/all/ffi.rs runs it and reads the frame log the
 * app wrote to check which inputs arrived.
 */
#include <stddef.h>

#include "beryl_client.h"
#include "offscreen.h"

/* A sixtieth of a second, in nanoseconds. */
#define TICK 16666667ull

/* Whether wake is one the header names, with a rate to ask for. */
static int valid_wake(BerylWake wake) {
    int kind = wake.kind == BERYL_WAKE_NOW || wake.kind == BERYL_WAKE_AT ||
               wake.kind == BERYL_WAKE_NEVER;
    return kind && wake.hz > 0.0f;
}

/* Fields are assigned one by one: C89 compilers, MSVC's among them, warn of an
 * initializer that is not constant. */
static BerylPointer pointer(uint32_t id, uint32_t kind, uint32_t phase, float x) {
    BerylPointer event;
    event.id = id;
    event.kind = kind;
    event.phase = phase;
    event.x = x;
    event.y = 300.0f;
    event.button = BERYL_BUTTON_PRIMARY;
    event.wheel_x = 0.0f;
    event.wheel_y = 0.0f;
    event.modifiers = 0;
    return event;
}

static BerylKey key(uint32_t code, uint32_t phase) {
    BerylKey event;
    event.code = code;
    event.phase = phase;
    event.modifiers = 0;
    return event;
}

/* Returns 0 when every call returned what the header says, else the number of the
 * first check that failed. */
int beryl_client_header_test(const char *data_dir) {
    BerylConfig config;
    BerylRequest request;
    BerylClient *app;
    int requests = 0;
    config.fixture = "verbs-zones";
    config.data_dir = data_dir;
    config.stand_in = NULL;
    config.display_hz = 120.0f;
    config.wake = NULL;
    config.context = NULL;
    app = beryl_client_new(&config);
    if (app == NULL) {
        return 1;
    }
    if (beryl_client_next_request(app, &request)) {
        return 2; /* no surface yet, so nothing to ask */
    }
    beryl_client_offscreen(app, 800, 600);
    beryl_client_pointer(app, pointer(1, BERYL_POINTER_TOUCH, BERYL_PHASE_DOWN, 400.0f));
    beryl_client_pointer(app, pointer(1, BERYL_POINTER_TOUCH, BERYL_PHASE_MOVE, 410.0f));
    beryl_client_pointer(app, pointer(1, BERYL_POINTER_TOUCH, BERYL_PHASE_UP, 410.0f));
    beryl_client_key(app, key(BERYL_KEY_BACK, BERYL_KEY_DOWN));
    beryl_client_key(app, key(BERYL_KEY_BACK, BERYL_KEY_UP));
    beryl_client_text(app, "h\xc3\xa9llo");
    beryl_client_lifecycle(app, BERYL_LOW_MEMORY);
    /* Neither names anything, so both are dropped. */
    beryl_client_pointer(app, pointer(2, 99, BERYL_PHASE_DOWN, 400.0f));
    beryl_client_key(app, key(999, BERYL_KEY_DOWN));
    if (!valid_wake(beryl_client_frame(app, 1000 * TICK))) {
        return 3;
    }
    if (!valid_wake(beryl_client_frame(app, 1001 * TICK))) {
        return 4;
    }
    while (beryl_client_next_request(app, &request)) {
        if (request.kind > BERYL_REQUEST_OPEN_URL || ++requests > 100) {
            return 5;
        }
    }
    beryl_client_surface_lost(app);
    beryl_client_close(app);
    return 0;
}
