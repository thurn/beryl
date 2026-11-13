---
name: arena-pixel-match
description: Make one thing in Beryl's UI (an animation, a prompt, a layout, an effect) match MTG Arena as nearly pixel for pixel as can be, starting from the arena-video skill's recordings. Films Arena and Beryl whole and side by side, lists every difference in where things are, how large, which way up and when, fixes them largest first, and re-films until none is left or each one left is named. Instructions given with the request override Arena. Use when the user invokes /arena-pixel-match, asks for something to look or move exactly like Arena, or says a part of the UI does not match Arena.
---

# Arena pixel match

The goal is a side-by-side film in which Beryl and Arena agree, frame for
frame, on where every thing is, how large it is, which way it is turned, what
is in front of what and what light plays on it. "Close in spirit" is a failure
here. So is matching the numbers someone wrote down about Arena while the
picture differs.

This skill starts from the `arena-video` skill: read it first. It finds the
moments and makes the films. This one says how to use them when the bar is
identical.

## What overrides Arena

In this order:

1. **The request.** Anything the user said with the request wins over Arena,
   whatever Arena does. Write these down before you look at a frame, and say
   in your report which differences from Arena they caused. A difference the
   request asks for would be a new Beryl exception: ask the user explicitly
   whether to add it before you do, and add it to `data/ui-conventions.json`
   (`just conventions` renders the page) only after a clear yes.
2. **The numbered exceptions** in `docs/ui-conventions.md`. Each was the user's
   word once. Where one stands in the way of a match, keep it, name it in the
   report with what matching would have looked like, and let the user lift it.
   A difference that no numbered exception lists is not one, even if an old
   report or design choice led to it: match Arena, or ask the user before
   listing it.
3. **Arena, to the pixel.** Everything else.

Arena's own art, card frames, fonts, icons and sounds are never copied into
Beryl: match what they do and where they are, in Beryl's skin. Colors, type
and icon drawings stay Beryl's unless the request says otherwise. Everything
else is in scope by default: position, size, path, timing, easing, rotation,
perspective, depth order, shadows, glows, what makes room and when, and where
each zone sits on the table.

## Why this is its own skill

The card draw (2026-11-12) was "fixed" once and was still nothing like
Arena's. The causes are the traps this skill is built around:

- **The question was answered as it was put.** The report guessed at timings,
  so timings were fixed. Nobody listed every difference first. The card's
  route across the table and where it landed in the hand were both wrong and
  were never looked at.
- **The films were crops.** Crops made for the tracker cut off the hand and
  most of the table. A whole Arena table was never put beside a whole Beryl
  table.
- **One number hid three motions.** "A draw takes 1050 ms" was a flight, a hold
  and a drop. Reused on a flight with no hold, it was simply slow.
- **A share measured on one motion was used on another.** "Edge-on at 55% of
  the flight" came from a 400 ms spell cast.
- **The fixture flattered the result.** The filmed draw happened to land where
  Arena's does; in a real game the card landed by its id.
- **The reference prose was wrong in places.** Notes in
  `data/arena-motion.json` said where cards came from without anyone having
  filmed it whole.
- **The checks passed.** `just motion` holds durations and staggers. No check
  knows a route or a destination.

## The loop

### 1. Find it in Arena, three times

Use `arena-video` to find at least three instances, in different recordings
where there are any. Arena varies by seat, by count and by what else is on
the table, and one instance is one instance. If the user named a moment, that
is the first instance, not the only one.

### 2. Film Arena whole before anything else

No crop, the recording's own frame rate, the whole motion with half a second
either side:

```sh
just video film <id> 17:45.8 --span 2 --fps 30 --cols 8 --width 480
```

A sheet stays readable to about 16 frames, so film a long moment in halves, or
at 15 fps first and 30 after. Only then crop, to read one detail (a flip, a
shadow), and never judge a route from a crop. Timing read at 15 fps is good to
67 ms; read it at 30.

### 3. List everything that changes, not only the subject

For each instance write the inventory: every thing on screen that moves,
appears, changes size, changes face or is lit during the moment. The subject,
what makes room for it, the pile it left, banners over it, its shadow, its
glow, anything that waits for it. For each, in fractions of the table's width
and height read off the whole-table frames:

- where it starts, each place it turns or pauses, where it ends, and which
  thing on the table each of those is next to (the library, the stack's left
  edge, the right end of the hand);
- its size at each of those against a card at rest in the hand and against the
  table's height;
- which way up it is and when its face changes, as a time and as a share of
  the way travelled;
- the time of each of those from the first frame of change, and so each phase
  on its own (lift, flight, hold, drop), never one total;
- what is in front of what;
- what the pictures show of depth: a shadow parting from the card, a far edge
  drawn in, growth toward the camera.

`just video measure` gives a path and sizes where it can follow the card. Where
it cannot (several movers), read the frames.

### 4. Film Beryl the same way

The same moment, the whole table, stepped stage time:

- A verb of the stage: `just motion --film <verb> [case]`. Each take's folder
  under `~/Library/Caches/beryl/motion/<checkout>/film/<verb>/` has `strip/`,
  the whole table at 30 frames a second, each frame named by its stage ms, and
  `brief.txt` has the subject's pose at every tick.
- A card's moment: the `specs` skill (`just specs --film`, `--motion`).
- A still (a prompt, a layout): `just ui-shots`, or `just specs --shots`.

Film a second situation as well as the fixture's: another hand size, the other
seat, a card whose id sorts elsewhere, a spell on the stack. A match that
holds in one fixture and not in a game is not a match. Where no fixture has
the situation, add one (`ui/fixtures.json`, `beryl_cli`'s `boards.rs`).

### 5. Pair them at one scale and one clock

Put Arena's frames over Beryl's, the whole table of each at the same width,
frame `n` of one over frame `n` of the other from the first frame of change:

```sh
# Arena: one row of 7 frames, 15 a second (each 480 by 270).
just video film <id> 17:46.1 --span 0.47 --fps 15 --cols 7 --width 480
# Beryl: the same 7 instants from the take's strip/ (every other frame of 30).
magick $(ls $TAKE/strip/*.jpg | awk 'NR>=4 && (NR-4)%2==0' | head -7) \
  -resize 480x +append beryl-row.png
magick <arena sheet> beryl-row.png -append pair.png
```

Start Beryl's row at the frame the motion begins (the `NR>=4` above), as
Arena's row starts at its first frame of change. For one instant, lay one over
the other, which shows a difference in place or size at a glance:

```sh
magick arena-frame.png beryl-frame.png -compose blend \
  -define compose:args=50 -composite over.png
```

Where the moment is an instance in `data/arena-motion.json`, give that instance
a whole-table `command` (no `--crop`) and `just motion --film` makes the
contact sheet itself (`contact/<case>-<k>-<seat>.png`).

Write the images to the cache or your scratch folder. Never commit one.

### 6. Write the difference table

One row for each thing in the inventory at each instant that matters: Arena's
value, Beryl's value, the difference. Then order the rows by how wrong the
picture is, which is nearly always this order:

1. A thing goes somewhere else, or comes from somewhere else, or ends
   somewhere else. A zone sits somewhere else on the table.
2. A thing Arena shows is missing, or Beryl shows one Arena does not.
3. Size, and which way up.
4. The order and length of the phases.
5. Easing within a phase.
6. Depth, shadow and light.

Do not start fixing until the table is whole. If the user gave a guess at the
cause, the table is how you check it: answer the guess, and report the rest.

A zone that sits elsewhere than Arena's (Beryl's stack is further left) is a
row of the table like any other. Moving a zone changes every motion that
touches it: do it when the request is about the table, and otherwise route
relative to where Beryl's zone is and name what that leaves different.

### 7. Fix from the top, and film again after each fix

Take the rows in order. After each fix, film and pair again (steps 4 and 5),
and look at the whole pair, not the row you fixed: a fix to a route changes
what the timing looks like. Stop when every row is inside the tolerance below
or is named in the report with its reason.

Arena is a scene in depth. The stage is flat but can draw depth: a card's
turn-over is drawn in perspective (`stage.css`, `pose.ts`), and growth toward
the camera is a pose's width. A match usually needs both, and a shadow.

### 8. Make it stay matched

- Put the values in the data (`data/ui-conventions.json`, each phase its own
  case where the phases differ), and Arena's whole-table instance beside them
  in `data/arena-motion.json`. Correct any note there that the films
  contradict.
- Add a check for each row you fixed that no check knew: a test that pins the
  route's farthest point, the landing place, the order of a hand. A duration
  check does not hold a route.
- Add the broken situation as a fixture or spec moment (AGENTS.md, Checks).

### 9. Report

Show the user the final pair sheet. Give the difference table as it now
stands: what matches, and each difference left with its numbers and why it is
left (the request, a numbered exception, Beryl's skin, something the stage
cannot draw, something the recordings do not show). Say what you checked on
film and what you did not check in the app.

## Tolerance

Read at the recording's frame rate, on whole-table frames at one scale:

| What                 | Matched when within                         |
| -------------------- | ------------------------------------------- |
| When a phase begins  | one frame of the recording (33 ms at 30)    |
| Where a thing is     | 1% of the table's width                     |
| How large it is      | 3% of its size                              |
| How far it is turned | 3 degrees                                   |
| What is in front     | the same, at every frame                    |
| Which end, which row | the same, always                            |

The recordings are compressed 1080p video at 30 frames a second (a few at
60), so nothing finer can be read from them. Say so rather than claim more.

## Before you call it done

- Did you film the whole table of both, and look at the pair after the last
  change?
- Does the thing start, travel and end where Arena's does, in a second
  situation as well as the first?
- Is every phase timed on its own?
- Did every number you reused come from this motion, in Arena, filmed whole?
- Is each difference left named, with its reason?
- Would a check fail if the route or the destination went back to how it was?

No step here waits for the user, but one: before adding a numbered exception,
ask him explicitly and wait for a clear yes. Everywhere else his eye might be
wanted, proceed on Arena and say so in the report.
