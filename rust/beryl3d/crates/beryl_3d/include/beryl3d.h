/*
 * Beryl 3D's C functions, which the phone hosts call (docs/beryl-3d.md, "Hosts").
 * Written by hand to match src/ffi/mod.rs. Every function is called on the platform's
 * main thread, which owns the view and the display's ticks.
 */
#ifndef BERYL3D_H
#define BERYL3D_H

#include <stdint.h>

/* The whole app: the client, the replay or session, and the renderer. */
typedef struct Beryl3d Beryl3d;

typedef struct {
    /* The recording to replay: <data_dir>/<fixture>.json is read. UTF-8. */
    const char *fixture;
    /* The app's writable folder, where frame logs are written. UTF-8. */
    const char *data_dir;
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
 * BERYL_WAKE_AT. */
typedef struct {
    uint32_t kind;
    uint64_t at;
} BerylWake;

enum {
    BERYL_SUSPENDED = 0,
    BERYL_RESUMED = 1,
    BERYL_LOW_MEMORY = 2,
};

/* Starts the app replaying config->fixture. NULL, after printing why, when the
 * recording cannot be read. */
Beryl3d *beryl3d_new(const BerylConfig *config);

/* The host has a surface: on iOS a CAMetalLayer, kept until beryl3d_surface_lost.
 * width and height are physical pixels; scale is pixels per point. */
void beryl3d_surface(Beryl3d *app, void *native_surface, uint32_t width,
                     uint32_t height, float scale, BerylInsets safe_area);

/* The host lost its surface, or may no longer draw in it. */
void beryl3d_surface_lost(Beryl3d *app);

/* BERYL_SUSPENDED, BERYL_RESUMED or BERYL_LOW_MEMORY. */
void beryl3d_lifecycle(Beryl3d *app, uint32_t event);

/* Draws one frame if one is wanted at now_nanoseconds, the monotonic clock's time.
 * Returns when the next is wanted: now, at a time, never. */
BerylWake beryl3d_frame(Beryl3d *app, uint64_t now_nanoseconds);

#endif
