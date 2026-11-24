"""`just client camera-fit`: solves the pitch and vertical field of view of Arena's table camera
from the corners of cards measured in its recordings (docs/beryl-3d.md, 'Space and camera').

data/client/camera-fit.json lists the cards. Each is a rectangle printed on the face of a
card lying on Arena's table, with its four corners in the pixels of a 1920 by 1080 frame,
in the order bottom left, bottom right, top right, top left. A tapped card is one Arena has
turned on the table.

The rectangle's proportions are not known. Arena's battlefield cards are cropped art tiles,
not 63 by 88 cards, and the edges that are sharp enough to measure are those of the art box.
So each card has its own aspect ratio, and the fit uses two facts that hold whatever the
aspect ratio is:

- The sides of every untapped card point at one vanishing point. Its height fixes the
  pitch, given the field of view.
- The edges of a tapped card are at right angles on the table though not on screen. How far
  from a right angle they look fixes the field of view.

The model for each card is a camera that looks at the plane z = 0 from the viewer's side
with no roll and the picture's centre on its line of sight, and a rectangle on that plane
with its own position, size, aspect ratio and, when it is tapped, its own turn. Every
residual is a corner's distance on screen from the measured one, in units of the card's
measured bottom edge, so a near card and a far one count alike. The fit minimizes the sum of
squares by Levenberg-Marquardt, started from each point of a grid over pitch and field of
view, and keeps the best.
"""

from __future__ import annotations

import argparse
import json
import math
from dataclasses import dataclass
from pathlib import Path

from .. import paths

from .command import Command

# The frame the corners are measured in.
FRAME_WIDTH = 1920.0
FRAME_HEIGHT = 1080.0
# The ranges searched, in degrees.
PITCH_RANGE = (5.0, 60.0)
FOV_RANGE = (10.0, 60.0)
# The grid of starting points, in degrees.
SEED_STEP = 5.0
# The camera's distance from the point it looks at, in millimetres. Each card's size is
# free, so this only sets the scale of the solution.
DISTANCE = 1000.0
MAX_ITERATIONS = 200
# A step whose largest change of pitch or field of view is below this, in degrees, ends the
# fit.
TOLERANCE = 1e-5

Point = tuple[float, float]


def fit_path() -> Path:
    return paths.REPO / "data" / "client" / "camera-fit.json"


@dataclass(frozen=True)
class Card:
    recording: str
    time: str
    name: str
    tapped: bool
    corners: tuple[Point, Point, Point, Point]

    def bottom_width(self) -> float:
        (x0, y0), (x1, y1) = self.corners[0], self.corners[1]
        return math.hypot(x1 - x0, y1 - y0)


def load(path: Path) -> list[Card]:
    cards: list[Card] = []
    for entry in json.loads(path.read_text())["cards"]:
        corners = tuple((float(x), float(y)) for x, y in entry["corners"])
        if len(corners) != 4:
            raise ValueError(f"{entry['name']}: a card has four corners")
        cards.append(
            Card(
                recording=str(entry["recording"]),
                time=str(entry["time"]),
                name=str(entry["name"]),
                tapped=bool(entry.get("tapped", False)),
                corners=(corners[0], corners[1], corners[2], corners[3]),
            )
        )
    return cards


@dataclass(frozen=True)
class View:
    """A camera of the model: pitch is the angle between its line of sight and straight
    down, in degrees, and fov its vertical field of view, in degrees."""

    pitch: float
    fov: float

    def project(self, x: float, y: float) -> Point:
        """The point (x, y, 0) of the table on the frame, in pixels from the top left."""
        pitch = math.radians(self.pitch)
        focal = FRAME_HEIGHT / 2 / math.tan(math.radians(self.fov) / 2)
        # The eye is at (0, -sin, cos) * DISTANCE; forward is (0, sin, -cos) and up is
        # (0, cos, sin).
        ry = y + math.sin(pitch) * DISTANCE
        rz = -math.cos(pitch) * DISTANCE
        depth = ry * math.sin(pitch) - rz * math.cos(pitch)
        up = ry * math.cos(pitch) + rz * math.sin(pitch)
        return (
            FRAME_WIDTH / 2 + focal * x / depth,
            FRAME_HEIGHT / 2 - focal * up / depth,
        )

    def unproject(self, point: Point) -> Point:
        """The point of the table that projects to `point`."""
        pitch = math.radians(self.pitch)
        focal = FRAME_HEIGHT / 2 / math.tan(math.radians(self.fov) / 2)
        u = (point[0] - FRAME_WIDTH / 2) / focal
        v = (FRAME_HEIGHT / 2 - point[1]) / focal
        # The ray from the eye: forward + u * right + v * up.
        dy = math.sin(pitch) + v * math.cos(pitch)
        dz = -math.cos(pitch) + v * math.sin(pitch)
        eye_y = -math.sin(pitch) * DISTANCE
        eye_z = math.cos(pitch) * DISTANCE
        t = -eye_z / dz
        return (u * t, eye_y + dy * t)


# A card's own parameters: its centre's x and y on the table, its width, the logarithm of
# its aspect ratio (height over width) and its turn in radians.
CardParameters = list[float]

# A column of the Jacobian: the row of its first stored value, and the values. Rows outside
# them are zero, as a card's own parameters move only its own eight residuals.
Column = tuple[int, list[float]]

SIGNS = ((-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0))


def card_corners(view: View, card: CardParameters) -> list[Point]:
    x, y, width, log_aspect, turn = card
    half_w, half_h = width / 2, width * math.exp(log_aspect) / 2
    c, s = math.cos(turn), math.sin(turn)
    out: list[Point] = []
    for sx, sy in SIGNS:
        dx, dy = sx * half_w, sy * half_h
        out.append(view.project(x + c * dx - s * dy, y + s * dx + c * dy))
    return out


def card_residuals(view: View, card: Card, parameters: CardParameters) -> list[float]:
    scale = card.bottom_width()
    out: list[float] = []
    for (px, py), (mx, my) in zip(card_corners(view, parameters), card.corners):
        out += [(px - mx) / scale, (py - my) / scale]
    return out


def initial_card(view: View, card: Card) -> CardParameters:
    """A card's parameters read straight off its corners under `view`."""
    table = [view.unproject(p) for p in card.corners]
    bottom = math.dist(table[0], table[1])
    left = math.dist(table[0], table[3])
    x = math.fsum(p[0] for p in table) / 4
    y = math.fsum(p[1] for p in table) / 4
    turn = math.atan2(table[1][1] - table[0][1], table[1][0] - table[0][0])
    return [x, y, bottom, math.log(max(left, 1e-6) / max(bottom, 1e-6)), turn]


def solve_linear(matrix: list[list[float]], vector: list[float]) -> list[float] | None:
    """Gaussian elimination with partial pivoting; None when the matrix is singular."""
    n = len(vector)
    a = [row[:] + [vector[i]] for i, row in enumerate(matrix)]
    for col in range(n):
        pivot = max(range(col, n), key=lambda r: abs(a[r][col]))
        if abs(a[pivot][col]) < 1e-300:
            return None
        a[col], a[pivot] = a[pivot], a[col]
        for r in range(col + 1, n):
            f = a[r][col] / a[col][col]
            if f:
                for k in range(col, n + 1):
                    a[r][k] -= f * a[col][k]
    out = [0.0] * n
    for r in range(n - 1, -1, -1):
        out[r] = (a[r][n] - math.fsum(a[r][k] * out[k] for k in range(r + 1, n))) / a[
            r
        ][r]
    return out


@dataclass(frozen=True)
class Fit:
    view: View
    cards: list[CardParameters]
    # The square root of the mean of the squared residuals.
    rms: float
    # One standard deviation of pitch and field of view, in degrees, from the curvature of
    # the sum of squares at the solution.
    pitch_error: float
    fov_error: float


class Problem:
    """The parameters are pitch and field of view, then five per card; a card's untapped
    turn is held at zero."""

    def __init__(self, cards: list[Card]) -> None:
        self.cards = cards

    def free(self, index: int) -> list[int]:
        """The indices of card `index`'s free parameters in its five."""
        return [0, 1, 2, 3, 4] if self.cards[index].tapped else [0, 1, 2, 3]

    def residuals(self, view: View, parameters: list[CardParameters]) -> list[float]:
        out: list[float] = []
        for card, p in zip(self.cards, parameters):
            out += card_residuals(view, card, p)
        return out

    def jacobian(
        self, view: View, parameters: list[CardParameters]
    ) -> tuple[list[float], list[Column], list[tuple[int, int]]]:
        """The residuals, their derivatives by each free parameter (one column each), and
        for each column after the first two, the card and the parameter it is."""
        base = self.residuals(view, parameters)
        columns: list[Column] = []
        h = 1e-6
        for which in ("pitch", "fov"):
            moved = View(
                view.pitch + (h if which == "pitch" else 0.0),
                view.fov + (h if which == "fov" else 0.0),
            )
            columns.append(
                (
                    0,
                    [
                        (a - b) / h
                        for a, b in zip(self.residuals(moved, parameters), base)
                    ],
                )
            )
        index: list[tuple[int, int]] = []
        for i, card in enumerate(self.cards):
            for k in self.free(i):
                p = parameters[i][:]
                step = h * max(1.0, abs(p[k]))
                p[k] += step
                local = card_residuals(view, card, p)
                columns.append(
                    (i * 8, [(v - base[i * 8 + j]) / step for j, v in enumerate(local)])
                )
                index.append((i, k))
        return base, columns, index

    def fit_from(self, seed: View) -> Fit | None:
        view = seed
        parameters = [initial_card(view, c) for c in self.cards]
        for p, c in zip(parameters, self.cards):
            if not c.tapped:
                p[4] = 0.0
        damping = 1e-3
        cost = math.fsum(r * r for r in self.residuals(view, parameters))
        normal: list[list[float]] = []
        done = False
        for _ in range(MAX_ITERATIONS):
            base, columns, index = self.jacobian(view, parameters)
            n = len(columns)
            normal = [[dot(columns[i], columns[j]) for j in range(n)] for i in range(n)]
            gradient = [dot(columns[i], (0, base)) for i in range(n)]
            improved = False
            for _ in range(20):
                damped = [row[:] for row in normal]
                for i in range(n):
                    damped[i][i] += damping * max(normal[i][i], 1e-12)
                step = solve_linear(damped, [-g for g in gradient])
                if step is None:
                    damping *= 10
                    continue
                trial_view = View(view.pitch + step[0], view.fov + step[1])
                trial = [p[:] for p in parameters]
                for (i, k), d in zip(index, step[2:]):
                    trial[i][k] += d
                if not (
                    PITCH_RANGE[0] - 5 <= trial_view.pitch <= PITCH_RANGE[1] + 5
                    and FOV_RANGE[0] - 5 <= trial_view.fov <= FOV_RANGE[1] + 5
                    and all(t[2] > 0 for t in trial)
                ):
                    damping *= 10
                    continue
                trial_cost = math.fsum(r * r for r in self.residuals(trial_view, trial))
                if trial_cost < cost:
                    done = max(abs(step[0]), abs(step[1])) < TOLERANCE
                    view, parameters, cost = trial_view, trial, trial_cost
                    damping = max(damping / 10, 1e-12)
                    improved = True
                    break
                damping *= 10
            if not improved or done:
                break
        count = 8 * len(self.cards)
        if not (
            PITCH_RANGE[0] <= view.pitch <= PITCH_RANGE[1]
            and FOV_RANGE[0] <= view.fov <= FOV_RANGE[1]
        ):
            return None
        free = sum(len(self.free(i)) for i in range(len(self.cards))) + 2
        variance = cost / max(count - free, 1)
        errors = covariance_diagonal(normal, 2)
        return Fit(
            view=view,
            cards=parameters,
            rms=math.sqrt(cost / count),
            pitch_error=math.sqrt(max(errors[0] * variance, 0.0)),
            fov_error=math.sqrt(max(errors[1] * variance, 0.0)),
        )


def dot(a: Column, b: Column) -> float:
    """The dot product of two columns that are zero outside their stored rows."""
    (sa, va), (sb, vb) = a, b
    start, end = max(sa, sb), min(sa + len(va), sb + len(vb))
    return math.fsum(va[r - sa] * vb[r - sb] for r in range(start, end))


def covariance_diagonal(normal: list[list[float]], count: int) -> list[float]:
    """The first `count` diagonal entries of the inverse of `normal`."""
    out: list[float] = []
    for i in range(count):
        unit = [1.0 if j == i else 0.0 for j in range(len(normal))]
        column = solve_linear(normal, unit)
        out.append(column[i] if column is not None else math.inf)
    return out


def solve(cards: list[Card]) -> Fit:
    """The best fit over a grid of starting points."""
    problem = Problem(cards)
    best: Fit | None = None
    pitch = PITCH_RANGE[0]
    while pitch <= PITCH_RANGE[1]:
        fov = FOV_RANGE[0]
        while fov <= FOV_RANGE[1]:
            fit = problem.fit_from(View(pitch, fov))
            if fit is not None and (best is None or fit.rms < best.rms):
                best = fit
            fov += SEED_STEP * 2
        pitch += SEED_STEP * 2
    if best is None:
        raise ValueError("no fit stayed inside the ranges searched")
    return best


def report(cards: list[Card], fit: Fit) -> list[str]:
    lines = [
        f"pitch_degrees {fit.view.pitch:.2f} (± {fit.pitch_error:.2f})",
        f"fov_degrees {fit.view.fov:.2f} (± {fit.fov_error:.2f})",
        f"rms residual {fit.rms:.4f} of a card's bottom edge, over {len(cards)} cards",
    ]
    for card, p in zip(cards, fit.cards):
        r = card_residuals(fit.view, card, p)
        worst = max(math.hypot(r[i], r[i + 1]) for i in range(0, 8, 2))
        rms = math.sqrt(math.fsum(v * v for v in r) / 8)
        tapped = f" tapped {math.degrees(p[4]):+.1f}°" if card.tapped else ""
        lines.append(
            f"  {card.recording} {card.time} {card.name}: rms {rms:.4f}, worst corner "
            f"{worst:.4f}, aspect {math.exp(p[3]):.3f}{tapped}"
        )
    return lines


def run_camera_fit(args: argparse.Namespace) -> int:
    path = Path(args.file) if args.file else fit_path()
    cards = load(path)
    fit = solve(cards)
    for line in report(cards, fit):
        print(line)
    return 0


def camera_fit_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--file", help="the measured cards, by default data/client/camera-fit.json"
    )


COMMANDS: tuple[Command, ...] = (
    Command(
        "camera-fit",
        "solves the desktop camera's pitch and field of view from cards measured in Arena",
        (
            ("just client camera-fit", "fit data/client/camera-fit.json"),
            (
                "just client camera-fit --file cards.json",
                "fit another file of measured cards",
            ),
        ),
        run_camera_fit,
        camera_fit_arguments,
    ),
)
