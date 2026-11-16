/*
 * Beryl 3D's C functions, which the phone hosts call (docs/beryl-3d.md, "Hosts").
 * Written by hand to match src/ffi in beryl_3d. Every function is called on the
 * platform's main thread, which owns the view and the display's ticks. Only the
 * wake callback in BerylConfig is called from other threads.
 *
 * Every enumerated value is a uint32_t with named constants. A value no constant
 * names is dropped.
 */
#ifndef BERYL3D_H
#define BERYL3D_H

#include <stdbool.h>
#include <stdint.h>

/* The whole app: the client, the replay or session, and the renderer. */
typedef struct Beryl3d Beryl3d;

typedef struct {
    /* The recording to replay: <data_dir>/<fixture>.json is read. UTF-8. NULL
     * plays a live game. */
    const char *fixture;
    /* The app's writable folder, where frame logs, replays and the ledger of
     * games are written. UTF-8. */
    const char *data_dir;
    /* Debug builds only: the name of an agent that answers the person's
     * decisions in a fresh live game, for unattended runs. NULL for a person. */
    const char *stand_in;
    /* The display's highest refresh rate, in frames a second. */
    float display_hz;
    /* Called with context, from any thread, when an input arrives from another
     * thread while the host may have paused its ticks: ask for a frame soon,
     * hopping to the main thread. May be NULL. The app is never freed, so wake
     * and context must stay valid for the process's life. */
    void (*wake)(void *context);
    void *context;
} BerylConfig;

/* The parts of the surface that system bars or a notch cover, in points. */
typedef struct {
    float top;
    float right;
    float bottom;
    float left;
} BerylInsets;

enum {
    BERYL_WAKE_NOW = 0,
    BERYL_WAKE_AT = 1,
    BERYL_WAKE_NEVER = 2,
};

/* When the next frame is wanted; `at` is the monotonic time in nanoseconds for
 * BERYL_WAKE_AT. `hz` is the rate to ask the display for from now on. */
typedef struct {
    uint32_t kind;
    uint64_t at;
    float hz;
} BerylWake;

enum {
    BERYL_SUSPENDED = 0,
    BERYL_RESUMED = 1,
    BERYL_LOW_MEMORY = 2,
};

enum {
    BERYL_POINTER_MOUSE = 0,
    BERYL_POINTER_TOUCH = 1,
    BERYL_POINTER_PEN = 2,
};

enum {
    BERYL_PHASE_DOWN = 0,
    BERYL_PHASE_MOVE = 1,
    BERYL_PHASE_UP = 2,
    BERYL_PHASE_CANCEL = 3,
    BERYL_PHASE_LEAVE = 4,
    BERYL_PHASE_WHEEL = 5,
};

enum {
    BERYL_BUTTON_PRIMARY = 0,
    BERYL_BUTTON_SECONDARY = 1,
    BERYL_BUTTON_MIDDLE = 2,
    BERYL_BUTTON_NONE = 3,
};

/* Modifier bits. */
enum {
    BERYL_SHIFT = 1,
    BERYL_CONTROL = 2,
    BERYL_ALT = 4,
    BERYL_COMMAND = 8,
};

/* A mouse, touch or pen event. id is 0 for the mouse, then one for each finger,
 * and at most 255. The position is in points from the surface's top left. */
typedef struct {
    uint32_t id;
    uint32_t kind;     /* BERYL_POINTER_ */
    uint32_t phase;    /* BERYL_PHASE_ */
    float x;
    float y;
    uint32_t button;   /* BERYL_BUTTON_ */
    float wheel_x;     /* points scrolled, for BERYL_PHASE_WHEEL */
    float wheel_y;
    uint32_t modifiers;
} BerylPointer;

/* The keys Beryl binds, and Back for Android's back button. */
enum {
    BERYL_KEY_A = 0,
    BERYL_KEY_B = 1,
    BERYL_KEY_C = 2,
    BERYL_KEY_D = 3,
    BERYL_KEY_E = 4,
    BERYL_KEY_F = 5,
    BERYL_KEY_G = 6,
    BERYL_KEY_H = 7,
    BERYL_KEY_I = 8,
    BERYL_KEY_J = 9,
    BERYL_KEY_K = 10,
    BERYL_KEY_L = 11,
    BERYL_KEY_M = 12,
    BERYL_KEY_N = 13,
    BERYL_KEY_O = 14,
    BERYL_KEY_P = 15,
    BERYL_KEY_Q = 16,
    BERYL_KEY_R = 17,
    BERYL_KEY_S = 18,
    BERYL_KEY_T = 19,
    BERYL_KEY_U = 20,
    BERYL_KEY_V = 21,
    BERYL_KEY_W = 22,
    BERYL_KEY_X = 23,
    BERYL_KEY_Y = 24,
    BERYL_KEY_Z = 25,
    BERYL_KEY_DIGIT0 = 26,
    BERYL_KEY_DIGIT1 = 27,
    BERYL_KEY_DIGIT2 = 28,
    BERYL_KEY_DIGIT3 = 29,
    BERYL_KEY_DIGIT4 = 30,
    BERYL_KEY_DIGIT5 = 31,
    BERYL_KEY_DIGIT6 = 32,
    BERYL_KEY_DIGIT7 = 33,
    BERYL_KEY_DIGIT8 = 34,
    BERYL_KEY_DIGIT9 = 35,
    BERYL_KEY_ENTER = 36,
    BERYL_KEY_ESCAPE = 37,
    BERYL_KEY_SPACE = 38,
    BERYL_KEY_TAB = 39,
    BERYL_KEY_BACKSPACE = 40,
    BERYL_KEY_DELETE = 41,
    BERYL_KEY_ARROW_UP = 42,
    BERYL_KEY_ARROW_DOWN = 43,
    BERYL_KEY_ARROW_LEFT = 44,
    BERYL_KEY_ARROW_RIGHT = 45,
    BERYL_KEY_BACK = 46,
};

enum {
    BERYL_KEY_DOWN = 0,
    BERYL_KEY_REPEAT = 1,
    BERYL_KEY_UP = 2,
};

typedef struct {
    uint32_t code;     /* BERYL_KEY_A to BERYL_KEY_BACK */
    uint32_t phase;    /* BERYL_KEY_DOWN, REPEAT or UP */
    uint32_t modifiers;
} BerylKey;

enum {
    BERYL_REQUEST_BEGIN_TEXT_ENTRY = 0,
    BERYL_REQUEST_END_TEXT_ENTRY = 1,
    BERYL_REQUEST_SHARE = 2,
    BERYL_REQUEST_HAPTIC = 3,
    BERYL_REQUEST_OPEN_URL = 4,
};

enum {
    BERYL_HAPTIC_LIGHT = 0,
    BERYL_HAPTIC_MEDIUM = 1,
};

/* Something the app asks of the platform. For BEGIN_TEXT_ENTRY, the rectangle is
 * the edited field's, in points, and one field is edited at a time. `text` is the
 * field's contents, the path to share or the URL, else NULL: UTF-8 the app owns,
 * valid until the next call of beryl3d_next_request or beryl3d_frame. */
typedef struct {
    uint32_t kind;     /* BERYL_REQUEST_ */
    uint32_t haptic;   /* BERYL_HAPTIC_, for BERYL_REQUEST_HAPTIC */
    float x;
    float y;
    float width;
    float height;
    const char *text;
} BerylRequest;

/* Starts the app replaying config->fixture, or playing a live game when it is
 * NULL. NULL, after printing why, when the recording cannot be read. */
Beryl3d *beryl3d_new(const BerylConfig *config);

/* The host has a surface: on iOS a CAMetalLayer, kept until beryl3d_surface_lost.
 * width and height are physical pixels; scale is pixels per point. A scale that is
 * not a positive number is taken as 1, and an inset that is negative or not a
 * number as 0. */
void beryl3d_surface(Beryl3d *app, void *native_surface, uint32_t width,
                     uint32_t height, float scale, BerylInsets safe_area);

/* The host lost its surface, or may no longer draw in it. */
void beryl3d_surface_lost(Beryl3d *app);

/* A pointer event. Dropped before the first surface, and when its position or
 * wheel is not a finite number. */
void beryl3d_pointer(Beryl3d *app, BerylPointer event);

/* A key event. Dropped before the first surface. */
void beryl3d_key(Beryl3d *app, BerylKey event);

/* The edited field's whole new contents, UTF-8. Dropped before the first surface. */
void beryl3d_text(Beryl3d *app, const char *utf8);

/* BERYL_SUSPENDED, BERYL_RESUMED or BERYL_LOW_MEMORY. */
void beryl3d_lifecycle(Beryl3d *app, uint32_t event);

/* Draws one frame if one is wanted at now_nanoseconds, the monotonic clock's time.
 * Returns when the next is wanted: now, at a time, never. */
BerylWake beryl3d_frame(Beryl3d *app, uint64_t now_nanoseconds);

/* Writes the next thing the app asks of the platform into *out and returns true,
 * or returns false when there is none. Call after each frame until false. */
bool beryl3d_next_request(Beryl3d *app, BerylRequest *out);

#endif
