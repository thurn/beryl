/*
 * The functions only beryl_client's tests have (its `tools` feature), beside
 * include/beryl_client.h: an offscreen target in place of a native surface, which a
 * test cannot give, and an end to the app, which finishes its frame log.
 */
#ifndef BERYL_CLIENT_OFFSCREEN_H
#define BERYL_CLIENT_OFFSCREEN_H

#include "beryl_client.h"

/* Draws into an offscreen target of width by height pixels at one pixel per point. */
void beryl_client_offscreen(BerylClient *app, uint32_t width, uint32_t height);

/* Ends the app; its frame log is written before this returns. */
void beryl_client_close(BerylClient *app);

#endif
