import json
import math
import random
import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

import pytest

from beryl_tools import cli
from beryl_tools.video_measure import Clip, Unmeasurable, measure

WIDTH, HEIGHT = 400, 225
FPS = 30
FRAME: float = 1 / FPS
# A card at rest: big enough that 2% of its size is a pixel.
CARD_W, CARD_H = 50, 70


@dataclass(frozen=True)
class Flight:
    """A card flying from one center to another, easing in and out, its size swelling by
    `swell` at the middle of the flight on top of growing from `scale_from` to 1."""

    start: float
    duration: float
    origin: tuple[float, float]
    target: tuple[float, float]
    scale_from: float = 1.0
    swell: float = 0.0

    def progress(self, t: float) -> float:
        p = min(1.0, max(0.0, (t - self.start) / self.duration))
        return p * p * (3 - 2 * p)

    def at(self, t: float) -> tuple[float, float, float]:
        p = self.progress(t)
        x = self.origin[0] + (self.target[0] - self.origin[0]) * p
        y = self.origin[1] + (self.target[1] - self.origin[1]) * p
        scale = self.scale_from + (1 - self.scale_from) * p
        return x, y, scale + self.swell * math.sin(math.pi * p)


def render(flights: list[Flight], length: float, seed: int = 7) -> Clip:
    """Frames of the flights over a noisy table, with a little noise of their own as a
    recording's compression adds; every card wears the same art."""
    rng = random.Random(seed)
    table = [rng.randint(30, 100) for _ in range(WIDTH * HEIGHT)]
    art = [rng.randint(150, 250) for _ in range(CARD_W * CARD_H)]
    times: list[float] = [n * FRAME for n in range(round(length * FPS) + 1)]
    frames: list[bytes] = []
    for t in times:
        pixels = [max(0, min(255, v + rng.randint(-3, 3))) for v in table]
        # Later flights are dealt from under earlier ones, so paint them first.
        for flight in reversed(flights):
            cx, cy, scale = flight.at(t)
            w, h = round(CARD_W * scale), round(CARD_H * scale)
            left, top = round(cx - w / 2), round(cy - h / 2)
            for y in range(max(0, top), min(HEIGHT, top + h)):
                row = art[(y - top) * CARD_H // h * CARD_W :]
                for x in range(max(0, left), min(WIDTH, left + w)):
                    pixels[y * WIDTH + x] = row[(x - left) * CARD_W // w]
        frames.append(bytes(pixels))
    return Clip(WIDTH, HEIGHT, times, frames, (WIDTH, HEIGHT))


CAST = Flight(0.5, 0.6, (60, 170), (330, 90), scale_from=0.8, swell=0.5)


def true_peak(flight: Flight, times: list[float]) -> float:
    return max(flight.at(t)[2] for t in times)


def test_a_card_cast_comes_back_with_its_timing_path_and_size() -> None:
    clip = render([CAST], 1.5)
    [card] = measure(clip)
    assert abs(card.start - CAST.start) <= FRAME
    assert abs((card.end - card.start) - CAST.duration) <= FRAME
    peak, _ = card.peak
    assert peak == pytest.approx(true_peak(CAST, clip.times), rel=0.02)
    assert card.rest.size == pytest.approx(math.sqrt(CARD_W * CARD_H), rel=0.02)
    # The path runs from where it left to where it landed.
    first, last = card.blobs[0].center, card.blobs[-1].center
    assert math.dist(first, CAST.origin) < 10
    assert math.dist(last, CAST.target) < 10


DEAL: list[Flight] = [
    Flight(0.4 + 0.15 * n, 0.4, (60, 112), (330, 40 + 72 * n)) for n in range(3)
]


def test_a_staggered_deal_of_three_comes_back_card_by_card() -> None:
    clip = render(DEAL, 1.6)
    cards = measure(clip, cards=3)
    for card, flight in zip(cards, DEAL):
        assert abs(card.start - flight.start) <= FRAME
        assert abs((card.end - card.start) - flight.duration) <= FRAME
        assert card.peak[0] == pytest.approx(1.0, rel=0.02)
        assert math.dist(card.blobs[-1].center, flight.target) < 10
    stagger = [b.start - a.start for a, b in zip(cards, cards[1:])]
    assert all(abs(s - 0.15) <= FRAME for s in stagger)


def test_two_cards_moving_where_one_was_asked_for_cannot_be_measured() -> None:
    clip = render(DEAL[:2], 1.4)
    with pytest.raises(Unmeasurable, match="2 things moved"):
        measure(clip)


def test_a_card_followed_to_where_it_lands_leaves_out_what_moves_around_it() -> None:
    clip = render(DEAL[:2], 1.4)
    [card] = measure(clip, landing=DEAL[1].target)
    assert abs(card.start - DEAL[1].start) <= FRAME
    assert math.dist(card.blobs[-1].center, DEAL[1].target) < 10


def test_a_card_still_moving_at_the_end_cannot_be_measured() -> None:
    clip = render([CAST], 0.9)
    with pytest.raises(Unmeasurable, match="last frame"):
        measure(clip)


def encode(clip: Clip, args: list[str]) -> None:
    subprocess.run(
        ["ffmpeg", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "gray"]
        + ["-s", f"{clip.width}x{clip.height}", "-r", str(FPS), "-i", "-", *args],
        input=b"".join(clip.frames),
        check=True,
    )


NO_FFMPEG: bool = shutil.which("ffmpeg") is None


@pytest.mark.skipif(NO_FFMPEG, reason="needs ffmpeg")
def test_the_command_measures_a_recording_and_a_folder_of_frames(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    clip = render([CAST], 1.5)
    movie = tmp_path / "cast.mp4"
    encode(clip, ["-c:v", "mpeg4", "-q:v", "2", str(movie)])
    folder = tmp_path / "frames"
    folder.mkdir()
    encode(clip, [str(folder / "%03d.png")])
    for n, t in enumerate(clip.times):
        (folder / f"{n + 1:03d}.png").rename(
            folder / f"{round(t * 1000):05d}-yours.png"
        )
        # The other seat's frames are skipped by the glob.
        (folder / f"{round(t * 1000):05d}-theirs.png").write_bytes(b"")

    found: list[dict[str, object]] = []
    for argv in (
        [str(movie), "0", "--span", "2", "--since", "0.3"],
        [str(folder), "--glob", "*-yours.png", "--since", "300"],
    ):
        assert cli.main(["video", "measure", *argv, "--json"]) == 0
        found.append(json.loads(capsys.readouterr().out))
    for result in found:
        [card] = result["cards"]  # type: ignore[misc]
        assert abs(card["start"] - CAST.start) <= FRAME
        assert abs(card["duration_ms"] - 600) <= 1000 * FRAME
        assert abs(card["delay_ms"] - 200) <= 1000 * FRAME
        assert card["peak"] == pytest.approx(true_peak(CAST, clip.times), rel=0.02)
        first, last = card["path"][0], card["path"][-1]
        assert abs(first["x"] - CAST.origin[0] / WIDTH) < 0.03
        assert abs(last["y"] - CAST.target[1] / HEIGHT) < 0.03
        assert Path(str(result["overlay"])).is_file()

    two = render(DEAL[:2], 1.4)
    encode(two, ["-c:v", "mpeg4", "-q:v", "2", str(tmp_path / "two.mp4")])
    assert cli.main(["video", "measure", str(tmp_path / "two.mp4"), "0"]) == 1
    out = capsys.readouterr()
    assert out.out == ""
    assert "cannot measure: 2 things moved" in out.err
