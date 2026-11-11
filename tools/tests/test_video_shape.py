import math
import random
from dataclasses import dataclass

import pytest

from beryl_tools import video_shape
from beryl_tools.video_measure import Clip, measure

WIDTH, HEIGHT = 400, 225
FPS = 30
CARD_W, CARD_H = 50, 70


@dataclass(frozen=True)
class Throw:
    """A card flying from one center to another over `duration` from `start`, evenly: lifted
    `arc` pixels up the screen at the middle of its flight, turned `turn` degrees clockwise
    there, and its width `narrow` times its own there."""

    origin: tuple[float, float]
    target: tuple[float, float]
    arc: float = 0.0
    turn: float = 0.0
    narrow: float = 1.0
    start: float = 0.4
    duration: float = 0.6

    def at(self, t: float) -> tuple[float, float, float, float]:
        p = min(1.0, max(0.0, (t - self.start) / self.duration))
        middle = math.sin(math.pi * p)
        x = self.origin[0] + (self.target[0] - self.origin[0]) * p
        y = self.origin[1] + (self.target[1] - self.origin[1]) * p - self.arc * middle
        return x, y, self.turn * middle, 1 + (self.narrow - 1) * middle


def render(throw: Throw, length: float = 1.4) -> Clip:
    """Frames of the throw over a noisy table, the card one flat art."""
    rng = random.Random(11)
    table = [rng.randint(30, 100) for _ in range(WIDTH * HEIGHT)]
    times: list[float] = [n / FPS for n in range(round(length * FPS) + 1)]
    frames: list[bytes] = []
    for t in times:
        pixels = list(table)
        cx, cy, turn, narrow = throw.at(t)
        cos, sin = math.cos(math.radians(turn)), math.sin(math.radians(turn))
        reach = math.ceil(math.hypot(CARD_W, CARD_H) / 2)
        for y in range(max(0, round(cy) - reach), min(HEIGHT, round(cy) + reach)):
            for x in range(max(0, round(cx) - reach), min(WIDTH, round(cx) + reach)):
                # The pixel in the card's own frame: turned back about its middle.
                u = (x - cx) * cos + (y - cy) * sin
                v = -(x - cx) * sin + (y - cy) * cos
                if abs(u) <= CARD_W * narrow / 2 and abs(v) <= CARD_H / 2:
                    pixels[y * WIDTH + x] = 200
        frames.append(bytes(pixels))
    return Clip(WIDTH, HEIGHT, times, frames, (WIDTH, HEIGHT))


FLAT = Throw((60, 170), (330, 90))


def test_a_straight_flight_does_not_bow_turn_or_squash() -> None:
    clip = render(FLAT)
    [card] = measure(clip)
    bow = video_shape.card_bow(clip, card)
    shape = video_shape.shape(clip, card)
    assert bow is not None and bow.side == "straight"
    assert shape is not None
    assert abs(shape.turn.value) < 3
    assert shape.squash.value == pytest.approx(1.0, abs=0.06)


def test_a_thrown_card_bows_up_by_its_lift_over_its_chord() -> None:
    clip = render(Throw(FLAT.origin, FLAT.target, arc=40))
    [card] = measure(clip)
    bow = video_shape.card_bow(clip, card)
    assert bow is not None and bow.side == "up"
    assert bow.share == pytest.approx(40 / math.dist(FLAT.origin, FLAT.target), rel=0.1)
    # A path that runs down the screen bows to a side of it.
    points = [(200 + 30 * math.sin(math.pi * n / 10), 30.0 + 16 * n) for n in range(11)]
    found = video_shape.bow(points, [n / FPS for n in range(11)])
    assert found is not None and (found.side, found.share) == ("right", 0.188)


def test_a_card_turning_in_flight_is_read_from_its_pixels() -> None:
    clip = render(Throw(FLAT.origin, FLAT.target, turn=30))
    [card] = measure(clip)
    shape = video_shape.shape(clip, card)
    assert shape is not None
    assert shape.turn.value == pytest.approx(30, abs=4)
    assert shape.squash.value == pytest.approx(1.0, abs=0.1)
    other = render(Throw(FLAT.origin, FLAT.target, turn=-20))
    [card] = measure(other)
    shape = video_shape.shape(other, card)
    assert shape is not None and shape.turn.value == pytest.approx(-20, abs=4)


def test_a_card_turning_over_in_flight_is_squashed() -> None:
    clip = render(Throw(FLAT.origin, FLAT.target, narrow=0.3))
    [card] = measure(clip)
    shape = video_shape.shape(clip, card)
    assert shape is not None
    assert shape.squash.value == pytest.approx(0.3, abs=0.08)
    assert abs(shape.turn.value) < 4
    said = dict(video_shape.words(video_shape.card_bow(clip, card), shape))
    assert said["bow"].endswith("(straight)") and said["squash"].startswith("0.")


def test_a_mover_that_is_no_solid_card_gives_no_turn_or_squash() -> None:
    assert video_shape.sides([bytes([1, 0, 0, 0, 0, 0, 0, 1])] * 8, _box(8, 8)) is None
    assert dict(video_shape.words(None, None)) == {
        "turn": "not shown by its pixels",
        "squash": "not shown by its pixels",
    }


def _box(width: int, height: int) -> video_shape.Blob:
    return video_shape.Blob(0, 0, width, height, width * height)
