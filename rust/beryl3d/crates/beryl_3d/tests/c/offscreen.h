/*
 * The functions only beryl_3d's tests have (its `tools` feature), beside
 * include/beryl3d.h: an offscreen target in place of a native surface, which a
 * test cannot give, and an end to the app, which finishes its frame log.
 */
#ifndef BERYL3D_OFFSCREEN_H
#define BERYL3D_OFFSCREEN_H

#include "beryl3d.h"

/* Draws into an offscreen target of width by height pixels at one pixel per point. */
void beryl3d_offscreen(Beryl3d *app, uint32_t width, uint32_t height);

/* Ends the app; its frame log is written before this returns. */
void beryl3d_close(Beryl3d *app);

#endif
