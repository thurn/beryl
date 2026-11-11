"""`by audio`: the UI's music and sound effects, transcoded from the CC0 packs staged in
$BERYL_CACHE/audio-sources into ui/public/audio, as ui/src/audio/manifest.json selects them.

The outputs are committed, so the app and its CI never need the packs or ffmpeg; this command
makes the selection reproducible and editable. Each manifest entry (a music cue or a sound) has
one or more variants, each a file in a source pack. Variant n of cue `id` becomes
`music/<id>-<n>.m4a` or `sfx/<id>-<n>.wav`, the names the UI's engine (ui/src/audio) asks for.

- Music is AAC in MP4, from ffmpeg's own encoder, whose edit list WebKit's decoder honors at
  the start: no priming delays the music. Decoders may keep the padding to the last AAC frame
  at the end (up to 23 ms of silence), so each loop's exact length goes to LOOPS, which the
  engine loops at rather than at the decoded buffer's end. A constant gain brings each track to MUSIC_LUFS (EBU R128 loudness), held under
  a true peak of PEAK_DB; a constant gain keeps the loop point seamless, as loudnorm's dynamic
  mode would not.
- Sound effects are 16-bit mono WAV, which has no encoder delay to make a cue late: leading
  silence is cut, and the peak brought to PEAK_DB. Their relative levels are the manifest's
  `gain`, applied at playback, so tuning one needs no transcode."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import NotRequired, TypedDict, cast

from . import paths

MANIFEST: Path = paths.UI / "src" / "audio" / "manifest.json"
OUTPUT: Path = paths.UI / "public" / "audio"
# Each looping music cue's variants' lengths in seconds, written with the outputs.
LOOPS: Path = paths.UI / "src" / "audio" / "loops.json"
FFMPEG = "ffmpeg"
MUSIC_LUFS = -18.0
PEAK_DB = -1.0
# Fit the full rotation into the committed audio budget.
MUSIC_BITRATE = "46k"
# Below this level a sound's start is silence, cut so the cue sounds when it is played.
SILENCE_DB = -50
# The committed audio's budget: the repository is public and every clone carries it.
LIMIT_BYTES: int = 12 * 1024 * 1024


class Source(TypedDict):
    title: str
    author: str
    license: str
    url: str
    # The pack's folder under the staged sources.
    dir: str


class Variant(TypedDict):
    source: str
    file: str


class Cue(TypedDict):
    variants: list[Variant]
    loop: NotRequired[bool]
    gain: NotRequired[float]
    voices: NotRequired[int]


class Manifest(TypedDict):
    sources: dict[str, Source]
    music: dict[str, Cue]
    sounds: dict[str, Cue]


def load(path: Path = MANIFEST) -> Manifest:
    return cast(Manifest, json.loads(path.read_text()))


def outputs(manifest: Manifest) -> dict[str, tuple[Source, Variant]]:
    """Each output file, relative to OUTPUT, with the source file it is made from."""
    files: dict[str, tuple[Source, Variant]] = {}
    for folder, ext, cues in (
        ("music", "m4a", manifest["music"]),
        ("sfx", "wav", manifest["sounds"]),
    ):
        for cue, entry in cues.items():
            for n, variant in enumerate(entry["variants"]):
                source = manifest["sources"][variant["source"]]
                files[f"{folder}/{cue}-{n}.{ext}"] = (source, variant)
    return files


def stray(out: Path, expected: set[str]) -> list[Path]:
    """Files under `out` that no manifest entry makes."""
    if not out.exists():
        return []
    return sorted(
        p
        for p in out.rglob("*")
        if p.is_file() and p.relative_to(out).as_posix() not in expected
    )


def looping(manifest: Manifest) -> dict[str, int]:
    """Each looping music cue with its number of variants."""
    return {
        cue: len(entry["variants"])
        for cue, entry in manifest["music"].items()
        if entry.get("loop", False)
    }


def problems(manifest: Manifest, out: Path = OUTPUT, loops: Path = LOOPS) -> list[str]:
    """What is wrong with the committed audio: a missing or stray file, a loop length
    missing, or the budget exceeded. Needs neither ffmpeg nor the packs."""
    files = outputs(manifest)
    found = [f"missing {name}" for name in files if not (out / name).is_file()]
    lengths = cast(
        dict[str, list[float]], json.loads(loops.read_text()) if loops.exists() else {}
    )
    if {cue: len(v) for cue, v in lengths.items()} != looping(manifest):
        found.append(f"{loops.name} does not match the looping music; run `just audio`")
    found += [f"stray {p.relative_to(out).as_posix()}" for p in stray(out, set(files))]
    total = sum(p.stat().st_size for p in out.rglob("*") if p.is_file())
    if total > LIMIT_BYTES:
        found.append(f"{total} bytes, over the {LIMIT_BYTES}-byte budget")
    return found


def measure(source: Path, filters: str) -> str:
    """ffmpeg's report on `source` through `filters`, which print to stderr."""
    run = subprocess.run(
        [FFMPEG, "-hide_banner", "-nostats", "-i", str(source), "-af", filters]
        + ["-f", "null", "-"],
        capture_output=True,
        text=True,
        check=True,
    )
    return run.stderr


def number(report: str, pattern: str) -> float:
    match = re.findall(pattern, report)
    if not match:
        raise ValueError(f"ffmpeg printed no {pattern!r}")
    return float(match[-1])


def seconds(source: Path) -> float:
    """`source`'s exact length: its sample count over its rate, as a WAV's header gives them."""
    run = subprocess.run(
        ["ffprobe", "-v", "error", "-select_streams", "a:0"]
        + [
            "-show_entries",
            "stream=duration_ts,sample_rate",
            "-of",
            "json",
            str(source),
        ],
        capture_output=True,
        text=True,
        check=True,
    )
    stream = cast(dict[str, list[dict[str, str | int]]], json.loads(run.stdout))[
        "streams"
    ][0]
    return int(stream["duration_ts"]) / int(stream["sample_rate"])


def music_gain(source: Path) -> float:
    """The gain in dB that brings `source` to MUSIC_LUFS without its true peak passing
    PEAK_DB."""
    report = measure(source, "ebur128=peak=true")
    loudness = number(report, r"I:\s+(-?[\d.]+) LUFS")
    peak = number(report, r"Peak:\s+(-?[\d.]+) dBFS")
    return min(MUSIC_LUFS - loudness, PEAK_DB - peak)


TRIM = f"silenceremove=start_periods=1:start_threshold={SILENCE_DB}dB"


def sound_gain(source: Path) -> float:
    """The gain in dB that brings the trimmed `source`'s peak to PEAK_DB."""
    peak = number(
        measure(source, f"{TRIM},volumedetect"), r"max_volume:\s+(-?[\d.]+) dB"
    )
    return PEAK_DB - peak


def encode(source: Path, dest: Path, filters: str, codec: list[str]) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [FFMPEG, "-hide_banner", "-loglevel", "error", "-y", "-i", str(source)]
        + ["-af", filters, "-map_metadata", "-1", *codec, str(dest)],
        check=True,
    )


def transcode(name: str, source: Path, dest: Path) -> None:
    if name.startswith("music/"):
        encode(
            source,
            dest,
            f"volume={music_gain(source):.2f}dB",
            ["-c:a", "aac", "-b:a", MUSIC_BITRATE, "-movflags", "+faststart"],
        )
    else:
        encode(
            source,
            dest,
            f"{TRIM},volume={sound_gain(source):.2f}dB",
            ["-ac", "1", "-ar", "44100", "-c:a", "pcm_s16le", "-bitexact"],
        )


def audio(args: argparse.Namespace) -> int:
    manifest = load(Path(args.manifest))
    out = Path(args.output)
    if args.check:
        found = problems(manifest, out, Path(args.loops))
        for problem in found:
            print(f"audio: {problem}", file=sys.stderr)
        return 1 if found else 0
    sources = Path(args.sources)
    files = outputs(manifest)
    for name, (source, variant) in files.items():
        path = sources / source["dir"] / variant["file"]
        if not path.is_file():
            print(f"audio: no {path}", file=sys.stderr)
            return 1
        transcode(name, path, out / name)
        print(f"audio: {name} <- {source['title']}: {variant['file']}")
    loops = Path(args.loops)
    sources_of = manifest["sources"]
    lengths = {
        cue: [
            seconds(sources / sources_of[v["source"]]["dir"] / v["file"])
            for v in manifest["music"][cue]["variants"]
        ]
        for cue in looping(manifest)
    }
    loops.write_text(json.dumps(lengths, indent=2) + "\n")
    for path in stray(out, set(files)):
        path.unlink()
        print(f"audio: removed {path.relative_to(out)}")
    found = problems(manifest, out, loops)
    for problem in found:
        print(f"audio: {problem}", file=sys.stderr)
    total = sum(p.stat().st_size for p in out.rglob("*") if p.is_file())
    print(f"audio: {len(files)} files, {total / 1e6:.1f} MB -> {out}")
    return 1 if found else 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "audio",
        help="transcode the manifest's music and sounds into ui/public/audio",
    )
    p.add_argument("--manifest", default=str(MANIFEST))
    p.add_argument("--output", default=str(OUTPUT))
    p.add_argument("--loops", default=str(LOOPS))
    p.add_argument(
        "--sources",
        default=str(paths.cache_root() / "audio-sources"),
        help="the staged packs (default $BERYL_CACHE/audio-sources)",
    )
    p.add_argument(
        "--check",
        action="store_true",
        help="only check the committed files against the manifest and the budget",
    )
    p.set_defaults(func=audio)
