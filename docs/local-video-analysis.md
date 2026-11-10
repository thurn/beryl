# Local Magic gameplay video analysis

## Goal and scope

Build an offline annotation tool that turns Arena recordings into a searchable,
timestamped event timeline with evidence and a review queue. Start with land
plays and spell casts; add resolutions, activated abilities, and triggered
abilities once measured accuracy supports them. No paid APIs, cloud inference,
or Arena logs are required. Download OCR models and reference artwork during
setup; analysis then runs locally on Apple silicon.

The initial source is the provided Arena Powered Cube recording (video ID
`SN9QFe2kM4Y`): approximately 68 minutes, 1440p, 60 fps. Metadata and three
sampled frames were inspected; extraction accuracy has not been measured.
Facecam occlusion, stacked lands, and small text are expected limitations. Paper
Magic, arbitrary client layouts, hidden information recovery, and a complete
rules-engine replay are outside the first version.

## Architecture

Implement a Python package under `tools/src/beryl_tools/video/`, exposed through
`tools/src/beryl_tools/cli.py` as a proposed `by video analyze` command. Keep
media handling, recognition, tracking, and event inference separate; Arena
layout rules belong in an adapter. Do not couple recognition to Beryl's
implemented card pool or require engine support for each recognized card.

Use FFmpeg/ffprobe for decoding and media timestamps, OpenCV for crops, image
comparison and tracking, and PaddleOCR as the first local OCR backend to
benchmark. Keep these optional dependencies isolated from ordinary builds. Add
another OCR backend only if measured recognition or Apple silicon installation
problems justify it. A local vision model is a later experiment, not a baseline
dependency.

Pipeline:

1. **Index and segment.** Record source hash, dimensions, time base,
   configuration, and tool/model versions. Separate games from drafting and
   deckbuilding; reset tracks at cuts. Allow manual region calibration and
   facecam masks initially.
2. **Locate changes.** Scan downscaled frames in gameplay regions at an initial
   10 fps. Run recognition on stable frames around changes, with a periodic
   fallback scan. Revisit candidate intervals at source frame rate to locate
   transitions. Tune sampling against missed short events; motion alone cannot
   guarantee recall. Stream frames rather than saving the entire decoded video.
3. **Recognize cards.** OCR name strips, stack labels, life totals, and
   counters. Match imperfect names against a card catalog and aggregate readings
   across frames. For unreadable names, rectify artwork crops and compare local
   image descriptors against reference art. Preserve multiple candidates or
   unknown; never force a nearest match. Decklists narrow candidates without
   excluding generated cards, tokens, alternate faces, or unfamiliar opponent
   cards.
4. **Track observations.** Associate detections using position, appearance,
   zone, and temporal continuity. Give instances separate track IDs even when
   names match. Link hover previews to existing objects rather than counting
   another card. Record occlusion explicitly; disappearance alone is not a zone
   change.
5. **Infer events.** Apply temporal rules to observations: hand-to-stack
   supports a cast; stack-to-battlefield supports resolution. Battlefield
   appearance alone does not establish a cast or land play. Require
   source/label/transition evidence before classifying an ability as activated
   or triggered. Otherwise retain an unresolved stack event. Track provisional
   state and flag contradictions rather than inventing missing actions to repair
   it.
6. **Review and export.** Emit JSONL events plus a local review page served over
   localhost, with video seeking, evidence crops, and editable labels. Save
   human corrections separately so reruns preserve them. Group uncertain events
   for review; allow browsing the full timeline to catch omissions.

## Event and timestamp contract

Each event records `id`, `game_id`, `kind`, `actor`, candidate card identities,
`track_ids`, optional targets/mode, `time_start_ms`, `time_end_ms`, evidence
frame references, confidence scores, and status (`observed`, `inferred`,
`unresolved`, or `reviewed`). Keep recognition confidence separate from event
confidence; raw scores are not calibrated probabilities.

Times are relative to the original recording, derived from decoded presentation
timestamps rather than frame number divided by nominal fps. A cast timestamp is
the first visible evidence of casting, not the click or resolution. Preserve a
bracketing interval when the boundary is uncertain. Record casting and
resolution separately, deduplicate overlapping analysis windows, and flag gaps
across edits. The system cannot recover events permanently obscured or absent
from the video.

Cache observations and artwork under `$BERYL_CACHE/video/<source-hash>/`, keyed
by configuration and model versions. Keep recordings, evidence images, and
generated reports outside Git. Reuse Beryl's card metadata where applicable;
maintain a separate reference catalog for cards outside its supported decks.

## Delivery and evaluation

First manually label five minutes containing ordinary plays, a stack
interaction, and an obscured board. Build extraction, OCR, tracking, and
cast/land events against that sample. Then evaluate on a separate manually
labeled 10–15-minute excerpt without tuning against its labels. Report
precision, recall, identity/actor accuracy, timestamp error, runtime, peak
memory, and correction minutes per video minute. Count unresolved observable
events as misses, and report unobservable intervals separately. An event-free
output must not look successful.

The initial acceptance target is at least 95% precision and recall for
observable land plays and casts, with 95% of matched events within one second of
the labeled visible onset. These are goals, not demonstrated performance. Add
ability recognition only after the baseline passes; report activation and
trigger metrics separately. Prioritize evidence-backed partial results over
unsupported detail.

Tests should cover source-time preservation, cuts, duplicate cards, hover versus
cast, occlusion, and cross-window deduplication using synthetic observations.
Keep real-video evaluation local and opt-in; do not add this recording or
expensive media processing to routine CI.
