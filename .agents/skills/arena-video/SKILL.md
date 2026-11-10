---
name: arena-video
description: Find out exactly how MTG Arena presents something (a card's cast or trigger, a prompt, a choice, a targeting arrow, an animation's timing) from the gameplay recordings indexed in ~/arena, and compare it with Beryl's UI. Gives a searchable timeline of every spell, ability, prompt and button with timestamps, a screenshot per event, and filmstrips of any moment at any frame rate and crop. Use when matching Beryl to Arena, when asked how Arena shows or animates something, or before designing a prompt, chooser or animation Arena already has.
---

# Arena video reference

Arena's flow is Beryl's reference (AGENTS.md, Workflow). The recordings in
`~/Movies/arena` are hours of real Arena play, indexed into `~/arena`
(`$BERYL_ARENA`), and `just video` makes them answer questions like "how does Arena let you attack a planeswalker?"
for the price of a few images.

You cannot watch video. You read a text index to find the second something
happens, then look at stills tiled into one image. Never step through a
recording frame by frame: find first, then film a few seconds.

## What is there

`just video list` shows each recording's id and whether it is indexed. An
indexed recording has a folder `~/arena/<id>/`:

- `timeline.txt`: one line per event, `time  kind  name | detail [buttons]`.
  Read it with `grep`, or whole (about 600 lines an hour).
- `events.jsonl`: the same with end times and each event's `shot`.
- `shots/`: one full frame per event, named by its time.
- `text.jsonl`: every line of text on screen, twice a second, with its position
  in 1920x1080 pixels. `just video find` searches it.
- `film/`: the filmstrips you make.

`~/arena/cards/` holds the card dumps and their sheets.

To index one, run `just video index <id>`. It takes about four minutes an hour
of video and all the machine's cores, so index one recording at a time, and
never as part of a check. It costs no tokens. Indexing again only derives the
events again unless `--force` is given.

## Starting from a card

To study one card, start from its dump, which is every appearance of it in
every recording as a page of text:

```
just video card "Teferi, Time Raveler" --sheet
```

It prints, and saves to `~/arena/cards/`, the card's type line, each distinct
ability text lettered once, then per recording a numbered line for every cast
and every ability: the time, how long it stayed on the stack, whose turn, the
buttons offered, the shot's path, and what followed within ten seconds
(prompts, other stack objects, life changes). After those come the stretches
it was in hand, shown (a hover preview, a browser, a reveal) or in a draft
pack. `--sheet` tiles the numbered events' shots, twelve to an image, so one
image shows how the card looks on the stack across games. Read the text, pick
the instances worth seeing by number, then film them (below).

`just video cards` shows which of Beryl's own cards the recordings cover and
how well, and `just video cards --top 40` the cards seen most across the
corpus, which is where the recordings have the most to teach.

The kinds of event:

| Kind      | What was on screen                                           |
| --------- | ------------------------------------------------------------ |
| `spell`   | A spell became the top of the stack; detail is its type line |
| `ability` | An ability did; detail is its rules text                     |
| `prompt`  | A heading or one-line prompt, with the buttons offered       |
| `turn`    | The "YOUR TURN" / "OPPONENT'S TURN" banner                   |
| `life`    | A life total changed                                         |
| `match`   | A new opponent                                               |
| `draft`   | The draft began                                              |

## The loop

1. **Say what you are looking for in Arena's words.** A card name, a prompt's
   wording ("Choose attackers."), a button ("Decline"), a type line
   ("Planeswalker"), or a phrase of rules text ("surveil").

2. **Find the moments.**

   ```
   grep -n -i "planeswalker\|choose attackers" ~/arena/<id>/timeline.txt
   just video find <id> "search your library"
   ```

   `find` prints the timeline's matches, then every span during which the text
   was anywhere on screen, which catches what never became an event (a hovered
   card, a button, a counter). Read the timeline lines around a hit: the turn
   banner before it says whose turn it is, and the events after it say what the
   action led to. Search every indexed recording before concluding Arena never
   shows something.

3. **Look once, wide.** Open the event's shot, or film the whole exchange
   slowly to see its beats:

   ```
   just video film <id> 33:28 --span 12 --fps 2 --cols 5 --width 640
   ```

   It prints the image's path and the time of every frame, row by row. Read the
   image, and note the half second where each thing changes.

4. **Look again, close.** Film just that change:

   ```
   just video film <id> 33:30.3 --span 1.5 --fps 6 --cols 3 --width 960
   just video film <id> 33:32.3 --span 0.5 --fps 30 --cols 5 --width 576 \
     --crop 660,90,1260,600
   ```

   Use 6 fps to see states, and the recording's own rate (30, sometimes 60)
   with a crop to see motion: how long a flight takes, where an arrow starts,
   what eases. `--crop x,y,w,h` is in the recording's own pixels. Keep a sheet
   to about 16 frames, or its text stops being readable.

5. **Compare with Beryl.** Reach the same moment in Beryl with the `specs`
   skill (`just specs --film`, `--motion`) or `just ui-shots`, and put the two
   side by side: what is highlighted and when, what the prompt says, which
   buttons exist, where things come from and go to, how long each beat lasts.

6. **Report.** Give Arena's behavior as a numbered sequence, each step with its
   timestamp, so anyone can film it again. Then the differences from Beryl.
   Then what the recording could not show.

## Reading what you see

- **Say only what the frames show.** Stills show a cursor's position, not its
  button: you cannot tell a drag from two clicks. A recording that ends early
  (a concession, a cut) does not show the confirmed state. Name each such gap
  rather than filling it from memory of Arena, and look for a second instance
  in another recording.
- **Timing comes from frame times, not impressions.** Count frames between the
  first and last frame of a motion at the recording's rate.
- **One instance is one instance.** Arena varies by card type, by seat and by
  how many objects are involved. Find two or three before calling something a
  rule, and say how many you saw.
- **The streamer is always the bottom seat.** What the opponent does is seen
  from across the table, which is the view Beryl owes the other player.

## What the index does not know

- Who controls a spell or ability. The turn banner and the buttons ("Opponent's
  Turn", "Resolve") usually settle it; the shot always does.
- Anything that never reaches the stack or a heading: land drops, combat
  damage, which creatures attacked. Find these through the text layer, the life
  events and the prompts around them, then look.
- The right spelling, always. Names come from reading the screen: expect the
  odd "Alll Attack", a life total misread while mana floats beside it, and a
  wrong detail on unusual frames such as Classes. Search with a loose pattern.
- Layouts other than Arena's desktop client at 16:9.

## What to take from Arena

Take its flow: what is asked, in what order, with which defaults and ways to
decline, what is highlighted, how long things take. Beryl's own rules still
win where they differ: everything is a physical object, nothing is shown
twice, every transition is animated (AGENTS.md, UI; `docs/ui-conventions.md`).
Do not copy Arena's art, frames, effects or wording wholesale, and never commit
a recording, a shot or a filmstrip: they stay outside the tree.

## A worked example

"How do you choose between attacking a player and a planeswalker?"

1. `grep -i "planeswalker\|kaito\|choose attackers"` on `SswBO8aOOgc` shows an
   opposing Kaito arriving at 33:07 and the next "Choose attackers." at 33:30.5.
2. `film 33:28 --span 13.5 --fps 2` shows the beats: outlines, a prompt, an
   arrow to the top right, then the game ending.
3. `film 33:30.3 --span 1.5 --fps 6` and the next 1.5 s show the states:
   attackers that can attack get a cyan outline; "No Attacks" and "All Attack"
   appear; a clicked attacker turns gold, and only then do the opponent's life
   total and the planeswalker glow cyan; no arrow is drawn for the default,
   the player.
4. `film 33:32.3 --span 0.5 --fps 30 --crop 660,90,1260,600` shows the motion:
   one arrow with a tail from each selected attacker follows the cursor and
   lands on the planeswalker in about a quarter of a second.
5. Not shown: drag or click, and the confirmed attack, because the opponent
   conceded at 33:32.8. Another recording is needed for those.

## Extending the index

The tool is `tools/src/beryl_tools/video.py` (regions, event rules, commands)
and `video_cards.py` (matching card names across recordings), with the
recognizer in `video_ocr.swift` and tests on synthetic screens in
`tools/tests/test_video.py`. A new kind of event is a `Box` for where its text
appears and a function from frames to events: check the region against
`text.jsonl` first, then run `just video index <id>` to derive again in
seconds. Never tune a region to one recording without checking another.
